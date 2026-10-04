//! OpenSSH-style known-hosts store kept in the app config directory.
//!
//! Lines are `host keytype base64` (port 22) or `[host]:port keytype base64`,
//! the same plain layout OpenSSH writes without `HashKnownHosts`.

use std::path::PathBuf;

use parking_lot::Mutex;
use russh::keys::{HashAlg, PublicKey};
use serde::Serialize;
use specta::Type;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct KnownHost {
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
}

/// Why a server's host key was not accepted, with what the user needs to decide.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyIssue {
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
    /// Fingerprints currently trusted for this host; empty on first contact.
    pub known_fingerprints: Vec<String>,
    /// The offered key in OpenSSH format, passed back to trust it.
    pub key: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Trusted,
    Unknown,
    Changed(Vec<String>),
}

pub struct KnownHosts {
    file: PathBuf,
    lock: Mutex<()>,
}

pub fn fingerprint(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

fn pattern(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_string()
    } else {
        format!("[{host}]:{port}")
    }
}

fn parse_pattern(pattern: &str) -> (String, u16) {
    if let Some(rest) = pattern.strip_prefix('[') {
        if let Some((host, port)) = rest.split_once("]:") {
            if let Ok(port) = port.parse() {
                return (host.to_string(), port);
            }
        }
    }
    (pattern.to_string(), 22)
}

struct Line {
    pattern: String,
    key: PublicKey,
}

impl KnownHosts {
    pub fn new(file: PathBuf) -> Self {
        Self { file, lock: Mutex::new(()) }
    }

