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
}
