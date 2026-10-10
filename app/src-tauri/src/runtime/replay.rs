use super::*;
use maxed_recording::PcapReader;
use std::io::Read;

impl CaptureRuntime {
    pub fn demo(&self, app: tauri::AppHandle) -> Result<(), String> {
        let resources = crate::demo::DemoResources::resolve(&app)?;
        let root = app
            .path()
            .app_data_dir()
            .map_err(|_| "Local app data is unavailable")?;
        self.start_demo(
            root,
            resources,
            Arc::new(crate::demo::RealtimeClock::default()),
            self.publisher(app)?,
        )
    }

    /// Native demo entry point. Uses the same ordered publisher, adapter and
    /// session owner as capture; the clock seam permits deterministic tests.
    pub fn start_demo(
        &self,
        root: std::path::PathBuf,
        resources: crate::demo::DemoResources,
        clock: Arc<dyn crate::demo::DemoClock>,
        publisher: Publisher,
    ) -> Result<(), String> {
        let mut session = self.session.lock().map_err(|_| WORKER_FAILED)?;
        if session.is_some() || self.closing.load(Ordering::Acquire) {
            return Err("Stop capture before starting demo".into());
        }
        if self.shared.failed.load(Ordering::Acquire) || self.has_unsaved_summaries() {
            return Err("Save pending summaries or restart Maxed before starting demo".into());
        }
        let mut options = RuntimeOptions::live(root, None);
        options.namespace = Namespace::Demo;
        let cancel = Arc::new(AtomicBool::new(false));
        let mut adapter = self.shared.open_home(
            &options,
            &cancel,
            Self::domain_bridge(publisher.clone()),
            Some(resources.catalog),
        )?;
        *self.publisher.lock().map_err(|_| WORKER_FAILED)? = Some(publisher.clone());
        let sink: StatusSink =
            Arc::new(move |status| publisher.publish(Publication::Capture(status)));
        prepare_demo(&mut adapter, &self.shared, &sink)?;
        let (stop, stopped) = bounded(1);
        let shared = self.shared.clone();
        let closing = self.closing.clone();
        let worker_cancel = cancel.clone();
        let worker_sink = sink.clone();
        let worker = thread::Builder::new()
            .name("maxed-demo".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    play_demo(
                        resources.story,
                        clock.as_ref(),
                        &stopped,
                        worker_cancel,
                        &shared,
                        &worker_sink,
                        &mut adapter,
                    )
                }));
                let failed = !matches!(result, Ok(Ok(())));
                let reason = if closing.load(Ordering::Acquire) {
                    FightEnd::AppClosed
                } else {
                    FightEnd::CaptureStopped
                };
                let finalized = adapter.finish(adapter.at(), reason);
                let terminal = CaptureStatus {
                    state: if failed {
                        CaptureState::Failed
                    } else {
                        CaptureState::Stopped
                    },
                    ..Default::default()
                };
                shared.publish(terminal, &worker_sink);
                let captured = adapter.capture(shared.status());
                adapter.publish_snapshot();
                if finalized.is_err() || captured.is_err() || adapter.close().is_err() {
                    adapter.refresh_store();
                    *shared.recovery.lock().unwrap_or_else(|p| p.into_inner()) = Some(adapter);
                }
            })
            .map_err(|_| {
                self.shared.publish(CaptureStatus::default(), &sink);
                "Synthetic demo worker could not start"
            })?;
        *session = Some(Session {
            stop,
            cancel,
            worker,
            sink,
        });
        Ok(())
    }

    /// Gate both OS notifications and sound with the originating stream.
    /// Retired live streams and every demo/replay stream are ineligible.
    pub fn external_delivery_allowed(&self, stream_id: &str) -> bool {
        !self.closing.load(Ordering::Acquire)
            && stream_id.starts_with("live-")
            && self
                .shared
                .views
                .snapshot
                .lock()
                .is_ok_and(|snapshot| snapshot.stream_id == stream_id)
            && matches!(
                self.shared.status().state,
                CaptureState::Live | CaptureState::WaitingForGame | CaptureState::Recovering
            )
            && self
                .shared
                .views
                .store
                .lock()
                .is_ok_and(|status| !matches!(status.state, maxed_types::StoreState::Failed))
    }

    /// Blocking worker entry point for replay orchestration; no notification
    /// delivery exists on this path. A caller cannot target the live namespace.
    pub fn replay(
        &self,
        app: tauri::AppHandle,
        options: RuntimeOptions,
        path: std::path::PathBuf,
    ) -> Result<(), String> {
        if options.namespace != Namespace::Replay {
            return Err("Replay requires its own namespace".into());
        }
        let file = std::fs::File::open(path).map_err(|_| "Replay could not be opened")?;
        self.replay_reader(
            options,
            std::io::BufReader::new(file),
            Self::domain_bridge(self.publisher(app)?),
        )
    }

    pub(super) fn replay_reader(
        &self,
        options: RuntimeOptions,
        reader: impl Read,
        bridge: Bridge,
    ) -> Result<(), String> {
        let session = self.session.lock().map_err(|_| WORKER_FAILED)?;
        if session.is_some() || self.closing.load(Ordering::Acquire) {
            return Err("Stop capture before replay".into());
        }
        if self
            .shared
            .recovery
            .lock()
            .map_err(|_| WORKER_FAILED)?
            .is_some()
        {
            return Err("Save pending summaries before replay".into());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let mut adapter = self.shared.open_home(&options, &cancel, bridge, None)?;
        let mut pipeline = Pipeline::new(cancel);
        let presentation_clock = Instant::now();
        let result = (|| {
            let mut reader = PcapReader::new(reader).map_err(|_| "Replay format is unsupported")?;
            while let Some(frame) = reader.next_frame().map_err(|_| "Replay data is invalid")? {
                if self.closing.load(Ordering::Acquire) {
                    break;
                }
                if maxed_capture::parse_frame(&frame)
                    .ok()
                    .flatten()
                    .is_some_and(|s| !s.bytes.is_empty())
                {
                    adapter.traffic(frame.at)?;
                }
                for event in pipeline.push(frame) {
                    adapter.decoded(event)?;
                }
                adapter.publish_due(presentation_clock.elapsed());
            }
            Ok::<_, String>(())
        })();
        for event in pipeline.finish() {
            let _ = adapter.decoded(event);
        }
        let reason = if self.closing.load(Ordering::Acquire) {
            FightEnd::AppClosed
        } else {
            FightEnd::CaptureStopped
        };
        let end = adapter.finish(adapter.at(), reason);
        if end.is_err() || adapter.close().is_err() {
            adapter.refresh_store();
            *self.shared.recovery.lock().map_err(|_| WORKER_FAILED)? = Some(adapter);
            return Err("Replay summaries are not saved; retry storage".into());
        }
        result
    }

    pub fn reset_preview(&self) -> Result<maxed_types::LiveSnapshot, String> {
        let session = self.session.lock().map_err(|_| WORKER_FAILED)?;
        if session.is_some()
            || self
                .shared
                .recovery
                .lock()
                .map_err(|_| WORKER_FAILED)?
                .is_some()
        {
            return Err("Stop capture and save pending changes before leaving preview".into());
        }
        let mut alerts = self
            .shared
            .alerts
            .lock()
            .map_err(|_| "Alert settings unavailable")?;
        let mut delivery = self
            .shared
            .views
            .delivery
            .lock()
            .map_err(|_| "Alert delivery unavailable")?;
        delivery.retire();
        drop(delivery); // History preparation/readers must not block alert commands.
        self.shared.home.restore_live()?;
        alerts.namespace = Namespace::Live;
        let defaults = Views::default();
        defaults.snapshot.lock().unwrap().stream_id = format!("stopped-{}", storage::random_id()?);
        *self
            .shared
            .views
            .snapshot
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = defaults.snapshot.into_inner().unwrap();
        *self
            .shared
            .views
            .store
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = defaults.store.into_inner().unwrap();
        Ok(self.snapshot())
    }
}

