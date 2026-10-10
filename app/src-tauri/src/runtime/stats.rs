//! Stats history reads and CSV export. The CSV is built and written here; the
//! WebView receives only the row count.
use super::history::{HistoryCaller, HistoryKey};
use super::{stats_csv, CaptureRuntime};
use maxed_types::*;
use std::{collections::HashMap, io::Write, path::PathBuf, sync::Arc};
use tokio::sync::oneshot;

// Fixed messages: never a path, file contents or player names.
const EXPORT_DIALOG: &str = "The save dialog could not open; try again.";
const EXPORT_BUSY: &str = "Saved history is busy; export again.";
const EXPORT_SUPERSEDED: &str = "Saved history changed; export again.";
const EXPORT_WRITE: &str = "The CSV file could not be saved; choose another location.";

/// The native Save dialog, behind a trait so tests can answer it.
pub(crate) trait SaveDialog: Send + Sync {
    /// `None` when the user cancels.
    fn choose(&self, suggested: String) -> oneshot::Receiver<Option<PathBuf>>;
}

pub(crate) struct TauriSaveDialog(pub tauri::AppHandle);
impl SaveDialog for TauriSaveDialog {
    fn choose(&self, suggested: String) -> oneshot::Receiver<Option<PathBuf>> {
        use tauri::Manager;
        use tauri_plugin_dialog::DialogExt;
        let (send, receive) = oneshot::channel();
        let mut builder = self
            .0
            .dialog()
            .file()
            .set_file_name(suggested)
            .add_filter("CSV", &["csv"]);
        if let Some(window) = self.0.get_webview_window("main") {
            builder = builder.set_parent(&window);
        }
        builder.save_file(move |path| {
            let _ = send.send(path.and_then(|p| p.into_path().ok()));
        });
        receive
    }
}

enum ExportData {
    Maps(Vec<StatsMapRow>),
    Sessions(Vec<StatsSessionRow>),
    Dungeons(Vec<StatsDungeonRow>),
    Party(Vec<maxed_store::SessionRunParty>),
}

fn export_read(
    reader: &maxed_store::StoreReader,
    request: StatsExportRequest,
) -> Result<ExportData, maxed_store::StoreError> {
    Ok(match request.kind {
        StatsExportKind::Maps => ExportData::Maps(reader.stats_maps(request.filter)?.rows),
        StatsExportKind::Dungeons => {
            ExportData::Dungeons(reader.stats_dungeons(request.filter)?.rows)
        }
        StatsExportKind::Sessions => ExportData::Sessions(
            reader
                .stats_sessions_export(request.filter, request.include_empty)?
                .0,
        ),
        StatsExportKind::SessionRuns { id } => ExportData::Party(
            reader.stats_session_party(&id, request.filter.character_id.as_deref())?,
        ),
    })
}

fn empty(kind: &StatsExportKind) -> ExportData {
    match kind {
        StatsExportKind::Maps => ExportData::Maps(vec![]),
        StatsExportKind::Sessions => ExportData::Sessions(vec![]),
        StatsExportKind::Dungeons => ExportData::Dungeons(vec![]),
        StatsExportKind::SessionRuns { .. } => ExportData::Party(vec![]),
    }
}

/// Write beside the destination, then rename, so a failed export never leaves half a file.
fn write_csv(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let directory = path.parent().ok_or(EXPORT_WRITE)?;
    let mut file = tempfile::NamedTempFile::new_in(directory).map_err(|_| EXPORT_WRITE)?;
    file.write_all(bytes).map_err(|_| EXPORT_WRITE)?;
    file.as_file().sync_all().map_err(|_| EXPORT_WRITE)?;
    file.persist(path).map_err(|_| EXPORT_WRITE)?;
    Ok(())
}

