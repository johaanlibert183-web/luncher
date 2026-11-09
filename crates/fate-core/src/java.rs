//! Java runtime discovery and validation.

use crate::error::{FateError, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The Java major version Minecraft 1.20.1 / Forge 47.x is built for.
pub const TARGET_MAJOR: u32 = 17;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JavaInstall {
    pub path: PathBuf,
    pub major: u32,
    pub version: String,
    pub arch: String,
    pub vendor: String,
    pub source: String,
}

impl JavaInstall {
    pub fn is_64bit(&self) -> bool {
        let a = self.arch.to_ascii_lowercase();
        a.contains("64") || a == "amd64" || a == "x86_64"
    }

    /// Forge 1.20.1 targets Java 17; 18–21 generally work, older or much newer ones do not.
    pub fn is_compatible(&self) -> bool {
        self.is_64bit() && (TARGET_MAJOR..=21).contains(&self.major)
    }

    pub fn status_text(&self) -> String {
        if !self.is_64bit() {
            return "32-bit runtime — Minecraft 1.20.1 needs 64-bit Java 17".to_string();
        }
        if self.major < TARGET_MAJOR {
            return format!("Java {} is too old — install Java 17", self.major);
        }
        if self.major > 21 {
            return format!("Java {} is newer than Forge 47.x supports — use Java 17", self.major);
        }
        if self.major == TARGET_MAJOR {
            "Java 17 — recommended".to_string()
        } else {
            format!("Java {} — usable, but Java 17 is recommended", self.major)
        }
    }
}

/// Parses a JDK `release` file (`JAVA_VERSION="17.0.21"` lines).
pub fn parse_release_text(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim().trim_matches('"').to_string();
            map.insert(k.trim().to_string(), v);
        }
    }
    map
}

/// Extracts the major version from strings like `17.0.21`, `1.8.0_402`, `21`.
pub fn major_from_version(version: &str) -> Option<u32> {
    let cleaned = version.trim().trim_start_matches("1.").to_string();
    let first = if version.trim().starts_with("1.") {
        cleaned.split(['.', '_', '-', '+']).next()
    } else {
        version.trim().split(['.', '_', '-', '+']).next()
    }?;
    first.parse().ok()
}

pub fn parse_release_file(dir: &Path, source: &str) -> Option<JavaInstall> {
    let release = dir.join("release");
    let text = std::fs::read_to_string(&release).ok()?;
    let map = parse_release_text(&text);
    let raw_version = map.get("JAVA_VERSION").cloned()?;
    let major = major_from_version(&raw_version)?;
    let arch = map
        .get("OS_ARCH")
        .cloned()
        .unwrap_or_else(|| if cfg!(target_pointer_width = "64") { "x86_64".into() } else { "x86".into() });
    let java = find_java_exe(dir)?;
    Some(JavaInstall {
        path: java,
        major,
        version: raw_version,
        arch,
        vendor: map
            .get("IMPLEMENTOR")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string()),
        source: source.to_string(),
    })
}

pub fn find_java_exe(home: &Path) -> Option<PathBuf> {
    for rel in ["bin/java.exe", "bin/java", "jre/bin/java.exe", "jre/bin/java"] {
        let candidate = home.join(rel);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Asks the runtime itself when no `release` file exists.
pub fn probe_version(java_exe: &Path) -> Option<(u32, String)> {
    let output = Command::new(java_exe).arg("-version").output().ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    // version "17.0.21" 2026-10-20
    let idx = text.find("version \"")? + "version \"".len();
    let rest = &text[idx..];
    let end = rest.find('"')?;
    let raw = &rest[..end];
    Some((major_from_version(raw)?, raw.to_string()))
}

fn push_if_java(out: &mut Vec<JavaInstall>, dir: PathBuf, source: &str) {
    if !dir.is_dir() {
        return;
    }
    if let Some(install) = parse_release_file(&dir, source) {
        out.push(install);
    } else if let Some(exe) = find_java_exe(&dir) {
        if let Some((major, version)) = probe_version(&exe) {
            out.push(JavaInstall {
                path: exe,
                major,
                version,
                arch: if cfg!(target_pointer_width = "64") { "x86_64".into() } else { "x86".into() },
                vendor: "unknown".to_string(),
                source: source.to_string(),
            });
        }
    }
}

fn glob_dirs(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.push(path);
        }
    }
}

