//! Fixed, embedded, hash-checked PCM assets. No UI paths or remote inputs.
use super::{
    delivery::{DeliveryNotifier, DeliverySink},
    *,
};
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager;

const WHITE: &[u8] = include_bytes!("../../../resources/sounds/white_bag.wav");
const PB: &[u8] = include_bytes!("../../../resources/sounds/personal_best.wav");
const KEY: &[u8] = include_bytes!("../../../resources/sounds/key_popped.wav");
const CHAT: &[u8] = include_bytes!("../../../resources/sounds/chat_message.wav");
const FLAWLESS: &[u8] = include_bytes!("../../../resources/sounds/flawless.wav");
const INVALID: &str = "Bundled alert sound failed validation.";

#[derive(Debug, PartialEq)]
enum DeviceAction {
    Keep,
    Reopen,
    Unavailable,
}

fn device_action(
    open: Option<&rodio::cpal::DeviceId>,
    // Outer None: no default device. Inner None: its ID could not be read.
    current: Option<Option<&rodio::cpal::DeviceId>>,
) -> DeviceAction {
    match current {
        None => DeviceAction::Unavailable,
        Some(None) => DeviceAction::Keep,
        Some(Some(id)) if open == Some(id) => DeviceAction::Keep,
        Some(_) => DeviceAction::Reopen,
    }
}

