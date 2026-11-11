//! Serialized, fail-closed local settings. No command exposes filesystem errors.
use maxed_types::{AppSettings, ChaseItem, MAX_CHASE_ITEMS};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

pub const SETTINGS_ERROR: &str =
    "Local settings could not be read or saved. Restore a supported settings file and retry.";
pub const SETTINGS_INVALID: &str = "Settings are invalid or unsupported.";
const LIMIT: u64 = 64 * 1024;

pub fn validate(settings: &AppSettings) -> Result<(), String> {
    if !settings.alerts.valid()
        || !settings.chat.valid()
        || settings.version != 1
        || !(1..=1_000_000_000).contains(&settings.recording_limit_bytes)
        || settings
            .adapter_id
            .as_ref()
            .is_some_and(|v| v.is_empty() || v.len() > 1024 || v.contains('\0'))
        || settings
            .assets_path
            .as_ref()
            .is_some_and(|v| v.len() > 32768 || v.contains('\0') || !Path::new(v).is_absolute())
    {
        return Err(SETTINGS_INVALID.into());
    }
    let mut seen = BTreeSet::new();
    if settings.loot.chase.len() > MAX_CHASE_ITEMS
        || settings.loot.chase.iter().any(|item| {
            item.dungeon.as_ref().is_some_and(|name| {
                name.is_empty() || name.len() > 256 || name.chars().any(char::is_control)
            }) || item.pinned_at.0 > i64::MAX as u64
                || !seen.insert((item.item_type, item.dungeon.as_deref()))
        })
    {
        return Err(SETTINGS_ERROR.into());
    }
    if settings.party.presets.len() > 32 {
        return Err("At most 32 presets".into());
    }
    let mut preset_ids = BTreeSet::new();
    let mut preset_names = BTreeSet::new();
    let mut auto_apply_dungeons = BTreeSet::new();
    for preset in &settings.party.presets {
        if !(1..=40).contains(&preset.name.chars().count()) {
            return Err("Preset names must be 1 to 40 characters".into());
        }
        if !preset_names.insert(preset.name.to_lowercase()) {
            return Err("Preset names must be unique".into());
        }
        if !preset_ids.insert(preset.id.as_str()) {
            return Err("Preset ids must be unique".into());
        }
        if preset.min_maxed.is_some_and(|value| value > 8) {
            return Err("Maxed stats must be 0 to 8".into());
        }
        let preset_dungeons: BTreeSet<_> = preset.auto_apply.iter().collect();
        for dungeon in preset_dungeons {
            if !auto_apply_dungeons.insert(dungeon) {
                return Err(format!("{dungeon} is set to auto-apply in two presets"));
            }
        }
    }
    if settings
        .party
        .default_preset_id
        .as_deref()
        .is_some_and(|id| !preset_ids.contains(id))
    {
        return Err("Default preset not found".into());
    }
    Ok(())
}

