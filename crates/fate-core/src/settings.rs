//! Persistent launcher settings and the low-end graphics profile.

use crate::error::{FateError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerfProfile {
    LowEnd,
    Normal,
}

impl Default for PerfProfile {
    fn default() -> Self {
        PerfProfile::LowEnd
    }
}

impl PerfProfile {
    pub fn all() -> [PerfProfile; 2] {
        [PerfProfile::LowEnd, PerfProfile::Normal]
    }

    pub fn label(self) -> &'static str {
        match self {
            PerfProfile::LowEnd => "Low-end (2 cores / 4 GB)",
            PerfProfile::Normal => "Normal",
        }
    }

    pub fn render_distance(self) -> u32 {
        match self {
            PerfProfile::LowEnd => 6,
            PerfProfile::Normal => 10,
        }
    }

    pub fn simulation_distance(self) -> u32 {
        match self {
            PerfProfile::LowEnd => 5,
            PerfProfile::Normal => 8,
        }
    }

    pub fn particles(self) -> &'static str {
        match self {
            PerfProfile::LowEnd => "minimal",
            PerfProfile::Normal => "all",
        }
    }

    /// Graphics keys written into `options.txt` when the player asks for the profile.
    pub fn options_overrides(self) -> Vec<(&'static str, String)> {
        let mut out = vec![
            ("renderDistance", self.render_distance().to_string()),
            ("simulationDistance", self.simulation_distance().to_string()),
            ("particles", self.particles().to_string()),
            ("entityShadows", bool_str(self == PerfProfile::Normal)),
            ("ao", bool_str(self == PerfProfile::Normal)),
            ("graphicsMode", "0".to_string()),
            ("maxFps", if self == PerfProfile::LowEnd { "60" } else { "260" }.to_string()),
            ("enableVsync", bool_str(true)),
            ("biomeBlendRadius", "1".to_string()),
            ("mipmapLevels", "4".to_string()),
        ];
        if self == PerfProfile::LowEnd {
            out.push(("renderClouds", "false".to_string()));
            out.push(("fancyGraphics", "false".to_string()));
            out.push(("useNativeTransport", "true".to_string()));
            out.push(("screenEffectScale", "0.5".to_string()));
        }
        out
    }
}

fn bool_str(v: bool) -> String {
    if v { "true" } else { "false" }.to_string()
}