pub(super) fn volume_gain(percent: u8) -> f32 {
    f32::from(percent.min(100)) / 100.0
}
pub(super) fn asset(kind: &AlertKind) -> (&'static [u8], &'static str) {
    match kind {
        AlertKind::WhiteBag => (
            WHITE,
            "3a075e4485d343d4ed656b96f8b82a6be485b5c50e9a09a52e8d21fcfef00a2e",
        ),
        AlertKind::PersonalBest => (
            PB,
            "d9c615cf12554613714d6f9190382d2a922dfbb6e5b616f6afe4628a1161d10a",
        ),
        AlertKind::KeyPopped => (
            KEY,
            "760c8b6f53669900c781cdb0da8364b11b4999fea7e56545fc87f5ea83f7017a",
        ),
        AlertKind::Whisper | AlertKind::GuildMessage | AlertKind::PartyMessage => (
            CHAT,
            "3ecd2cd25d85653cdc88bae9043332ccc6489b364024d378f8bb6a387a6187ef",
        ),
        AlertKind::Death => (&[], ""),
        AlertKind::FlawlessRun => (
            FLAWLESS,
            "8055698f2b10a4f1cc6cda71339fa223f792fdbc80b6dec9655b42c684579c25",
        ),
    }
}
pub(super) fn validate(bytes: &[u8], hash: &str) -> Result<(), String> {
    if !(44..=300_000).contains(&bytes.len())
        || bytes.get(..4) != Some(b"RIFF")
        || bytes.get(8..12) != Some(b"WAVE")
        || format!("{:x}", Sha256::digest(bytes)) != hash
    {
        return Err(INVALID.into());
    }
    Ok(())
}
pub(super) struct NativeSink {
    notifier: Arc<NativeNotifier>,
    stream: Option<rodio::MixerDeviceSink>,
    device_id: Option<rodio::cpal::DeviceId>,
    player: Option<rodio::Player>,
    failed: Arc<AtomicBool>,
}
struct NativeNotifier {
    app: tauri::AppHandle,
    notifications: super::notifications::Notifications,
}
impl DeliveryNotifier for NativeNotifier {
    fn refresh_permission(&self) -> NotificationPermission {
        self.notifications.refresh_permission()
    }
    fn focused(&self) -> Result<bool, String> {
        self.app
            .get_webview_window("main")
            .ok_or("Main window unavailable")?
            .is_focused()
            .map_err(|_| "Window focus unavailable".into())
    }
    fn notify_silent(&self, alert: &AlertView) -> Result<(), String> {
        #[cfg(windows)]
        {
            // Called on maxed-alerts, with delivery unlocked. cache() clones the
            // catalog + generation lease and releases the asset lock immediately.
            let cache = self
                .app
                .try_state::<Arc<crate::setup::SetupService>>()
                .and_then(|setup| setup.assets.cache());
            self.notifications.show(alert, cache.as_ref())
        }
        #[cfg(not(windows))]
        self.notifications.show(alert)
    }
}
impl NativeSink {
    pub fn new(app: tauri::AppHandle) -> Self {
        let mut value = Self {
            notifier: Arc::new(NativeNotifier {
                app,
                notifications: super::notifications::Notifications::new(),
            }),
            stream: None,
            device_id: None,
            player: None,
            failed: Arc::new(AtomicBool::new(false)),
        };
        let _ = value.recover();
        value
    }
}
impl DeliverySink for NativeSink {
    fn play(&mut self, kind: AlertKind, volume: u8) -> Result<(), String> {
        if !self.sound_available() {
            return Err("Sound device unavailable; use Test notification to retry.".into());
        }
        let (bytes, hash) = asset(&kind);
        validate(bytes, hash)?;
        self.stop();
        let stream = self.stream.as_ref().ok_or("Sound device unavailable")?;
        let source = rodio::Decoder::try_from(std::io::Cursor::new(bytes)).map_err(|_| INVALID)?;
        let player = rodio::Player::connect_new(stream.mixer());
        player.set_volume(volume_gain(volume));
        player.append(source);
        self.player = Some(player);
        Ok(())
    }
    fn notifier(&self) -> Arc<dyn DeliveryNotifier> {
        self.notifier.clone()
    }
    fn permission(&self) -> NotificationPermission {
        self.notifier.notifications.permission()
    }
    fn sound_available(&self) -> bool {
        self.stream.is_some() && !self.failed.load(Ordering::Acquire)
    }
    fn device_stale(&self) -> bool {
        let device = rodio::cpal::default_host().default_output_device();
        let device_id = device.as_ref().and_then(|device| device.id().ok());
        device_action(
            self.device_id.as_ref(),
            device.as_ref().map(|_| device_id.as_ref()),
        ) != DeviceAction::Keep
    }
    fn playing(&self) -> bool {
        self.sound_available() && self.player.as_ref().is_some_and(|p| !p.empty())
    }
    fn stop(&mut self) {
        if let Some(player) = self.player.take() {
            player.stop();
        }
    }
    fn recover(&mut self) -> Result<(), String> {
        let device = rodio::cpal::default_host().default_output_device();
        let device_id = device.as_ref().and_then(|device| device.id().ok());
        if self.sound_available()
            && device_action(
                self.device_id.as_ref(),
                device.as_ref().map(|_| device_id.as_ref()),
            ) == DeviceAction::Keep
        {
            return Ok(());
        }
        self.stop();
        self.stream = None;
        self.device_id = None;
        // A fresh flag prevents a retiring stream's callback from failing its replacement.
        self.failed = Arc::new(AtomicBool::new(false));
        let failed = self.failed.clone();
        let device = device.ok_or("Sound device unavailable; use Test notification to retry.")?;
        self.stream = rodio::DeviceSinkBuilder::from_device(device)
            .and_then(|builder| {
                builder
                    .with_error_callback(move |_| {
                        failed.store(true, Ordering::Release);
                    })
                    .open_stream()
            })
            .ok();
        if self.stream.is_none() {
            return Err("Sound device unavailable; use Test notification to retry.".into());
        }
        self.device_id = device_id;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_kinds_share_a_valid_distinct_cue() {
        let whisper = asset(&AlertKind::Whisper);
        assert_ne!(whisper.0, asset(&AlertKind::KeyPopped).0);
        for kind in [
            AlertKind::Whisper,
            AlertKind::GuildMessage,
            AlertKind::PartyMessage,
        ] {
            let cue = asset(&kind);
            assert_eq!(cue, whisper);
            assert_eq!(validate(cue.0, cue.1), Ok(()));
        }
    }

    #[test]
    fn default_device_decision() {
        let host = rodio::cpal::default_host().id();
        let speakers = rodio::cpal::DeviceId(host, "speakers".into());
        let headset = rodio::cpal::DeviceId(host, "headset".into());
        assert_eq!(
            device_action(Some(&speakers), Some(Some(&speakers))),
            DeviceAction::Keep
        );
        assert_eq!(
            device_action(Some(&speakers), Some(Some(&headset))),
            DeviceAction::Reopen
        );
        assert_eq!(
            device_action(None, Some(Some(&headset))),
            DeviceAction::Reopen
        );
        assert_eq!(
            device_action(Some(&speakers), None),
            DeviceAction::Unavailable
        );
        assert_eq!(device_action(None, None), DeviceAction::Unavailable);
        assert_eq!(
            device_action(Some(&speakers), Some(None)),
            DeviceAction::Keep
        );
        assert_eq!(device_action(None, Some(None)), DeviceAction::Keep);
    }
}
