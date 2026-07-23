//! Platform detection.

use std::fs;

pub struct Platform {
    pub os: &'static str,   // "macos" | "linux"
    pub arch: &'static str, // "aarch64" | "x86_64" | ...
    pub wsl: bool,
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
        pkg: if std::env::consts::OS == "macos" {
            PkgManager::Brew
        } else {
            PkgManager::Apt
        },
    }
}

impl Platform {
    pub fn label(&self) -> String {
        format!(
            "{} {}{}",
            self.os,
            self.arch,
            if self.wsl { " (WSL)" } else { "" }
        )
    }
}