fn prepare_demo(adapter: &mut Adapter, shared: &Shared, sink: &StatusSink) -> Result<(), String> {
    shared.publish(
        CaptureStatus {
            state: CaptureState::WaitingForGame,
            ..Default::default()
        },
        sink,
    );
    adapter.capture(shared.status()).inspect_err(|_| {
        shared.publish(CaptureStatus::default(), sink);
    })?;
    adapter.publish_snapshot();
    Ok(())
}

fn play_demo(
    story: Vec<u8>,
    clock: &dyn crate::demo::DemoClock,
    stopped: &Receiver<()>,
    cancel: Arc<AtomicBool>,
    shared: &Shared,
    sink: &StatusSink,
    adapter: &mut Adapter,
) -> Result<(), String> {
    let mut pipeline = Pipeline::new(cancel.clone());
    let result = (|| {
        let mut reader = PcapReader::new(std::io::Cursor::new(story))
            .map_err(|_| "Synthetic demo format is unsupported")?;
        let mut first = None;
        'frames: while let Some(frame) = reader
            .next_frame()
            .map_err(|_| "Synthetic demo data is invalid")?
        {
            let origin = *first.get_or_insert(frame.at);
            let due = Duration::from_micros(frame.at.0 - origin.0);
            loop {
                // A burst can finish decoding just after the snapshot gate closes.
                // Keep the live worker's presentation cadence during packet gaps.
                let wake = due.min(clock.elapsed().saturating_add(Duration::from_millis(250)));
                if cancel.load(Ordering::Acquire)
                    || !clock.wait_until(wake, stopped)
                    || cancel.load(Ordering::Acquire)
                {
                    break 'frames;
                }
                let elapsed = clock.elapsed();
                adapter.publish_due(elapsed);
                adapter.refresh_store();
                if elapsed >= due {
                    break;
                }
            }
            if maxed_capture::parse_frame(&frame)
                .ok()
                .flatten()
                .is_some_and(|s| !s.bytes.is_empty())
            {
                adapter.traffic(frame.at)?;
            }
            for event in pipeline.push(frame) {
                let state = match &event {
                    PipelineEvent::Decoded(DecodeEvent::Packet(packet))
                        if !matches!(packet.packet, Packet::Unknown { .. }) =>
                    {
                        Some(CaptureState::Live)
                    }
                    PipelineEvent::Decoded(DecodeEvent::Diagnostic {
                        kind: Diagnostic::Recovering,
                        ..
                    }) => Some(CaptureState::Recovering),
                    _ => None,
                };
                adapter.decoded(event)?;
                if let Some(state) = state {
                    shared.publish(
                        CaptureStatus {
                            state,
                            ..shared.status()
                        },
                        sink,
                    );
                    adapter.capture(shared.status())?;
                }
            }
            adapter.publish_due(clock.elapsed());
            adapter.refresh_store();
        }
        Ok(())
    })();
    // Final decoder diagnostics must be delivered on EOF, cancellation and error.
    let mut closed = Ok(());
    for event in pipeline.finish() {
        if let Err(error) = adapter.decoded(event) {
            closed = Err(error);
        }
    }
    result.and(closed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_initial_capture_restores_stopped_query_and_publication() {
        let root = tempfile::tempdir().unwrap();
        let runtime = CaptureRuntime::default();
        let mut options = RuntimeOptions::live(root.path().into(), None);
        options.namespace = Namespace::Demo;
        let mut adapter = options
            .open(
                &AtomicBool::new(false),
                runtime.shared.views.clone(),
                Bridge::default(),
            )
            .unwrap();
        adapter.close().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let output = seen.clone();
        let sink: StatusSink = Arc::new(move |status| output.lock().unwrap().push(status));
        assert_eq!(
            prepare_demo(&mut adapter, &runtime.shared, &sink).unwrap_err(),
            "Summary writer is closed"
        );
        runtime.stop().unwrap();
        runtime.reset_preview().unwrap();
        assert!(!runtime.is_running());
        assert_eq!(runtime.status().state, CaptureState::Stopped);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.first().unwrap().state, CaptureState::WaitingForGame);
        assert_eq!(seen.last().unwrap().state, CaptureState::Stopped);
    }
}
