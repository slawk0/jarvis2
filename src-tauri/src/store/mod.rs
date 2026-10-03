//! Local persistence: atomic JSON files in the app config directory.

pub mod profile_data;
pub mod profiles;
pub mod secrets;
pub mod settings;

use std::path::{Path, PathBuf};

use serde::{de::DeserializeOwned, Serialize};

use crate::error::{AppError, AppResult, ErrorCode};

/// Locations of everything Jarvis keeps on the local machine.
#[derive(Debug, Clone)]
pub struct Paths {
    root: PathBuf,
}

impl Paths {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn profiles_file(&self) -> PathBuf {
        self.root.join("profiles.json")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn known_hosts_file(&self) -> PathBuf {
        self.root.join("known_hosts")
    }

    pub fn profile_dir(&self, profile_id: &str) -> PathBuf {
        self.root.join("profiles").join(profile_id)
    }

    pub fn global_dir(&self) -> PathBuf {
        self.root.join("global")
    }
}

/// Read a JSON file. A missing file yields the default value. A file that
/// cannot be parsed is moved aside (never overwritten) and reported.
pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> AppResult<T> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(e) => return Err(e.into()),
    };
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(T::default());
    }
    match serde_json::from_slice(&bytes) {
        Ok(value) => Ok(value),
        Err(parse_error) => {
            let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
            let mut backup = path.as_os_str().to_owned();
            backup.push(format!(".corrupt-{stamp}"));
            let backup = PathBuf::from(backup);
            std::fs::rename(path, &backup)?;
            Err(AppError::new(
                ErrorCode::StoreCorrupt,
                format!(
                    "{} could not be read ({parse_error}); it was kept as {}",
                    path.display(),
                    backup.display()
                ),
            ))
        }
    }
}

/// Like [`load_json`], but fields missing from the file take their value from
/// `T::default()`. This keeps old files readable after new fields are added
/// without scattering `#[serde(default)]` over types shared with the frontend.
pub fn load_json_merged<T>(path: &Path) -> AppResult<T>
where
    T: DeserializeOwned + Default + Serialize,
{
    let stored: serde_json::Value = load_json(path)?;
    let mut merged = serde_json::to_value(T::default())?;
    merge(&mut merged, stored);
    match serde_json::from_value(merged) {
        Ok(value) => Ok(value),
        // Wrong types in an otherwise valid file: fall back to defaults.
        Err(_) => Ok(T::default()),
    }
}

fn merge(base: &mut serde_json::Value, overlay: serde_json::Value) {
    use serde_json::Value;
    match (base, overlay) {
        (_, Value::Null) => {}
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(&key) {
                    Some(slot) if slot.is_object() && value.is_object() => merge(slot, value),
                    Some(slot) => *slot = value,
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

/// Write a JSON file atomically: temp file in the same directory, fsync, rename.
pub fn save_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> AppResult<()> {
    use std::io::Write;

    let dir = path
        .parent()
        .ok_or_else(|| AppError::internal("store path has no parent"))?;
    std::fs::create_dir_all(dir)?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".tmp-{}", uuid::Uuid::new_v4().simple()));
    let tmp = PathBuf::from(tmp);
    let bytes = serde_json::to_vec_pretty(value)?;
    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    Ok(result?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_default() {
        let dir = tempfile::tempdir().unwrap();
        let v: Vec<String> = load_json(&dir.path().join("nope.json")).unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn round_trip_and_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("a.json");
        save_json(&path, &vec!["x".to_string()]).unwrap();
        save_json(&path, &vec!["y".to_string(), "z".to_string()]).unwrap();
        let v: Vec<String> = load_json(&path).unwrap();
        assert_eq!(v, vec!["y", "z"]);
        // No temp files left behind.
        let count = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(count, 1);
    }

    #[test]
    fn corrupt_file_is_backed_up_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        std::fs::write(&path, b"{ not json").unwrap();
        let err = load_json::<Vec<String>>(&path).unwrap_err();
        assert_eq!(err.code, ErrorCode::StoreCorrupt);
        assert!(!path.exists());
        let backups: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(backups.len(), 1);
        assert!(backups[0].starts_with("a.json.corrupt-"));
        assert_eq!(
            std::fs::read(dir.path().join(&backups[0])).unwrap(),
            b"{ not json"
        );
    }
}
