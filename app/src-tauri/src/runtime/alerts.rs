//! Native alert policy, delivery worker and calendar seams.
use super::{CaptureRuntime, Namespace};
use maxed_types::*;
use std::sync::Arc;
pub mod calendar;
pub mod delivery;
#[cfg(test)]
mod delivery_tests;
mod native;
pub(super) mod notifications;

/// Separate from history readiness: startup/schema failures do not prevent
/// settings edits. Only blocking lifecycle/settings workers take this lock.
/// Lifecycle code takes alerts before history locks; settings code takes alerts
/// before SettingsStore and policy. History readers never acquire alerts, and
/// no history lock spans settings file I/O.
pub(super) struct AlertSettingsState {
    pub settings: Option<Arc<crate::settings::SettingsStore>>,
    pub namespace: Namespace,
}
impl Default for AlertSettingsState {
    fn default() -> Self {
        Self {
            settings: None,
            namespace: Namespace::Live,
        }
    }
}

pub trait AlertCalendar: Send + Sync {
    fn seven_day_window(&self, at: Timestamp) -> Result<TimeRange, String>;
}
/// Alert delivery injection seam. Notifications supplied here must be silent.
pub trait AlertSink: Send + Sync {
    fn play(&self, kind: AlertKind, volume_percent: u8) -> Result<(), String>;
    fn notify(&self, alert: &AlertView) -> Result<(), String>;
    fn stop_sound(&self);
}
impl CaptureRuntime {
    pub fn configure_alert_settings(
        &self,
        settings: Arc<crate::settings::SettingsStore>,
    ) -> Result<(), String> {
        self.shared
            .alerts
            .lock()
            .map_err(|_| "Alert settings unavailable")?
            .settings = Some(settings.clone());
        *self
            .shared
            .views
            .chat_policy
            .lock()
            .map_err(|_| "Chat settings unavailable")? = settings
            .get()
            .map(|settings| settings.chat)
            .unwrap_or_default();
        // Preparation must not take the alerts lock: lifecycle callers hold it
        // while waiting for preparation to finish.
        *self
            .shared
            .home
            .settings
            .lock()
            .map_err(|_| "Alert settings unavailable")? = Some(settings);
        Ok(())
    }
    pub fn alert_delivery_status(&self) -> AlertDeliveryStatus {
        let delivery = self
            .shared
            .views
            .delivery
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let mut status = delivery.status();
        let notifier = delivery.sink.as_ref().map(|sink| sink.notifier());
        drop(delivery);
        if let Some(notifier) = notifier {
            status.permission = notifier.refresh_permission();
        }
        status
    }
    pub fn test_alert_delivery(&self, kind: AlertKind) -> Result<AlertDeliveryStatus, String> {
        self.test_alert_delivery_with_catalog(kind, None)
    }
    pub(crate) fn test_alert_delivery_with_catalog(
        &self,
        kind: AlertKind,
        catalog: Option<&Catalog>,
    ) -> Result<AlertDeliveryStatus, String> {
        let details = if kind == AlertKind::KeyPopped {
            catalog.and_then(|catalog| {
                let dungeons = || {
                    catalog
                        .dungeons
                        .iter()
                        .filter(|d| d.kind == AreaKind::Dungeon)
                };
                dungeons()
                    .find(|d| d.portal_type.is_some())
                    .or_else(|| dungeons().next())
                    .map(|d| AlertDetails::KeyPopped {
                        dungeon: d.name.clone(),
                    })
            })
        } else {
            None
        };
        let mut delivery = self
            .shared
            .views
            .delivery
            .lock()
            .map_err(|_| "Alert delivery unavailable")?;
        if !delivery.available() {
            return Err("Alert delivery unavailable".into());
        }
        let stream = delivery
            .stream
            .clone()
            .ok_or("Test notification requires active live capture")?;
        if !self.external_delivery_allowed(&stream)
            || matches!(kind, AlertKind::FlawlessRun | AlertKind::Death)
        {
            return Err(
                "Test notification requires active live capture and a supported cue".into(),
            );
        }
        let policy = self
            .shared
            .views
            .alert_policy
            .lock()
            .map_err(|_| "Alert policy unavailable")?;
        let at = delivery
            .event_now(&stream, std::time::Instant::now())
            .ok_or("Capture clock unavailable; wait for game traffic before Test")?;
        let loudness = policy.loudness(&kind);
        delivery.sample = Some(delivery::Pending {
            alert: maxed_store::CommittedAlert {
                view: AlertView {
                    id: "native-test".into(),
                    at,
                    kind,
                    run_id: None,
                    explanation: "Test notification".into(),
                    details,
                    presentation: Some(loudness),
                },
                source_stream_id: stream,
                source_sequence: Sequence(Timestamp(0)),
                loudness,
                committed_at: at,
            },
            due: std::time::Instant::now(),
            sample: true,
        });
        delivery.wake.signal();
        Ok(delivery.status())
    }
    pub fn update_alert_preferences(&self, preferences: AlertPreferences) -> Result<(), String> {
        if !preferences.valid() {
            return Err(crate::settings::SETTINGS_INVALID.into());
        }
        // Serialize against policy loading and namespace transitions without
        // holding history admission locks during the file lease/write/fsync.
        let alerts = self
            .shared
            .alerts
            .lock()
            .map_err(|_| "Alert settings unavailable")?;
        if alerts.namespace != Namespace::Live {
            return Err(
                "Live alert preferences cannot be changed during preview or replay.".into(),
            );
        }
        let settings = alerts
            .settings
            .as_ref()
            .ok_or("Alert settings unavailable")?;
        #[cfg(test)]
        if let Some(hook) = self.shared.on_alert_policy_save.lock().unwrap().clone() {
            hook();
        }
        settings.update_alert_preferences(preferences.clone())?;
        let mut delivery = self
            .shared
            .views
            .delivery
            .lock()
            .map_err(|_| "Alert delivery unavailable")?;
        delivery.wake.signal();
        delivery.sample = None;
        delivery.cancel_notifications();
        if let Some(sink) = &mut delivery.sink {
            sink.stop();
        }
        if delivery.waiting.as_ref().is_some_and(|pending| {
            matches!(
                delivery::effective(
                    &pending.alert,
                    &preferences,
                    delivery
                        .event_now(&pending.alert.source_stream_id, std::time::Instant::now())
                        .unwrap_or(Timestamp(0))
                ),
                AlertLoudness::Off | AlertLoudness::QuietLogOnly
            )
        }) {
            delivery.waiting = None;
        }
        *self
            .shared
            .views
            .alert_policy
            .lock()
            .map_err(|_| "Alert policy unavailable")? = preferences;
        Ok(())
    }
}