    fn read(&self) -> AppResult<Vec<Line>> {
        let text = match std::fs::read_to_string(&self.file) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        Ok(text
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    return None;
                }
                let (pattern, key) = line.split_once(char::is_whitespace)?;
                let key = PublicKey::from_openssh(key.trim()).ok()?;
                Some(Line { pattern: pattern.to_string(), key })
            })
            .collect())
    }

    /// Lines this store cannot interpret (comments, hashed hosts, markers).
    /// They are carried over untouched on every rewrite.
    fn foreign_lines(&self) -> Vec<String> {
        let Ok(text) = std::fs::read_to_string(&self.file) else {
            return Vec::new();
        };
        text.lines()
            .filter(|line| {
                let line = line.trim();
                let parsed = line.split_once(char::is_whitespace).is_some_and(|(_, key)| PublicKey::from_openssh(key.trim()).is_ok());
                !line.is_empty() && (line.starts_with('#') || !parsed)
            })
            .map(str::to_string)
            .collect()
    }

    fn write(&self, lines: &[Line]) -> AppResult<()> {
        let mut text = String::new();
        for line in self.foreign_lines() {
            text.push_str(&line);
            text.push('\n');
        }
        for line in lines {
            let key = line.key.to_openssh().map_err(|e| AppError::internal(e.to_string()))?;
            text.push_str(&format!("{} {}\n", line.pattern, key));
        }
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.file.with_extension("tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.file)?;
        Ok(())
    }

    pub fn check(&self, host: &str, port: u16, key: &PublicKey) -> AppResult<Verdict> {
        let _guard = self.lock.lock();
        let pattern = pattern(host, port);
        let known: Vec<PublicKey> = self.read()?.into_iter().filter(|l| l.pattern == pattern).map(|l| l.key).collect();
        if known.is_empty() {
            Ok(Verdict::Unknown)
        } else if known.iter().any(|k| k.key_data() == key.key_data()) {
            Ok(Verdict::Trusted)
        } else {
            Ok(Verdict::Changed(known.iter().map(fingerprint).collect()))
        }
    }

    /// Trust `key` for a host. With `replace`, previously trusted keys for
    /// that host are dropped (the explicit "key changed → replace" action).
    pub fn trust(&self, host: &str, port: u16, key: &str, replace: bool) -> AppResult<()> {
        let key = PublicKey::from_openssh(key).map_err(|e| AppError::invalid(format!("Invalid host key: {e}")))?;
        let _guard = self.lock.lock();
        let pattern = pattern(host, port);
        let mut lines = self.read()?;
        if replace {
            lines.retain(|l| l.pattern != pattern);
        }
        if !lines.iter().any(|l| l.pattern == pattern && l.key.key_data() == key.key_data()) {
            lines.push(Line { pattern, key });
        }
        self.write(&lines)
    }

    pub fn forget(&self, host: &str, port: u16) -> AppResult<()> {
        let _guard = self.lock.lock();
        let pattern = pattern(host, port);
        let mut lines = self.read()?;
        lines.retain(|l| l.pattern != pattern);
        self.write(&lines)
    }

    pub fn list(&self) -> AppResult<Vec<KnownHost>> {
        let _guard = self.lock.lock();
        Ok(self
            .read()?
            .iter()
            .map(|l| {
                let (host, port) = parse_pattern(&l.pattern);
                KnownHost { host, port, key_type: l.key.algorithm().to_string(), fingerprint: fingerprint(&l.key) }
            })
            .collect())
    }

    pub fn issue(&self, host: &str, port: u16, key: &PublicKey, known: Vec<String>) -> HostKeyIssue {
        HostKeyIssue {
            host: host.to_string(),
            port,
            key_type: key.algorithm().to_string(),
            fingerprint: fingerprint(key),
            known_fingerprints: known,
            key: key.to_openssh().unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_A: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILM+rvN+ot98qgEN796jTiQfZfG1KaT0PtFDJ/XFSqti";
    const KEY_B: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIB9dG4kjRhQTtWTVzd2t27+t0DEHBPW7iOD23TUiYLio";

    fn store() -> (tempfile::TempDir, KnownHosts) {
        let dir = tempfile::tempdir().unwrap();
        let kh = KnownHosts::new(dir.path().join("known_hosts"));
        (dir, kh)
    }

    fn key(s: &str) -> PublicKey {
        PublicKey::from_openssh(s).unwrap()
    }

    #[test]
    fn unknown_then_trusted() {
        let (_d, kh) = store();
        assert_eq!(kh.check("h", 22, &key(KEY_A)).unwrap(), Verdict::Unknown);
        kh.trust("h", 22, KEY_A, false).unwrap();
        assert_eq!(kh.check("h", 22, &key(KEY_A)).unwrap(), Verdict::Trusted);
        // Another port is a different host entry.
        assert_eq!(kh.check("h", 2222, &key(KEY_A)).unwrap(), Verdict::Unknown);
    }

    #[test]
    fn changed_key_is_detected_and_replaceable() {
        let (_d, kh) = store();
        kh.trust("h", 2222, KEY_A, false).unwrap();
        match kh.check("h", 2222, &key(KEY_B)).unwrap() {
            Verdict::Changed(old) => assert_eq!(old, vec![fingerprint(&key(KEY_A))]),
            other => panic!("unexpected {other:?}"),
        }
        kh.trust("h", 2222, KEY_B, true).unwrap();
        assert_eq!(kh.check("h", 2222, &key(KEY_B)).unwrap(), Verdict::Trusted);
        assert!(matches!(kh.check("h", 2222, &key(KEY_A)).unwrap(), Verdict::Changed(_)));
    }

    #[test]
    fn list_and_forget() {
        let (_d, kh) = store();
        kh.trust("a.example", 22, KEY_A, false).unwrap();
        kh.trust("b.example", 2200, KEY_B, false).unwrap();
        kh.trust("a.example", 22, KEY_A, false).unwrap();
        let all = kh.list().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!((all[1].host.as_str(), all[1].port), ("b.example", 2200));
        assert_eq!(all[0].key_type, "ssh-ed25519");
        assert!(all[0].fingerprint.starts_with("SHA256:"));
        kh.forget("a.example", 22).unwrap();
        assert_eq!(kh.list().unwrap().len(), 1);
    }

    #[test]
    fn unknown_lines_survive_rewrites() {
        let (dir, kh) = store();
        let file = dir.path().join("known_hosts");
        std::fs::write(&file, format!("# my comment\n|1|abc=|def= ssh-rsa notakey\nold.example {KEY_A}\n")).unwrap();
        kh.trust("new.example", 22, KEY_B, false).unwrap();
        kh.forget("old.example", 22).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(text, format!("# my comment\n|1|abc=|def= ssh-rsa notakey\nnew.example {KEY_B}\n"));
    }

    #[test]
    fn file_uses_openssh_layout() {
        let (dir, kh) = store();
        kh.trust("h", 22, KEY_A, false).unwrap();
        kh.trust("h", 2222, KEY_B, false).unwrap();
        let text = std::fs::read_to_string(dir.path().join("known_hosts")).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], format!("h {KEY_A}"));
        assert_eq!(lines[1], format!("[h]:2222 {KEY_B}"));
    }
}
