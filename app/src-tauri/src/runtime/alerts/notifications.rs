//! Fixed text and per-user identity for installed and portable Windows toasts.
use super::*;

#[cfg(windows)]
fn permission_from_setting(
    setting: windows::core::Result<windows::UI::Notifications::NotificationSetting>,
) -> NotificationPermission {
    use windows::UI::Notifications::NotificationSetting;
    match setting {
        Ok(NotificationSetting::Enabled) => NotificationPermission::Granted,
        Ok(_) => NotificationPermission::Denied,
        // Windows has no setting for a registered AUMID until its first toast.
        Err(error) if error.code().0 == 0x80070490u32 as i32 => NotificationPermission::Granted,
        Err(_) => NotificationPermission::Unavailable,
    }
}

pub(in crate::runtime) fn content(alert: &AlertView) -> (String, String) {
    if alert.kind == AlertKind::PersonalBest {
        match &alert.details {
            Some(AlertDetails::RunTimePersonalBest {
                dungeon,
                duration_micros,
                previous_micros,
            }) => {
                let elapsed =
                    |micros: u64| format!("{}:{:02}", micros / 60_000_000, micros / 1_000_000 % 60);
                let improvement = previous_micros.saturating_sub(*duration_micros);
                let delta = if improvement > 0 && improvement < 1_000_000 {
                    "under 1 s faster".into()
                } else {
                    format!("−{}", elapsed(improvement))
                };
                return (
                    format!("New PB: {dungeon} {} ({delta})", elapsed(*duration_micros)),
                    "Personal best observed dungeon clear time".into(),
                );
            }
            Some(AlertDetails::PersonalBest { dps, .. }) => {
                return ("New DPS PB".into(), format!("DPS: {}", dps_text(*dps)));
            }
            _ => {}
        }
    }
    if let Some(AlertDetails::Chat {
        channel,
        sender,
        excerpt,
    }) = &alert.details
    {
        let title = match channel {
            ChatChannel::Whisper => format!("Whisper from {sender}"),
            ChatChannel::Guild => format!("Guild: {sender}"),
            _ => format!("Party: {sender}"),
        };
        return (title, excerpt.clone());
    }
    if let (AlertKind::KeyPopped, Some(AlertDetails::KeyPopped { dungeon })) =
        (&alert.kind, &alert.details)
    {
        return (
            format!("{dungeon} key popped"),
            "Dungeon portal opened".into(),
        );
    }
    let (title, body) = text(&alert.kind);
    (title.into(), body.into())
}

/// Pure content plus best-effort artwork; callable without invoking WinRT.
#[cfg(any(windows, test))]
fn presentation(
    alert: &AlertView,
    artwork: impl FnOnce(&str) -> Option<std::path::PathBuf>,
) -> (String, String, Option<(std::path::PathBuf, String)>) {
    let (title, body) = content(alert);
    let icon = match (&alert.kind, &alert.details) {
        (AlertKind::KeyPopped, Some(AlertDetails::KeyPopped { dungeon })) => {
            artwork(dungeon).map(|path| (path, format!("{dungeon} portal")))
        }
        _ => None,
    };
    (title, body, icon)
}

