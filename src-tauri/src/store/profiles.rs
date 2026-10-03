//! Server profiles (connection details). Secrets are stored separately in the keyring.

use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{load_json, save_json, Paths};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::shell::validate;

pub const SECRET_PASSWORD: &str = "ssh/password";
pub const SECRET_PASSPHRASE: &str = "ssh/passphrase";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AuthType {
    Password,
    Key,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub label: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_type: AuthType,
    pub key_path: Option<String>,
    pub is_default: bool,
}

/// On-disk shape. The aliases accept the snake_case layout written by Jarvis
/// v1, which used the same file, so an existing install is picked up as-is.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredProfile {
    id: String,
    label: String,
    host: String,
    port: u16,
    username: String,
    #[serde(alias = "auth_type")]
    auth_type: AuthType,
    #[serde(default, alias = "key_path")]
    key_path: Option<String>,
    #[serde(default)]
    is_default: bool,
}

impl From<StoredProfile> for Profile {
    fn from(p: StoredProfile) -> Self {
        Self {
            id: p.id,
            label: p.label,
            host: p.host,
            port: p.port,
            username: p.username,
            auth_type: p.auth_type,
            key_path: p.key_path.filter(|k| !k.is_empty()),
            is_default: p.is_default,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInput {
    /// `None` creates a new profile.
    pub id: Option<String>,
    pub label: String,
    pub host: String,
    pub port: u32,
    pub username: String,
    pub auth_type: AuthType,
    pub key_path: Option<String>,
    /// `None` keeps whatever is stored; `Some` replaces it.
    pub password: Option<String>,
    pub passphrase: Option<String>,
    /// Explicitly forget the stored password / passphrase.
    pub clear_secrets: bool,
}

pub struct ProfileStore {
    file: PathBuf,
    profiles: Mutex<Vec<Profile>>,
}

impl ProfileStore {
    /// Returns the store plus a notice if the file was corrupt and set aside.
    pub fn load(paths: &Paths) -> (Self, Option<AppError>) {
        let file = paths.profiles_file();
        let (profiles, notice) = match load_json::<Vec<StoredProfile>>(&file) {
            Ok(p) => (p.into_iter().map(Profile::from).collect(), None),
            Err(e) => (Vec::new(), Some(e)),
        };
        (
            Self {
                file,
                profiles: Mutex::new(profiles),
            },
            notice,
        )
    }

    pub fn list(&self) -> Vec<Profile> {
        self.profiles.lock().clone()
    }

    pub fn get(&self, id: &str) -> AppResult<Profile> {
        self.profiles
            .lock()
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "profile"))
    }

    pub fn upsert(&self, input: &ProfileInput) -> AppResult<Profile> {
        let label = input.label.trim();
        let host = input.host.trim();
        let username = input.username.trim();
        if label.is_empty() {
            return Err(AppError::invalid("Label is required"));
        }
        validate::host(host)?;
        let port = validate::port(input.port)?;
        if username.is_empty()
            || username
                .bytes()
                .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
        {
            return Err(AppError::invalid("Invalid user name"));
        }
        let key_path = match input.auth_type {
            AuthType::Key => {
                let raw = input.key_path.as_deref().unwrap_or("").trim();
                if raw.is_empty() {
                    return Err(AppError::invalid("A private key file is required"));
                }
                Some(resolve_key_path(Path::new(raw))?)
            }
            AuthType::Password => None,
        };

        let mut profiles = self.profiles.lock();
        let profile = match &input.id {
            Some(id) => {
                let existing = profiles
                    .iter_mut()
                    .find(|p| &p.id == id)
                    .ok_or_else(|| AppError::new(ErrorCode::NotFound, "profile"))?;
                existing.label = label.to_string();
                existing.host = host.to_string();
                existing.port = port;
                existing.username = username.to_string();
                existing.auth_type = input.auth_type;
                existing.key_path = key_path;
                existing.clone()
            }
            None => {
                let profile = Profile {
                    id: uuid::Uuid::new_v4().to_string(),
                    label: label.to_string(),
                    host: host.to_string(),
                    port,
                    username: username.to_string(),
                    auth_type: input.auth_type,
                    key_path,
                    is_default: false,
                };
                profiles.push(profile.clone());
                profile
            }
        };
        save_json(&self.file, &*profiles)?;
        Ok(profile)
    }

    pub fn delete(&self, id: &str) -> AppResult<()> {
        let mut profiles = self.profiles.lock();
        let before = profiles.len();
        profiles.retain(|p| p.id != id);
        if profiles.len() == before {
            return Err(AppError::new(ErrorCode::NotFound, "profile"));
        }
        save_json(&self.file, &*profiles)
    }

    /// Mark one profile (or none) as the auto-connect default.
    pub fn set_default(&self, id: Option<&str>) -> AppResult<()> {
        let mut profiles = self.profiles.lock();
        if let Some(id) = id {
            if !profiles.iter().any(|p| p.id == id) {
                return Err(AppError::new(ErrorCode::NotFound, "profile"));
            }
        }
        for p in profiles.iter_mut() {
            p.is_default = Some(p.id.as_str()) == id;
        }
        save_json(&self.file, &*profiles)
    }
}

/// Accept what the user picked in the file dialog and turn it into a usable
/// private key path: a `.pub` file resolves to its private sibling, and files
/// that are not OpenSSH/PEM private keys are rejected with a clear reason.
pub fn resolve_key_path(picked: &Path) -> AppResult<String> {
    let picked = expand_home(picked);
    let path = if picked.extension().is_some_and(|e| e == "pub") {
        let private = picked.with_extension("");
        if !private.is_file() {
            return Err(AppError::new(
                ErrorCode::KeyFileInvalid,
                format!(
                    "{} is a public key and its private key {} was not found",
                    picked.display(),
                    private.display()
                ),
            ));
        }
        private
    } else {
        picked
    };
    let head = read_head(&path).map_err(|e| {
        AppError::new(
            ErrorCode::KeyFileInvalid,
            format!("{}: {}", path.display(), e),
        )
    })?;
    let text = String::from_utf8_lossy(&head);
    if text.starts_with("PuTTY-User-Key-File") {
        return Err(AppError::new(
            ErrorCode::KeyFileInvalid,
            "PuTTY .ppk keys are not supported. Convert it with PuTTYgen: Conversions → Export OpenSSH key.",
        ));
    }
    if !text.contains("PRIVATE KEY-----") {
        return Err(AppError::new(
            ErrorCode::KeyFileInvalid,
            format!("{} is not a private key file", path.display()),
        ));
    }
    Ok(path.to_string_lossy().into_owned())
}

fn read_head(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = vec![0u8; 256];
    let mut file = std::fs::File::open(path)?;
    let n = file.read(&mut buf)?;
    buf.truncate(n);
    Ok(buf)
}

pub fn expand_home(path: &Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => dirs::home_dir()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| path.to_path_buf()),
        Err(_) => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(label: &str) -> ProfileInput {
        ProfileInput {
            id: None,
            label: label.into(),
            host: "example.com".into(),
            port: 22,
            username: "root".into(),
            auth_type: AuthType::Password,
            key_path: None,
            password: None,
            passphrase: None,
            clear_secrets: false,
        }
    }

