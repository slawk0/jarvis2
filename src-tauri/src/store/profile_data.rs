//! Per-profile documents: everything a feature remembers about one server.
//!
//! Each [`DataKey`] is one JSON document under `profiles/<id>/`. Documents the
//! backend acts on (backup templates, restic repositories, …) are read back
//! through [`ProfileData::get_as`] into typed structs; documents only the UI
//! cares about are stored as-is and typed on the TypeScript side.

use std::path::PathBuf;

use parking_lot::Mutex;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

use super::{load_json, save_json, Paths};
use crate::error::AppResult;
use crate::shell::validate;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DataKey {
    Runbooks,
    SavedCommands,
    SftpBookmarks,
    BackupTemplates,
    ResticRepos,
    AlertThresholds,
    DbConnections,
    NginxTargets,
    LogAnalysisProfiles,
    LogSources,
    CrowdsecConfig,
    ComposeStacks,
    WorkspaceLayout,
}

impl DataKey {
    fn file_name(self) -> &'static str {
        match self {
            DataKey::Runbooks => "runbooks.json",
            DataKey::SavedCommands => "saved-commands.json",
            DataKey::SftpBookmarks => "sftp-bookmarks.json",
            DataKey::BackupTemplates => "backup-templates.json",
            DataKey::ResticRepos => "restic-repos.json",
            DataKey::AlertThresholds => "alert-thresholds.json",
            DataKey::DbConnections => "db-connections.json",
            DataKey::NginxTargets => "nginx-targets.json",
            DataKey::LogAnalysisProfiles => "log-analysis-profiles.json",
            DataKey::LogSources => "log-sources.json",
            DataKey::CrowdsecConfig => "crowdsec.json",
            DataKey::ComposeStacks => "compose-stacks.json",
            DataKey::WorkspaceLayout => "workspace-layout.json",
        }
    }
}

pub struct ProfileData {
    paths: Paths,
    lock: Mutex<()>,
}

impl ProfileData {
    pub fn new(paths: Paths) -> Self {
        Self { paths, lock: Mutex::new(()) }
    }

    fn file(&self, profile_id: &str, key: DataKey) -> AppResult<PathBuf> {
        validate::slug("profile id", profile_id)?;
        Ok(self.paths.profile_dir(profile_id).join(key.file_name()))
    }

    /// `Value::Null` when nothing has been stored yet.
    pub fn get(&self, profile_id: &str, key: DataKey) -> AppResult<Value> {
        let _guard = self.lock.lock();
        load_json::<Value>(&self.file(profile_id, key)?)
    }

    pub fn get_as<T: DeserializeOwned + Default>(&self, profile_id: &str, key: DataKey) -> AppResult<T> {
        let _guard = self.lock.lock();
        load_json::<T>(&self.file(profile_id, key)?)
    }

    pub fn set<T: Serialize + ?Sized>(&self, profile_id: &str, key: DataKey, value: &T) -> AppResult<()> {
        let _guard = self.lock.lock();
        save_json(&self.file(profile_id, key)?, value)
    }

    /// Read-modify-write under the store lock.
    pub fn update<T, R>(&self, profile_id: &str, key: DataKey, edit: impl FnOnce(&mut T) -> R) -> AppResult<R>
    where
        T: DeserializeOwned + Default + Serialize,
    {
        let _guard = self.lock.lock();
        let file = self.file(profile_id, key)?;
        let mut value = load_json::<T>(&file)?;
        let result = edit(&mut value);
        save_json(&file, &value)?;
        Ok(result)
    }

    /// Remove everything stored for a profile.
    pub fn delete_profile(&self, profile_id: &str) -> AppResult<()> {
        validate::slug("profile id", profile_id)?;
        let _guard = self.lock.lock();
        match std::fs::remove_dir_all(self.paths.profile_dir(profile_id)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn data() -> (tempfile::TempDir, ProfileData) {
        let dir = tempfile::tempdir().unwrap();
        let d = ProfileData::new(Paths::new(dir.path().to_path_buf()));
        (dir, d)
    }

    #[test]
    fn unset_is_null_and_values_round_trip() {
        let (_dir, d) = data();
        assert_eq!(d.get("p1", DataKey::Runbooks).unwrap(), Value::Null);
        d.set("p1", DataKey::Runbooks, &json!([{"name": "x"}])).unwrap();
        assert_eq!(d.get("p1", DataKey::Runbooks).unwrap(), json!([{"name": "x"}]));
        assert_eq!(d.get("p2", DataKey::Runbooks).unwrap(), Value::Null);
        assert_eq!(d.get("p1", DataKey::SavedCommands).unwrap(), Value::Null);
    }

    #[test]
    fn update_and_typed_read() {
        let (_dir, d) = data();
        d.update("p1", DataKey::ComposeStacks, |v: &mut Vec<String>| v.push("/srv/a".into())).unwrap();
        d.update("p1", DataKey::ComposeStacks, |v: &mut Vec<String>| v.push("/srv/b".into())).unwrap();
        let v: Vec<String> = d.get_as("p1", DataKey::ComposeStacks).unwrap();
        assert_eq!(v, vec!["/srv/a", "/srv/b"]);
    }

    #[test]
    fn delete_profile_removes_all_documents() {
        let (_dir, d) = data();
        d.set("p1", DataKey::Runbooks, &json!([1])).unwrap();
        d.set("p1", DataKey::SftpBookmarks, &json!([2])).unwrap();
        d.set("p2", DataKey::Runbooks, &json!([3])).unwrap();
        d.delete_profile("p1").unwrap();
        d.delete_profile("p1").unwrap();
        assert_eq!(d.get("p1", DataKey::Runbooks).unwrap(), Value::Null);
        assert_eq!(d.get("p2", DataKey::Runbooks).unwrap(), json!([3]));
    }

    #[test]
    fn rejects_path_traversal_in_profile_id() {
        let (_dir, d) = data();
        assert!(d.get("../x", DataKey::Runbooks).is_err());
        assert!(d.delete_profile("..").is_err());
    }
}