/// DPS as a grouped whole number, matching the UI's `formatDps` (#26).
fn dps_text(dps: f64) -> String {
    if !dps.is_finite() || dps < 0. {
        return "—".into();
    }
    if dps == 0. {
        return "0".into();
    }
    let digits = format!("{:.0}", dps.round());
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

pub(super) fn text(kind: &AlertKind) -> (&'static str, &'static str) {
    match kind {
        AlertKind::WhiteBag => ("White bag", "White bag observed"),
        AlertKind::KeyPopped => ("Key Popped", "Dungeon key popped"),
        AlertKind::PersonalBest => (
            "Personal best",
            "Personal best observed DPS or dungeon clear time",
        ),
        AlertKind::Death => ("Death", "Death observed"),
        AlertKind::Whisper => ("Whisper", "New whisper"),
        AlertKind::GuildMessage => ("Guild message", "New guild message"),
        AlertKind::PartyMessage => ("Party message", "New party message"),
        AlertKind::FlawlessRun => ("Flawless run", "Flawless run observed"),
    }
}

#[cfg(windows)]
pub(super) struct Notifications {
    registered: bool,
    permission: std::sync::atomic::AtomicU8,
}
#[cfg(windows)]
impl Notifications {
    const APP_ID: &'static str = "com.maxed.companion";

    pub fn new() -> Self {
        let value = Self {
            registered: Self::register().is_ok(),
            permission: std::sync::atomic::AtomicU8::new(2),
        };
        value.refresh_permission();
        value
    }

    fn register() -> std::io::Result<()> {
        use winreg::{enums::HKEY_CURRENT_USER, RegKey};
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(format!(r"Software\Classes\AppUserModelId\{}", Self::APP_ID))?;
        key.set_value("DisplayName", &"Maxed")?;
        key.set_value("ShowInSettings", &1u32)
    }

    pub fn permission(&self) -> NotificationPermission {
        match self.permission.load(std::sync::atomic::Ordering::Acquire) {
            0 => NotificationPermission::Granted,
            1 => NotificationPermission::Denied,
            _ => NotificationPermission::Unavailable,
        }
    }
    pub fn refresh_permission(&self) -> NotificationPermission {
        use windows::UI::Notifications::ToastNotificationManager;
        if !self.registered {
            return NotificationPermission::Unavailable;
        }
        let permission =
            match ToastNotificationManager::CreateToastNotifierWithId(&Self::APP_ID.into()) {
                Ok(notifier) => permission_from_setting(notifier.Setting()),
                Err(_) => NotificationPermission::Unavailable,
            };
        self.permission.store(
            match permission {
                NotificationPermission::Granted => 0,
                NotificationPermission::Denied => 1,
                _ => 2,
            },
            std::sync::atomic::Ordering::Release,
        );
        permission
    }

    pub fn show(
        &self,
        alert: &AlertView,
        cache: Option<&maxed_assets::CachedCatalog>,
    ) -> Result<(), String> {
        if self.refresh_permission() != NotificationPermission::Granted {
            return Err(
                "Windows notifications unavailable or disabled in Windows Settings.".into(),
            );
        }
        let (title, explanation, icon) = presentation(alert, |dungeon| {
            cache.and_then(|cache| maxed_assets::cached_toast_portal_icon(cache, dungeon))
        });
        // Unlike the plugin's detached show(), this returns native API errors.
        // sound(None) emits <audio silent="true"/> in the cached WinRT wrapper.
        let toast = || {
            tauri_winrt_notification::Toast::new(Self::APP_ID)
                .title(&title)
                .text1(&explanation)
                .sound(None)
        };
        if let Some((path, alt)) = &icon {
            if let Some(path) = maxed_assets::local_toast_icon_path(path) {
                if toast()
                    .icon(path, tauri_winrt_notification::IconCrop::Square, alt)
                    .show()
                    .is_ok()
                {
                    return Ok(());
                }
            }
        }
        // Artwork (including a WinRT image failure) must never prevent the text toast.
        toast()
            .show()
            .map_err(|_| "Windows notification could not be delivered.".into())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows::{
        core::{Error, HRESULT},
        UI::Notifications::NotificationSetting,
    };

    #[test]
    fn chat_toasts_show_sender_and_excerpt() {
        for (kind, channel, sender, title) in [
            (
                AlertKind::Whisper,
                ChatChannel::Whisper,
                "SynthAlpha",
                "Whisper from SynthAlpha",
            ),
            (
                AlertKind::GuildMessage,
                ChatChannel::Guild,
                "SynthBravo",
                "Guild: SynthBravo",
            ),
            (
                AlertKind::PartyMessage,
                ChatChannel::Party,
                "SynthCharlie",
                "Party: SynthCharlie",
            ),
        ] {
            let alert = AlertView {
                id: "a".into(),
                at: Timestamp(1),
                kind,
                run_id: None,
                explanation: String::new(),
                details: Some(AlertDetails::Chat {
                    channel,
                    sender: sender.into(),
                    excerpt: "need a carry?".into(),
                }),
                presentation: None,
            };
            assert_eq!(
                content(&alert),
                (title.to_string(), "need a carry?".to_string())
            );
        }
    }

    #[test]
    fn permission_mapping() {
        assert_eq!(
            permission_from_setting(Ok(NotificationSetting::Enabled)),
            NotificationPermission::Granted
        );
        for setting in [
            NotificationSetting::DisabledForApplication,
            NotificationSetting::DisabledForUser,
            NotificationSetting::DisabledByGroupPolicy,
            NotificationSetting::DisabledByManifest,
        ] {
            assert_eq!(
                permission_from_setting(Ok(setting)),
                NotificationPermission::Denied
            );
        }
        assert_eq!(
            permission_from_setting(Err(Error::from_hresult(HRESULT(0x80070490u32 as i32)))),
            NotificationPermission::Granted
        );
        assert_eq!(
            permission_from_setting(Err(Error::from_hresult(HRESULT(0x80070005u32 as i32)))),
            NotificationPermission::Unavailable
        );
    }
}

#[cfg(not(windows))]
pub(super) struct Notifications;
#[cfg(not(windows))]
impl Notifications {
    pub fn new() -> Self {
        Self
    }
    pub fn permission(&self) -> NotificationPermission {
        NotificationPermission::Unavailable
    }
    pub fn refresh_permission(&self) -> NotificationPermission {
        self.permission()
    }
    pub fn show(&self, _: &AlertView) -> Result<(), String> {
        Err("Windows notifications are unavailable on this platform.".into())
    }
}

#[cfg(test)]
mod pb_tests {
    use super::*;
    #[test]
    fn review1_native_subsecond_and_test_copy() {
        let mut alert = AlertView {
            id: "synthetic-pb".into(),
            at: Timestamp(1),
            kind: AlertKind::PersonalBest,
            run_id: Some("synthetic-run".into()),
            explanation: String::new(),
            presentation: Some(AlertLoudness::Banner),
            details: None,
        };
        alert.kind = AlertKind::PersonalBest;
        alert.details = Some(AlertDetails::RunTimePersonalBest {
            dungeon: "Lair of Shaitan".into(),
            duration_micros: 86_500_000,
            previous_micros: 87_000_000,
        });
        assert_eq!(
            content(&alert).0,
            "New PB: Lair of Shaitan 1:26 (under 1 s faster)"
        );
        assert_eq!(
            text(&AlertKind::PersonalBest).1,
            "Personal best observed DPS or dungeon clear time"
        );
    }
    #[test]
    fn native_pb_wording_distinguishes_time_and_damage() {
        let mut alert = AlertView {
            id: "synthetic-pb".into(),
            at: Timestamp(1),
            kind: AlertKind::PersonalBest,
            run_id: Some("synthetic-run".into()),
            explanation: String::new(),
            presentation: Some(AlertLoudness::Banner),
            details: Some(AlertDetails::RunTimePersonalBest {
                dungeon: "Lair of Shaitan".into(),
                duration_micros: 86_000_000,
                previous_micros: 87_000_000,
            }),
        };
        assert_eq!(content(&alert).0, "New PB: Lair of Shaitan 1:26 (−0:01)");
        alert.details = Some(AlertDetails::PersonalBest {
            fight_id: "fight".into(),
            character_id: "self".into(),
            boss_type: 300,
            dps: 1172.4,
            previous_dps: 1000.,
        });
        assert_eq!(content(&alert).0, "New DPS PB");
        assert_eq!(content(&alert).1, "DPS: 1,172");
    }

    #[test]
    fn dps_text_normalizes_negative_zero_without_accepting_negative_damage() {
        assert_eq!(dps_text(-0.0), "0");
        assert_eq!(dps_text(-0.0001), "—");
    }

    #[test]
    fn dps_text_is_a_grouped_whole_number() {
        assert_eq!(dps_text(1448.7), "1,449");
        assert_eq!(dps_text(1609.), "1,609");
        assert_eq!(dps_text(999.5), "1,000");
        assert_eq!(dps_text(42.), "42");
        assert_eq!(dps_text(1_234_567.4), "1,234,567");
        assert_eq!(dps_text(0.), "0");
        assert_eq!(dps_text(f64::NAN), "—");
        assert_eq!(dps_text(f64::INFINITY), "—");
        assert_eq!(dps_text(-1.), "—");
    }
}

#[cfg(test)]
mod icon_tests {
    use super::*;

    #[test]
    fn missing_icon_keeps_key_pop_text_available() {
        let alert = AlertView {
            id: "synthetic-key".into(),
            at: Timestamp(1),
            kind: AlertKind::KeyPopped,
            run_id: None,
            explanation: String::new(),
            presentation: None,
            details: Some(AlertDetails::KeyPopped {
                dungeon: "Unknown dungeon".into(),
            }),
        };
        let (title, body, icon) = presentation(&alert, |_| None);
        assert_eq!(title, "Unknown dungeon key popped");
        assert_eq!(body, "Dungeon portal opened");
        assert!(icon.is_none());
        let catalog = maxed_types::demo::demo_catalog();
        let dir = tempfile::tempdir().unwrap();
        let (title, _, icon) = presentation(&alert, |dungeon| {
            maxed_assets::toast_portal_icon(&catalog, dir.path(), dungeon)
        });
        assert_eq!(title, "Unknown dungeon key popped");
        assert!(icon.is_none());
    }

    #[test]
    fn portal_selection_preserves_text_for_missing_portals_and_invalid_rectangles() {
        use base64::Engine;
        use sha2::{Digest, Sha256};
        // Authored 1x1 RGBA sheet: a half-transparent red pixel, no game artwork.
        let png = base64::engine::general_purpose::STANDARD.decode(
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DQAAAEgQGALFXOsAAAAABJRU5ErkJggg=="
        ).unwrap();
        for case in 0..5 {
            let dir = tempfile::tempdir().unwrap();
            let mut catalog = maxed_types::demo::demo_catalog();
            catalog.client_fingerprint = "a".repeat(64);
            catalog.dungeons[0].name = "Synthetic dungeon".into();
            catalog.dungeons[0].portal_type = Some(4242);
            catalog.objects[0].object_type = 4242;
            catalog.objects[0].sprite_key = Some("synthetic".into());
            catalog.sprites = vec![SpriteRef {
                key: "synthetic".into(),
                sheet: "mapObjects.png".into(),
                x: 0,
                y: 0,
                width: 1,
                height: 1,
                trim: None,
            }];
            let root = dir.path().join(&catalog.client_fingerprint);
            std::fs::create_dir(&root).unwrap();
            let mut pack = b"MXTILE01".to_vec();
            pack.extend(catalog.client_fingerprint.as_bytes());
            pack.extend(1u32.to_le_bytes());
            for word in [3u32, 0, 0, 1, 1] {
                pack.extend(word.to_le_bytes());
            }
            pack.extend(140u64.to_le_bytes());
            pack.extend((png.len() as u32).to_le_bytes());
            pack.extend(Sha256::digest(&png));
            pack.extend(&png);
            std::fs::write(root.join("tiles.pack"), &pack).unwrap();
            std::fs::write(
                root.join("manifest.json"),
                serde_json::to_vec(&serde_json::json!({
                    "fingerprint": catalog.client_fingerprint,
                    "files": {"tiles.pack": {"bytes": pack.len()}}
                }))
                .unwrap(),
            )
            .unwrap();
            let mut alert = AlertView {
                id: "synthetic-key".into(),
                at: Timestamp(1),
                kind: AlertKind::KeyPopped,
                run_id: None,
                explanation: String::new(),
                presentation: None,
                details: Some(AlertDetails::KeyPopped {
                    dungeon: "Synthetic dungeon".into(),
                }),
            };
            match case {
                1 => catalog.dungeons[0].portal_type = None,
                2 => catalog.sprites[0].x = 1,
                3 => catalog.dungeons.clear(),
                4 => alert.kind = AlertKind::WhiteBag,
                _ => {}
            }
            let (title, body, icon) = presentation(&alert, |dungeon| {
                maxed_assets::toast_portal_icon(&catalog, dir.path(), dungeon)
            });
            if case == 4 {
                assert_eq!(
                    (title.as_str(), body.as_str()),
                    ("White bag", "White bag observed")
                );
            } else {
                assert_eq!(
                    (title.as_str(), body.as_str()),
                    ("Synthetic dungeon key popped", "Dungeon portal opened")
                );
            }
            if case == 0 {
                let (path, alt) = icon.unwrap();
                assert!(path.is_file());
                assert_eq!(alt, "Synthetic dungeon portal");
            } else {
                assert!(icon.is_none(), "case {case}");
            }
        }
    }
}