pub struct SettingsStore {
    root: PathBuf,
    serial: Mutex<()>,
}
impl SettingsStore {
    /// The caller must supply the resolved Tauri app-data directory.
    pub fn new(app_data: PathBuf) -> Result<Self, String> {
        if !app_data.is_absolute() {
            return Err(SETTINGS_ERROR.into());
        }
        Ok(Self {
            root: app_data,
            serial: Mutex::new(()),
        })
    }
    pub fn get(&self) -> Result<AppSettings, String> {
        let _guard = self.serial.lock().map_err(|_| SETTINGS_ERROR)?;
        self.read()
    }
    fn read(&self) -> Result<AppSettings, String> {
        let file = match File::open(self.root.join("settings.json")) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(AppSettings::default())
            }
            Err(_) => return Err(SETTINGS_ERROR.into()),
        };
        let mut bytes = Vec::new();
        file.take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| SETTINGS_ERROR)?;
        if bytes.len() as u64 > LIMIT {
            return Err(SETTINGS_ERROR.into());
        }
        let value = serde_json::from_slice(&bytes).map_err(|_| SETTINGS_ERROR)?;
        validate(&value).map_err(|_| SETTINGS_ERROR)?;
        Ok(value)
    }
    pub fn put(&self, value: &AppSettings) -> Result<(), String> {
        validate(value)?;
        self.patch(|current| {
            let mut next = value.clone();
            next.alerts = current.alerts;
            next.loot = current.loot;
            next.chat = current.chat;
            next.party = current.party;
            next.recap_image = current.recap_image;
            next
        })
    }
    pub fn put_chat(&self, chat: maxed_types::ChatSettings) -> Result<(), String> {
        if !chat.valid() {
            return Err(SETTINGS_INVALID.into());
        }
        self.patch(|mut current| {
            current.chat = chat;
            current
        })
    }
    pub fn put_party(&self, party: maxed_types::PartySettings) -> Result<(), String> {
        self.patch(|mut current| {
            current.party = party;
            current
        })
    }
    pub fn put_recap_image(
        &self,
        recap_image: maxed_types::RecapImageSettings,
    ) -> Result<(), String> {
        self.patch(|mut current| {
            current.recap_image = recap_image;
            current
        })
    }
    pub fn put_chase(&self, items: Vec<ChaseItem>) -> Result<(), String> {
        self.patch(|mut current| {
            current.loot.chase = items;
            current
        })
    }
    pub fn update_alert_preferences(
        &self,
        preferences: maxed_types::AlertPreferences,
    ) -> Result<(), String> {
        if !preferences.valid() {
            return Err(SETTINGS_INVALID.into());
        }
        self.patch(|mut current| {
            current.alerts = preferences;
            current
        })
    }
    fn patch(&self, patch: impl FnOnce(AppSettings) -> AppSettings) -> Result<(), String> {
        let _guard = self.serial.lock().map_err(|_| SETTINGS_ERROR)?;
        fs::create_dir_all(&self.root).map_err(|_| SETTINGS_ERROR)?;
        let _lease = writer_lease(&self.root.join("settings.lock"))?;
        let value = patch(self.read()?);
        validate(&value)?;
        let mut bytes = serde_json::to_vec(&value).map_err(|_| SETTINGS_ERROR)?;
        bytes.push(b'\n');
        if bytes.len() as u64 > LIMIT {
            return Err(SETTINGS_INVALID.into());
        }
        let staging = self.root.join("settings.pending");
        match fs::remove_file(&staging) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(SETTINGS_ERROR.into()),
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)
            .map_err(|_| SETTINGS_ERROR)?;
        let result = (|| {
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| SETTINGS_ERROR)?;
            drop(file);
            // Windows std::fs::rename uses replacement without removing the old
            // destination first. Failure leaves the previous document intact.
            fs::rename(&staging, self.root.join("settings.json")).map_err(|_| SETTINGS_ERROR)
        })();
        if result.is_err() {
            let _ = fs::remove_file(staging);
        }
        result.map_err(str::to_owned)
    }
}