    fn store() -> (tempfile::TempDir, ProfileStore) {
        let dir = tempfile::tempdir().unwrap();
        let (store, notice) = ProfileStore::load(&Paths::new(dir.path().to_path_buf()));
        assert!(notice.is_none());
        (dir, store)
    }

    #[test]
    fn create_update_delete_persist() {
        let (dir, store) = store();
        let a = store.upsert(&input("A")).unwrap();
        let mut edit = input("A2");
        edit.id = Some(a.id.clone());
        edit.port = 2222;
        store.upsert(&edit).unwrap();
        let (reloaded, _) = ProfileStore::load(&Paths::new(dir.path().to_path_buf()));
        let all = reloaded.list();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].label, "A2");
        assert_eq!(all[0].port, 2222);
        reloaded.delete(&a.id).unwrap();
        assert!(reloaded.list().is_empty());
        assert!(reloaded.delete(&a.id).is_err());
    }

    #[test]
    fn only_one_default() {
        let (_dir, store) = store();
        let a = store.upsert(&input("A")).unwrap();
        let b = store.upsert(&input("B")).unwrap();
        store.set_default(Some(&a.id)).unwrap();
        store.set_default(Some(&b.id)).unwrap();
        let defaults: Vec<_> = store.list().into_iter().filter(|p| p.is_default).collect();
        assert_eq!(defaults.len(), 1);
        assert_eq!(defaults[0].id, b.id);
        store.set_default(None).unwrap();
        assert!(store.list().iter().all(|p| !p.is_default));
    }

    #[test]
    fn validation() {
        let (_dir, store) = store();
        let mut bad = input("");
        assert!(store.upsert(&bad).is_err());
        bad = input("x");
        bad.host = "bad host".into();
        assert!(store.upsert(&bad).is_err());
        bad = input("x");
        bad.port = 0;
        assert!(store.upsert(&bad).is_err());
        bad = input("x");
        bad.auth_type = AuthType::Key;
        assert!(store.upsert(&bad).is_err());
    }

    #[test]
    fn reads_v1_layout() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("profiles.json"),
            r#"[{"id":"abc","label":"Old","host":"10.0.0.1","port":22,"username":"root","auth_type":"key","key_path":"/home/u/.ssh/id_ed25519"}]"#,
        )
        .unwrap();
        let (store, notice) = ProfileStore::load(&Paths::new(dir.path().to_path_buf()));
        assert!(notice.is_none());
        let p = &store.list()[0];
        assert_eq!(p.auth_type, AuthType::Key);
        assert_eq!(p.key_path.as_deref(), Some("/home/u/.ssh/id_ed25519"));
        assert!(!p.is_default);
    }

    #[test]
    fn key_path_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let private = dir.path().join("id_ed25519");
        let public = dir.path().join("id_ed25519.pub");
        std::fs::write(
            &private,
            "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----\n",
        )
        .unwrap();
        std::fs::write(&public, "ssh-ed25519 AAAA user@host\n").unwrap();
        assert_eq!(
            resolve_key_path(&public).unwrap(),
            private.to_string_lossy()
        );
        assert_eq!(
            resolve_key_path(&private).unwrap(),
            private.to_string_lossy()
        );

        let orphan = dir.path().join("other.pub");
        std::fs::write(&orphan, "ssh-ed25519 AAAA\n").unwrap();
        assert_eq!(
            resolve_key_path(&orphan).unwrap_err().code,
            ErrorCode::KeyFileInvalid
        );

        let ppk = dir.path().join("key.ppk");
        std::fs::write(&ppk, "PuTTY-User-Key-File-3: ssh-ed25519\n").unwrap();
        let err = resolve_key_path(&ppk).unwrap_err();
        assert!(err.details.unwrap().contains("PuTTY"));

        let junk = dir.path().join("notes.txt");
        std::fs::write(&junk, "hello").unwrap();
        assert!(resolve_key_path(&junk).is_err());
    }
}
