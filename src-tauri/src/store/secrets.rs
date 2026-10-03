//! Secrets live in the OS keyring, never in JSON files.
//!
//! The keyring cannot be enumerated portably, so each owner (a profile, or
//! the global scope) keeps a plain index of its secret *names* next to its
//! data. That index is what makes "delete a profile and all of its secrets"
//! possible.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use parking_lot::Mutex;

use super::{load_json, save_json, Paths};
use crate::error::{AppError, AppResult, ErrorCode};

const SERVICE: &str = "com.jarvis.servermanager";
/// Keyring service used by Jarvis v1; read once to migrate profile secrets.
const LEGACY_SERVICE: &str = "JarvisServerManager";
/// Owner id for secrets that do not belong to a server profile.
pub const GLOBAL: &str = "_global";

enum Backend {
    Os,
    #[allow(dead_code)]
    Memory(Mutex<HashMap<String, String>>),
}

pub struct Secrets {
    paths: Paths,
    backend: Backend,
    lock: Mutex<()>,
}

fn keyring_err(e: keyring::Error) -> AppError {
    AppError::new(ErrorCode::Keyring, e.to_string())
}

impl Secrets {
    pub fn new(paths: Paths) -> Self {
        Self {
            paths,
            backend: Backend::Os,
            lock: Mutex::new(()),
        }
    }

    #[cfg(test)]
    pub fn in_memory(paths: Paths) -> Self {
        Self {
            paths,
            backend: Backend::Memory(Mutex::new(HashMap::new())),
            lock: Mutex::new(()),
        }
    }

    fn account(owner: &str, name: &str) -> String {
        format!("{owner}/{name}")
    }

    fn index_file(&self, owner: &str) -> PathBuf {
        if owner == GLOBAL {
            self.paths.global_dir().join("secrets-index.json")
        } else {
            self.paths.profile_dir(owner).join("secrets-index.json")
        }
    }

    fn update_index(&self, owner: &str, edit: impl FnOnce(&mut BTreeSet<String>)) -> AppResult<()> {
        let _guard = self.lock.lock();
        let file = self.index_file(owner);
        let mut names: BTreeSet<String> = load_json(&file).unwrap_or_default();
        let before = names.clone();
        edit(&mut names);
        if names == before {
            return Ok(());
        }
        save_json(&file, &names)
    }

    pub fn set(&self, owner: &str, name: &str, value: &str) -> AppResult<()> {
        let account = Self::account(owner, name);
        match &self.backend {
            Backend::Os => keyring::Entry::new(SERVICE, &account)
                .and_then(|e| e.set_password(value))
                .map_err(keyring_err)?,
            Backend::Memory(map) => {
                map.lock().insert(account, value.to_string());
            }
        }
        self.update_index(owner, |names| {
            names.insert(name.to_string());
        })
    }

    pub fn get(&self, owner: &str, name: &str) -> AppResult<Option<String>> {
        let account = Self::account(owner, name);
        match &self.backend {
            Backend::Os => {
                match keyring::Entry::new(SERVICE, &account).and_then(|e| e.get_password()) {
                    Ok(value) => Ok(Some(value)),
                    Err(keyring::Error::NoEntry) => Ok(None),
                    Err(e) => Err(keyring_err(e)),
                }
            }
            Backend::Memory(map) => Ok(map.lock().get(&account).cloned()),
        }
    }

    /// Like [`get`](Self::get) but a missing secret is an empty string.
    pub fn get_or_empty(&self, owner: &str, name: &str) -> AppResult<String> {
        Ok(self.get(owner, name)?.unwrap_or_default())
    }

    pub fn exists(&self, owner: &str, name: &str) -> bool {
        matches!(self.get(owner, name), Ok(Some(_)))
    }

    pub fn delete(&self, owner: &str, name: &str) -> AppResult<()> {
        self.delete_entry(owner, name)?;
        self.update_index(owner, |names| {
            names.remove(name);
        })
    }