impl CaptureRuntime {
    pub async fn stats_maps(
        self: &Arc<Self>,
        filter: StatsFilter,
    ) -> Result<HistoryReply<StatsMaps>, String> {
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::StatsMaps(filter.clone()),
            move |job| {
                runtime.read_history(
                    job,
                    || StatsMaps {
                        rows: vec![],
                        truncated: false,
                        tracking_since: None,
                        characters: vec![],
                    },
                    |reader| reader.stats_maps(filter),
                )
            },
        )
        .await
    }

    pub async fn stats_sessions(
        self: &Arc<Self>,
        filter: StatsSessionFilter,
    ) -> Result<HistoryReply<StatsSessionPage>, String> {
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::StatsSessions(filter.clone()),
            move |job| {
                runtime.read_history(
                    job,
                    || StatsSessionPage {
                        rows: vec![],
                        next: None,
                        characters: vec![],
                    },
                    |reader| reader.stats_sessions(filter),
                )
            },
        )
        .await
    }

    pub async fn stats_session(
        self: &Arc<Self>,
        request: StatsSessionRequest,
    ) -> Result<HistoryReply<Option<StatsSessionDetail>>, String> {
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::StatsSession(request.clone()),
            move |job| runtime.read_history(job, || None, |reader| reader.stats_session(request)),
        )
        .await
    }

    pub async fn stats_dungeons(
        self: &Arc<Self>,
        filter: StatsFilter,
    ) -> Result<HistoryReply<StatsDungeons>, String> {
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::StatsDungeons(filter.clone()),
            move |job| {
                runtime.read_history(
                    job,
                    || StatsDungeons {
                        rows: vec![],
                        truncated: false,
                        characters: vec![],
                    },
                    |reader| reader.stats_dungeons(filter),
                )
            },
        )
        .await
    }

    pub(crate) async fn export_stats_csv(
        self: &Arc<Self>,
        dialog: &dyn SaveDialog,
        request: StatsExportRequest,
        class_names: impl FnOnce(&[u16]) -> HashMap<u16, String> + Send + 'static,
    ) -> Result<StatsExportResult, String> {
        let key = HistoryKey::StatsExport(request.clone());
        key.validate()?;
        let suggested = stats_csv::file_name(&request.kind, chrono::Local::now().date_naive());
        let Some(path) = dialog.choose(suggested).await.map_err(|_| EXPORT_DIALOG)? else {
            return Ok(StatsExportResult::Cancelled);
        };
        let generation = self
            .shared
            .home
            .generation
            .load(std::sync::atomic::Ordering::Acquire);
        let ensure_current = || {
            let queue = self
                .shared
                .home
                .scheduler
                .lock()
                .map_err(|_| super::home::UNAVAILABLE)?;
            if queue.stopped()
                || self
                    .shared
                    .home
                    .generation
                    .load(std::sync::atomic::Ordering::Acquire)
                    != generation
            {
                return Err(EXPORT_SUPERSEDED.to_owned());
            }
            Ok(())
        };
        let mut attempt = 0;
        let data = loop {
            ensure_current()?;
            attempt += 1;
            let runtime = self.clone();
            let read = request.clone();
            let reply = self
                .submit_history(HistoryCaller::MainWindow, key.clone(), move |job| {
                    if job.context.generation != generation {
                        return Err(EXPORT_SUPERSEDED.into());
                    }
                    let fallback = empty(&read.kind);
                    runtime.read_history(job, || fallback, |reader| export_read(reader, read))
                })
                .await;
            ensure_current()?;
            let (error, delay) = match reply? {
                HistoryReply::Ready { value } => break value,
                HistoryReply::Busy => (EXPORT_BUSY, std::time::Duration::from_secs(1)),
                HistoryReply::Superseded => {
                    (EXPORT_SUPERSEDED, std::time::Duration::from_millis(25))
                }
            };
            if attempt == 3 {
                return Err(error.into());
            }
            tokio::time::sleep(delay).await;
        };
        tauri::async_runtime::spawn_blocking(move || {
            let zone = chrono::Local;
            let table = match &data {
                ExportData::Maps(rows) => stats_csv::maps(rows),
                ExportData::Sessions(rows) => stats_csv::sessions(&zone, rows),
                ExportData::Dungeons(rows) => stats_csv::dungeons(&zone, rows),
                ExportData::Party(party) => {
                    let mut ids: Vec<u16> = party
                        .iter()
                        .filter_map(|p| p.totals.as_ref())
                        .flat_map(|t| t.players.iter().filter_map(|p| p.class_id))
                        .collect();
                    ids.sort_unstable();
                    ids.dedup();
                    stats_csv::party(&zone, party, &class_names(&ids))
                }
            };
            let rows = u32::try_from(table.rows.len()).map_err(|_| EXPORT_WRITE)?;
            let bytes = stats_csv::document(&table);
            write_csv(&path, &bytes)?;
            Ok(StatsExportResult::Saved { rows })
        })
        .await
        .map_err(|_| EXPORT_WRITE)?
    }
}