/// Scans the usual Windows install locations, `JAVA_HOME`, `PATH` and Mojang runtimes.
pub fn detect(minecraft_dir: Option<&Path>) -> Vec<JavaInstall> {
    let mut found: Vec<JavaInstall> = Vec::new();
    let mut roots: Vec<PathBuf> = Vec::new();

    if let Some(home) = std::env::var_os("JAVA_HOME") {
        roots.push(PathBuf::from(home));
    }

    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            if dir.join("java.exe").is_file() {
                // `<jdk>/bin` -> `<jdk>`
                if let Some(parent) = dir.parent() {
                    roots.push(parent.to_path_buf());
                }
            }
        }
    }

    let mut vendor_roots: Vec<PathBuf> = Vec::new();
    for base in crate::paths::program_files() {
        vendor_roots.push(base.join("Eclipse Adoptium"));
        vendor_roots.push(base.join("Java"));
        vendor_roots.push(base.join("Microsoft"));
        vendor_roots.push(base.join("Zulu"));
        vendor_roots.push(base.join("BellSoft"));
        vendor_roots.push(base.join("Amazon Corretto"));
    }
    if let Some(local) = crate::paths::localappdata() {
        vendor_roots.push(local.join("Programs").join("Eclipse Adoptium"));
        vendor_roots.push(local.join("Programs").join("Microsoft"));
    }
    for root in vendor_roots {
        glob_dirs(&root, &mut roots);
    }

    if let Some(mc) = minecraft_dir {
        // Mojang's own runtimes: runtime/<name>/<platform>/<name>
        let runtime = mc.join("runtime");
        let mut names = Vec::new();
        glob_dirs(&runtime, &mut names);
        for name in names {
            let mut platforms = Vec::new();
            glob_dirs(&name, &mut platforms);
            for platform in platforms {
                if let Ok(inner) = std::fs::read_dir(&platform) {
                    for entry in inner.flatten() {
                        let p = entry.path();
                        if p.is_dir() {
                            roots.push(p);
                        }
                    }
                }
                roots.push(platform);
            }
        }
    }

    for root in roots {
        push_if_java(&mut found, root, "detected");
    }

    // De-duplicate by resolved executable path.
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found.dedup_by(|a, b| {
        a.path.to_string_lossy().to_ascii_lowercase() == b.path.to_string_lossy().to_ascii_lowercase()
    });
    found.sort_by(|a, b| {
        let rank = |j: &JavaInstall| if j.major == TARGET_MAJOR { 0 } else { 1 };
        rank(a)
            .cmp(&rank(b))
            .then(b.major.cmp(&a.major))
            .then(a.path.cmp(&b.path))
    });
    found
}

/// Picks the runtime to launch with: an explicit override wins, then the best detection.
pub fn select<'a>(
    installs: &'a [JavaInstall],
    override_path: Option<&Path>,
) -> Result<&'a JavaInstall> {
    if let Some(path) = override_path {
        if let Some(j) = installs.iter().find(|j| j.path == path) {
            return Ok(j);
        }
        if path.is_file() {
            return Err(FateError::msg(format!(
                "{} exists but its version could not be validated; pick a Java 17 runtime from the list",
                path.display()
            )));
        }
        return Err(FateError::msg(format!(
            "configured Java executable not found: {}",
            path.display()
        )));
    }
    if let Some(j) = installs.iter().find(|j| j.major == TARGET_MAJOR && j.is_64bit()) {
        return Ok(j);
    }
    if let Some(j) = installs.iter().find(|j| j.is_compatible()) {
        return Ok(j);
    }
    Err(FateError::JavaNotFound(
        "install a 64-bit Java 17 runtime (Temurin, Adoptium or Microsoft OpenJDK) and press Rescan"
            .to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_release_file_format() {
        let text = "IMPLEMENTOR=\"Eclipse Adoptium\"\nJAVA_VERSION=\"17.0.21\"\nOS_ARCH=\"x86_64\"\n";
        let map = parse_release_text(text);
        assert_eq!(map.get("JAVA_VERSION").unwrap(), "17.0.21");
        assert_eq!(map.get("OS_ARCH").unwrap(), "x86_64");
    }

    #[test]
    fn major_versions() {
        assert_eq!(major_from_version("17.0.21"), Some(17));
        assert_eq!(major_from_version("1.8.0_402"), Some(8));
        assert_eq!(major_from_version("21"), Some(21));
        assert_eq!(major_from_version("21.0.1+12"), Some(21));
    }

    #[test]
    fn compatibility_rules() {
        let mk = |major, arch: &str| JavaInstall {
            path: PathBuf::from("java.exe"),
            major,
            version: major.to_string(),
            arch: arch.to_string(),
            vendor: "test".into(),
            source: "test".into(),
        };
        assert!(mk(17, "x86_64").is_compatible());
        assert!(!mk(17, "x86").is_64bit());
        assert!(!mk(17, "x86").is_compatible());
        assert!(!mk(8, "x86_64").is_compatible());
        assert!(!mk(25, "x86_64").is_compatible());
        assert!(mk(21, "amd64").is_compatible());
    }
}
