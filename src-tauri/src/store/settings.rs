//! Global app settings and UI preferences (not tied to a server profile).

use std::collections::BTreeMap;
use std::path::PathBuf;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{load_json_merged, save_json, Paths};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum CursorStyle {
    #[default]
    Block,
    Bar,
    Underline,
}

/// What the right mouse button does in the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum RightClick {
    #[default]
    Menu,
    Paste,
    /// Copy when text is selected, paste otherwise.
    CopyOrPaste,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Bell {
    #[default]
    None,
    Visual,
    Sound,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TerminalPrefs {
    pub font_family: String,
    pub font_size: u8,
    /// Line height in percent of the font's own height.
    pub line_height: u16,
    /// Extra space between characters, in pixels.
    pub letter_spacing: i8,
    /// Name of a built-in terminal colour scheme.
    pub theme: String,
    pub cursor_style: CursorStyle,
    pub cursor_blink: bool,
    pub scrollback: u32,
    /// Typing jumps back to the bottom of the scrollback.
    pub scroll_on_input: bool,
    pub copy_on_select: bool,
    pub right_click: RightClick,
    pub middle_click_paste: bool,
    /// Ctrl+C copies when text is selected and Ctrl+V pastes.
    pub ctrl_copy_paste: bool,
    pub confirm_multiline_paste: bool,
    pub bell: Bell,
    /// Characters that end a word when double-clicking to select.
    pub word_separators: String,
}

impl Default for TerminalPrefs {
    fn default() -> Self {
        Self {
            font_family: "JetBrains Mono Variable".into(),
            font_size: 13,
            line_height: 100,
            letter_spacing: 0,
            theme: "jarvis".into(),
            cursor_style: CursorStyle::Block,
            cursor_blink: true,
            scrollback: 10_000,
            scroll_on_input: true,
            copy_on_select: false,
            right_click: RightClick::Menu,
            middle_click_paste: false,
            ctrl_copy_paste: false,
            confirm_multiline_paste: false,
            bell: Bell::None,
            word_separators: " ()[]{}',\"`".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PangolinSettings {
    pub api_url: String,
    /// Prefix in front of `/v1` found when verifying ("" or "/api").
    pub base_path: String,
    pub org_id: String,
}

impl Default for PangolinSettings {
    fn default() -> Self {
        Self { api_url: "https://api.pangolin.net".into(), base_path: String::new(), org_id: String::new() }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub theme: Theme,
    pub sidebar_collapsed: bool,
    /// Tab ids pinned to the Favourites section.
    pub favourites: Vec<String>,
    /// Tab id → colour tag name.
    pub tab_colors: BTreeMap<String, String>,
    pub terminal: TerminalPrefs,
    /// Default local folder for downloads; `None` means the OS Downloads folder.
    pub download_dir: Option<String>,
    pub pangolin: PangolinSettings,
}

pub struct SettingsStore {
    file: PathBuf,
    settings: Mutex<AppSettings>,
}

impl SettingsStore {
    pub fn load(paths: &Paths) -> (Self, Option<AppError>) {
        let file = paths.settings_file();
        let (settings, notice) = match load_json_merged::<AppSettings>(&file) {
            Ok(s) => (s, None),
            Err(e) => (AppSettings::default(), Some(e)),
        };
        (Self { file, settings: Mutex::new(settings) }, notice)
    }

    pub fn get(&self) -> AppSettings {
        self.settings.lock().clone()
    }

    pub fn set(&self, settings: AppSettings) -> AppResult<()> {
        let mut current = self.settings.lock();
        save_json(&self.file, &settings)?;
        *current = settings;
        Ok(())
    }

    /// Local folder downloads go to unless the user picks another one.
    pub fn download_dir(&self) -> PathBuf {
        self.settings
            .lock()
            .download_dir
            .as_ref()
            .filter(|d| !d.trim().is_empty())
            .map(PathBuf::from)
            .or_else(dirs::download_dir)
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_partial_files() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        std::fs::write(paths.settings_file(), r#"{"sidebarCollapsed":true}"#).unwrap();
        let (store, notice) = SettingsStore::load(&paths);
        assert!(notice.is_none());
        let s = store.get();
        assert!(s.sidebar_collapsed);
        assert_eq!(s.terminal.font_size, 13);
        assert_eq!(s.pangolin.api_url, "https://api.pangolin.net");
    }

    #[test]
    fn set_persists() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        let (store, _) = SettingsStore::load(&paths);
        let mut s = store.get();
        s.favourites = vec!["docker".into()];
        s.tab_colors.insert("docker".into(), "blue".into());
        store.set(s).unwrap();
        let (again, _) = SettingsStore::load(&paths);
        assert_eq!(again.get().favourites, vec!["docker"]);
        assert_eq!(again.get().tab_colors["docker"], "blue");
    }
}