fn writer_lease(path: &Path) -> Result<File, String> {
    #[cfg(windows)]
    {
        use std::{
            os::windows::fs::OpenOptionsExt,
            time::{Duration, Instant},
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .share_mode(0)
                .open(path)
            {
                Ok(file) => return Ok(file),
                Err(error)
                    if matches!(error.raw_os_error(), Some(32 | 33))
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => return Err(SETTINGS_ERROR.into()),
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("Settings ownership requires Windows.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maxed_types::{PartySettings, RequirementPreset, SlotTiers};

    #[test]
    fn recap_image_defaults_true_and_round_trips() {
        let mut json = serde_json::to_value(AppSettings::default()).unwrap();
        json.as_object_mut().unwrap().remove("recap_image");
        let defaults: AppSettings = serde_json::from_value(json.clone()).unwrap();
        assert!(defaults.recap_image.names);
        assert!(defaults.recap_image.loot);
        assert!(defaults.recap_image.fights);
        json["recap_image"] = serde_json::json!({});
        assert_eq!(
            serde_json::from_value::<AppSettings>(json.clone()).unwrap(),
            defaults
        );
        let root = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(root.path().into()).unwrap();
        for names in [false, true] {
            for loot in [false, true] {
                for fights in [false, true] {
                    let settings = AppSettings {
                        recap_image: maxed_types::RecapImageSettings {
                            names,
                            loot,
                            fights,
                        },
                        ..defaults.clone()
                    };
                    store.put_recap_image(settings.recap_image.clone()).unwrap();
                    assert_eq!(store.get().unwrap(), settings);
                }
            }
        }
        json["recap_image"] =
            serde_json::json!({"names": true, "loot": true, "fights": true, "unknown": true});
        assert!(serde_json::from_value::<AppSettings>(json.clone()).is_err());
        json["recap_image"] = serde_json::json!({"names": "true"});
        assert!(serde_json::from_value::<AppSettings>(json).is_err());
    }

    fn party_preset(id: &str) -> RequirementPreset {
        RequirementPreset {
            id: id.into(),
            name: format!("Preset {id}"),
            min_maxed: Some(6),
            min_tier: SlotTiers {
                weapon: Some(11),
                ability: Some(5),
                armor: Some(12),
                ring: Some(4),
            },
            ut_st_meets: true,
            auto_apply: vec![format!("Synthetic dungeon {id}")],
        }
    }

    #[test]
    fn party_presets_round_trip() {
        let mut settings = AppSettings::default();
        let mut second = party_preset("second");
        second.min_maxed = Some(8);
        second.ut_st_meets = false;
        second.auto_apply.push("Synthetic extra dungeon".into());
        settings.party = PartySettings {
            presets: vec![party_preset("first"), second],
            default_preset_id: Some("second".into()),
        };
        let json = serde_json::to_vec(&settings).unwrap();
        let restored: AppSettings = serde_json::from_slice(&json).unwrap();
        assert_eq!(restored, settings);
        assert_eq!(validate(&restored), Ok(()));
    }

    #[test]
    fn settings_without_party_load_with_no_presets() {
        let mut json = serde_json::to_value(AppSettings::default()).unwrap();
        json.as_object_mut().unwrap().remove("party");
        let restored: AppSettings = serde_json::from_value(json).unwrap();
        assert_eq!(restored.party, PartySettings::default());
        assert_eq!(validate(&restored), Ok(()));
    }

    #[test]
    fn settings_with_empty_party_load_with_no_presets() {
        let mut json = serde_json::to_value(AppSettings::default()).unwrap();
        json["party"] = serde_json::json!({});
        let restored: AppSettings = serde_json::from_value(json).unwrap();
        assert_eq!(restored.party, PartySettings::default());
        validate(&restored).unwrap();
    }

    #[test]
    fn party_preset_count_limit() {
        let mut settings = AppSettings::default();
        settings.party.presets = (0..32).map(|id| party_preset(&id.to_string())).collect();
        assert_eq!(validate(&settings), Ok(()));
        settings.party.presets.push(party_preset("extra"));
        assert_eq!(validate(&settings).unwrap_err(), "At most 32 presets");
    }

    #[test]
    fn party_preset_name_length_counts_characters_without_trimming() {
        let mut settings = AppSettings::default();
        settings.party.presets.push(party_preset("first"));
        for name in [String::new(), "a".repeat(41), "é".repeat(41)] {
            settings.party.presets[0].name = name;
            assert_eq!(
                validate(&settings).unwrap_err(),
                "Preset names must be 1 to 40 characters"
            );
        }
        for name in [" ".into(), "a".repeat(40), "é".repeat(40)] {
            settings.party.presets[0].name = name;
            assert_eq!(validate(&settings), Ok(()));
        }
    }

    #[test]
    fn party_preset_names_are_unique_ignoring_case() {
        let mut settings = AppSettings::default();
        settings.party.presets = vec![party_preset("first"), party_preset("second")];
        for (first, second) in [("Raid", "Raid"), ("Raid", "rAID"), ("Élite", "élite")] {
            settings.party.presets[0].name = first.into();
            settings.party.presets[1].name = second.into();
            assert_eq!(
                validate(&settings).unwrap_err(),
                "Preset names must be unique"
            );
        }
        settings.party.presets[1].name = " Élite".into();
        assert_eq!(validate(&settings), Ok(()));
    }

    #[test]
    fn party_preset_dungeon_cannot_auto_apply_in_two_presets() {
        let mut settings = AppSettings::default();
        settings.party.presets = vec![party_preset("first"), party_preset("second")];
        settings.party.presets[1]
            .auto_apply
            .push("Synthetic dungeon first".into());
        assert_eq!(
            validate(&settings).unwrap_err(),
            "Synthetic dungeon first is set to auto-apply in two presets"
        );
    }

    #[test]
    fn party_preset_repeated_dungeon_in_one_preset_is_valid() {
        let mut settings = AppSettings::default();
        settings.party.presets.push(party_preset("first"));
        settings.party.presets[0]
            .auto_apply
            .push("Synthetic dungeon first".into());
        assert_eq!(validate(&settings), Ok(()));
    }

    #[test]
    fn party_default_preset_must_exist() {
        let mut settings = AppSettings::default();
        settings.party.default_preset_id = Some("missing".into());
        assert_eq!(validate(&settings).unwrap_err(), "Default preset not found");
        settings.party.presets.push(party_preset("first"));
        assert_eq!(validate(&settings).unwrap_err(), "Default preset not found");
        settings.party.default_preset_id = Some("first".into());
        assert_eq!(validate(&settings), Ok(()));
    }

    #[test]
    fn party_preset_maxed_stats_range() {
        let mut settings = AppSettings::default();
        settings.party.presets.push(party_preset("first"));
        for min_maxed in [Some(9), Some(u8::MAX)] {
            settings.party.presets[0].min_maxed = min_maxed;
            assert_eq!(
                validate(&settings).unwrap_err(),
                "Maxed stats must be 0 to 8"
            );
        }
        for min_maxed in [None, Some(0), Some(8)] {
            settings.party.presets[0].min_maxed = min_maxed;
            assert_eq!(validate(&settings), Ok(()));
        }
    }

    #[test]
    fn party_preset_ids_must_be_unique() {
        let mut settings = AppSettings::default();
        settings.party.presets = vec![party_preset("first"), party_preset("second")];
        settings.party.presets[1].id = "first".into();
        assert_eq!(
            validate(&settings).unwrap_err(),
            "Preset ids must be unique"
        );
    }

    #[test]
    fn validate_rejects_invalid_chat_settings() {
        let mut settings = AppSettings::default();
        settings.chat.hidden_keywords = vec![String::new()];
        assert_eq!(validate(&settings).unwrap_err(), SETTINGS_INVALID);
    }

    #[test]
    fn chat_updates_preserve_other_settings_and_survive_stale_general_saves() {
        let root = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(root.path().into()).unwrap();
        let mut stale = store.get().unwrap();
        let chat = maxed_types::ChatSettings {
            retention_days: 30,
            ..Default::default()
        };
        store.put_chat(chat.clone()).unwrap();
        stale.recording_limit_bytes = 1234;
        store.put(&stale).unwrap();
        let saved = store.get().unwrap();
        assert_eq!(saved.chat, chat);
        assert_eq!(saved.recording_limit_bytes, 1234);
        let before = fs::read(root.path().join("settings.json")).unwrap();
        assert_eq!(
            store
                .put_chat(maxed_types::ChatSettings {
                    retention_days: 0,
                    ..Default::default()
                })
                .unwrap_err(),
            SETTINGS_INVALID
        );
        assert_eq!(fs::read(root.path().join("settings.json")).unwrap(), before);
    }

    #[test]
    fn party_updates_preserve_other_settings_and_survive_stale_general_saves() {
        let root = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(root.path().into()).unwrap();
        let mut stale = store.get().unwrap();
        stale.recording_limit_bytes = 4321;
        store.put(&stale).unwrap();
        let party = PartySettings {
            presets: vec![party_preset("first")],
            default_preset_id: Some("first".into()),
        };
        store.put_party(party.clone()).unwrap();
        assert_eq!(
            store.get().unwrap(),
            AppSettings {
                party: party.clone(),
                ..stale.clone()
            }
        );
        stale.recording_limit_bytes = 1234;
        store.put(&stale).unwrap();
        assert_eq!(store.get().unwrap(), AppSettings { party, ..stale });
        let before = fs::read(root.path().join("settings.json")).unwrap();
        assert_eq!(
            store
                .put_party(PartySettings {
                    default_preset_id: Some("missing".into()),
                    ..Default::default()
                })
                .unwrap_err(),
            "Default preset not found"
        );
        assert_eq!(fs::read(root.path().join("settings.json")).unwrap(), before);
    }
}