    fn delete_entry(&self, owner: &str, name: &str) -> AppResult<()> {
        let account = Self::account(owner, name);
        match &self.backend {
            Backend::Os => {
                match keyring::Entry::new(SERVICE, &account).and_then(|e| e.delete_credential()) {
                    Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                    Err(e) => Err(keyring_err(e)),
                }
            }
            Backend::Memory(map) => {
                map.lock().remove(&account);
                Ok(())
            }
        }
    }

    /// Delete every secret whose name starts with `prefix` (e.g. one backup template).
    pub fn delete_prefixed(&self, owner: &str, prefix: &str) -> AppResult<()> {
        let names: BTreeSet<String> = load_json(&self.index_file(owner)).unwrap_or_default();
        for name in names.iter().filter(|n| n.starts_with(prefix)) {
            self.delete_entry(owner, name)?;
        }
        self.update_index(owner, |all| all.retain(|n| !n.starts_with(prefix)))
    }

    /// Delete everything an owner has stored. Used when a profile is removed.
    pub fn delete_all(&self, owner: &str) -> AppResult<()> {
        self.delete_prefixed(owner, "")
    }

    /// Jarvis v1 stored `<id>_pass` / `<id>_passphrase`; move them over on first use.
    pub fn migrate_legacy_profile(&self, profile_id: &str) {
        if !matches!(self.backend, Backend::Os) {
            return;
        }
        for (legacy, name) in [("pass", "ssh/password"), ("passphrase", "ssh/passphrase")] {
            if self.exists(profile_id, name) {
                continue;
            }
            let account = format!("{profile_id}_{legacy}");
            let Ok(entry) = keyring::Entry::new(LEGACY_SERVICE, &account) else {
                continue;
            };
            if let Ok(value) = entry.get_password() {
                if self.set(profile_id, name, &value).is_ok() {
                    let _ = entry.delete_credential();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secrets() -> (tempfile::TempDir, Secrets) {
        let dir = tempfile::tempdir().unwrap();
        let s = Secrets::in_memory(Paths::new(dir.path().to_path_buf()));
        (dir, s)
    }

    #[test]
    fn set_get_delete() {
        let (_dir, s) = secrets();
        assert_eq!(s.get("p1", "ssh/password").unwrap(), None);
        s.set("p1", "ssh/password", "hunter2").unwrap();
        assert_eq!(
            s.get("p1", "ssh/password").unwrap().as_deref(),
            Some("hunter2")
        );
        assert!(s.exists("p1", "ssh/password"));
        s.delete("p1", "ssh/password").unwrap();
        assert!(!s.exists("p1", "ssh/password"));
    }

    #[test]
    fn delete_all_removes_only_that_owner() {
        let (_dir, s) = secrets();
        s.set("p1", "ssh/password", "a").unwrap();
        s.set("p1", "db/x/password", "b").unwrap();
        s.set("p2", "ssh/password", "c").unwrap();
        s.set(GLOBAL, "pangolin/api-key", "d").unwrap();
        s.delete_all("p1").unwrap();
        assert!(!s.exists("p1", "ssh/password"));
        assert!(!s.exists("p1", "db/x/password"));
        assert!(s.exists("p2", "ssh/password"));
        assert!(s.exists(GLOBAL, "pangolin/api-key"));
    }

    #[test]
    fn delete_prefixed_is_scoped() {
        let (_dir, s) = secrets();
        s.set("p1", "backup/t1/s3-secret", "a").unwrap();
        s.set("p1", "backup/t1/db-password", "b").unwrap();
        s.set("p1", "backup/t2/s3-secret", "c").unwrap();
        s.delete_prefixed("p1", "backup/t1/").unwrap();
        assert!(!s.exists("p1", "backup/t1/s3-secret"));
        assert!(!s.exists("p1", "backup/t1/db-password"));
        assert!(s.exists("p1", "backup/t2/s3-secret"));
    }
}