/// Applies the profile to an `options.txt` body, preserving every unrelated line.
///
/// Returns the new body and the keys that actually changed. Lines for keys the file
/// does not contain yet are appended. Never removes user settings.
pub fn apply_options_profile(body: &str, profile: PerfProfile) -> (String, Vec<String>) {
    let overrides = profile.options_overrides();
    let mut lines: Vec<String> = body
        .lines()
        .filter(|l| !l.trim().is_empty() || body.trim().is_empty())
        .map(|l| l.to_string())
        .collect();
    let mut changed = Vec::new();

    for (key, value) in overrides {
        let wanted = format!("{key}:{value}");
        let mut found = false;
        for line in lines.iter_mut() {
            let current_key = line.split(':').next().unwrap_or("").trim();
            if current_key == key {
                if line.trim() != wanted {
                    *line = wanted.clone();
                    changed.push(key.to_string());
                }
                found = true;
                break;
            }
        }
        if !found {
            lines.push(wanted);
            changed.push(key.to_string());
        }
    }

    let mut out = lines.join("\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    (out, changed)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub minecraft_dir: PathBuf,
    /// Optional override; when unset `<minecraft_dir>/mods` is used.
    pub mods_dir: Option<PathBuf>,
    /// Optional path to `pack.json`; when unset the bundled pack is used.
    pub pack_path: Option<PathBuf>,
    /// Optional Java executable override.
    pub java_path: Option<PathBuf>,
    pub memory_auto: bool,
    /// Used only when `memory_auto` is false.
    pub memory_mb: u32,
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
    pub profile: PerfProfile,
    pub player_name: String,
    /// Offline identity: only valid for private/LAN play.
    pub offline_mode: bool,
    pub extra_jvm_args: String,
    pub extra_game_args: String,
    pub accept_offline_warning: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            minecraft_dir: crate::paths::default_minecraft_dir(),
            mods_dir: None,
            pack_path: None,
            java_path: None,
            memory_auto: true,
            memory_mb: 2048,
            width: 1280,
            height: 720,
            fullscreen: false,
            profile: PerfProfile::LowEnd,
            player_name: "Player".to_string(),
            offline_mode: true,
            extra_jvm_args: String::new(),
            extra_game_args: String::new(),
            accept_offline_warning: false,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> (Settings, Option<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(mut s) => {
                    s.sanitize();
                    (s, None)
                }
                Err(e) => (
                    Settings::default(),
                    Some(format!("settings.json could not be parsed ({e}); defaults are in use")),
                ),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Settings::default(), None),
            Err(e) => (
                Settings::default(),
                Some(format!("settings.json could not be read ({e}); defaults are in use")),
            ),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            crate::paths::ensure_dir(parent).map_err(|e| FateError::write(parent, e))?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| FateError::msg(format!("could not serialize settings: {e}")))?;
        std::fs::write(path, text).map_err(|e| FateError::write(path, e))
    }

    fn sanitize(&mut self) {
        if self.width < 320 {
            self.width = 1280;
        }
        if self.height < 240 {
            self.height = 720;
        }
        if self.memory_mb == 0 {
            self.memory_mb = 2048;
        }
        if self.player_name.trim().is_empty() {
            self.player_name = "Player".to_string();
        }
        // Offline names must be safe to pass on a command line.
        self.player_name = self
            .player_name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .take(16)
            .collect();
        if self.player_name.is_empty() {
            self.player_name = "Player".to_string();
        }
    }

    pub fn mods_dir(&self) -> PathBuf {
        self.mods_dir
            .clone()
            .unwrap_or_else(|| self.minecraft_dir.join("mods"))
    }

    /// Heap size actually used, clamped to what this machine can spare.
    pub fn effective_memory_mb(&self, total_mb: u32, available_mb: u32) -> u32 {
        let recommended = crate::launch::recommend_heap_mb(total_mb, available_mb);
        if self.memory_auto {
            recommended
        } else {
            let ceiling = total_mb.saturating_sub(1024).max(1024);
            self.memory_mb.clamp(1024, ceiling.max(recommended).max(1024))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_name_is_sanitized() {
        let (mut s, _) = Settings::load(Path::new("definitely/not/here.json"));
        s.player_name = "bad name!<>".to_string();
        s.sanitize();
        assert_eq!(s.player_name, "badname");
    }

    #[test]
    fn options_profile_updates_keys_and_keeps_others() {
        let before = "fov:0.5\nrenderDistance:16\nnarrator:0\n";
        let (after, changed) = apply_options_profile(before, PerfProfile::LowEnd);
        assert!(after.contains("fov:0.5"));
        assert!(after.contains("narrator:0"));
        assert!(after.contains("renderDistance:6"));
        assert!(after.contains("particles:minimal"));
        assert!(changed.contains(&"renderDistance".to_string()));
        assert!(!changed.contains(&"fov".to_string()));
    }

    #[test]
    fn options_profile_appends_missing_keys() {
        let (after, changed) = apply_options_profile("", PerfProfile::LowEnd);
        assert!(after.contains("renderDistance:6"));
        assert!(changed.iter().any(|k| k == "simulationDistance"));
    }

    #[test]
    fn memory_override_is_clamped_to_machine() {
        let mut s = Settings::default();
        s.memory_auto = false;
        s.memory_mb = 8192;
        let v = s.effective_memory_mb(4096, 2500);
        assert!(v <= 4096, "must never ask for more than the machine has");
    }
}
