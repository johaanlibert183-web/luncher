//! Filesystem locations, small path helpers and local network discovery.

use std::io;
use std::net::UdpSocket;
use std::path::{Path, PathBuf};

/// `%APPDATA%` expanded, if present.
pub fn appdata() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from)
}

pub fn localappdata() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

pub fn program_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(v) = std::env::var_os(var) {
            out.push(PathBuf::from(v));
        }
    }
    out
}

/// Where the launcher keeps settings, logs and its own metadata.
pub fn data_dir() -> PathBuf {
    appdata()
        .map(|p| p.join("FateLauncher"))
        .unwrap_or_else(|| PathBuf::from("fate-launcher-data"))
}

pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}

pub fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

/// The vanilla launcher's game directory, used as the default detection target.
pub fn default_minecraft_dir() -> PathBuf {
    appdata()
        .map(|p| p.join(".minecraft"))
        .unwrap_or_else(|| PathBuf::from(".minecraft"))
}

pub fn ensure_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

pub fn exists(path: &Path) -> bool {
    path.exists()
}

/// Private IPv4 addresses of this machine, best effort.
///
/// The UDP `connect` performs no traffic; it only asks the OS which local address
/// would be used for egress, which is the address other machines on the LAN can reach.
pub fn local_ipv4s() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = sock.local_addr() {
                let ip = addr.ip().to_string();
                if is_private_v4(&ip) {
                    out.push(ip);
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// True for 10/8, 172.16/12 and 192.168/16.
pub fn is_private_v4(ip: &str) -> bool {
    let parts: Vec<u8> = ip.split('.').filter_map(|p| p.parse().ok()).collect();
    if parts.len() != 4 {
        return false;
    }
    match (parts[0], parts[1]) {
        (10, _) => true,
        (172, b) if (16..=31).contains(&b) => true,
        (192, 168) => true,
        _ => false,
    }
}

/// Formats a directory for display, shortening the user's home to `~`.
pub fn display_path(path: &Path) -> String {
    let text = path.display().to_string();
    if let Some(home) = std::env::var_os("USERPROFILE") {
        let home = home.to_string_lossy().to_string();
        if !home.is_empty() && text.starts_with(&home) {
            return format!("~{}", &text[home.len()..]);
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_ranges() {
        assert!(is_private_v4("192.168.1.20"));
        assert!(is_private_v4("10.0.0.5"));
        assert!(is_private_v4("172.20.3.4"));
        assert!(!is_private_v4("172.32.0.1"));
        assert!(!is_private_v4("8.8.8.8"));
        assert!(!is_private_v4("not-an-ip"));
    }
}
