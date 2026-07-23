//! Platform detection.

use std::fs;

pub struct Platform {
    pub os: &'static str,   // "macos" | "linux" | "windows"
    pub arch: &'static str, // "aarch64" | "x86_64" | ...
    /// Running INSIDE a WSL distro (Linux binary).
    pub wsl: bool,
    /// The WSL distro dev tools install into when running on a Windows host.
    pub wsl_distro: Option<String>,
    pub pkg: PkgManager,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PkgManager {
    Brew,
    Apt,
}

pub fn detect() -> Platform {
    let wsl = fs::read_to_string("/proc/version")
        .map(|v| v.to_lowercase().contains("microsoft"))
        .unwrap_or(false);
    Platform {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        wsl,
        wsl_distro: None,
        // On a Windows host, Apt describes the WSL side where dev tools go
        // (Ubuntu-family distros); Host apps use winget instead.
        pkg: if std::env::consts::OS == "macos" {
            PkgManager::Brew
        } else {
            PkgManager::Apt
        },
    }
}

impl Platform {
    /// True when the TUI itself runs on Windows (not inside WSL).
    pub fn windows_host(&self) -> bool {
        self.os == "windows"
    }

    pub fn label(&self) -> String {
        let mut label = format!(
            "{} {}{}",
            self.os,
            self.arch,
            if self.wsl { " (WSL)" } else { "" }
        );
        if let Some(d) = &self.wsl_distro {
            label.push_str(&format!(" → WSL: {d}"));
        }
        label
    }

    /// List installed WSL distros (Windows host only). `wsl.exe -l -q`
    /// prints UTF-16LE, so decode manually.
    pub fn wsl_distros() -> Vec<String> {
        let Ok(out) = std::process::Command::new("wsl.exe").args(["-l", "-q"]).output() else {
            return Vec::new();
        };
        if !out.status.success() {
            return Vec::new();
        }
        let bytes = &out.stdout;
        let text = if bytes.iter().take(64).any(|&b| b == 0) {
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        } else {
            String::from_utf8_lossy(bytes).into_owned()
        };
        text.lines()
            .map(|l| l.trim().trim_start_matches('\u{feff}').to_string())
            .filter(|l| !l.is_empty())
            .collect()
    }
}
