//! Platform + environment detection.

use std::fs;

/// The target environment we're setting up. This is the axis software.json
/// resolves against: GUI apps are offered on Macos / LinuxDesktop / WindowsHost
/// but skipped on LinuxServer (headless).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Env {
    Macos,
    LinuxDesktop,
    LinuxServer,
    /// The TUI runs on Windows itself; dev tools go into a WSL distro, GUI apps
    /// install on the host via winget.
    WindowsHost,
}

impl Env {
    pub fn is_linux(&self) -> bool {
        matches!(self, Env::LinuxDesktop | Env::LinuxServer)
    }
}

pub struct Platform {
    pub arch: &'static str, // "aarch64" | "x86_64" | ...
    /// Running INSIDE a WSL distro (Linux binary).
    pub wsl: bool,
    pub env: Env,
    /// The WSL distro dev tools install into when running on a Windows host.
    pub wsl_distro: Option<String>,
}

pub fn detect() -> Platform {
    let os = std::env::consts::OS;
    let wsl = fs::read_to_string("/proc/version")
        .map(|v| v.to_lowercase().contains("microsoft"))
        .unwrap_or(false);

    let env = match os {
        "macos" => Env::Macos,
        "windows" => Env::WindowsHost,
        _ => {
            // Linux: desktop if a display server / desktop session is present.
            // Override with RFID_ONBOARD_LINUX=desktop|server (also lets us
            // preview both from a normal machine).
            let desktop = match std::env::var("RFID_ONBOARD_LINUX").as_deref() {
                Ok("desktop") => true,
                Ok("server") => false,
                _ => {
                    std::env::var_os("DISPLAY").is_some()
                        || std::env::var_os("WAYLAND_DISPLAY").is_some()
                        || std::env::var_os("XDG_CURRENT_DESKTOP").is_some()
                }
            };
            if desktop {
                Env::LinuxDesktop
            } else {
                Env::LinuxServer
            }
        }
    };

    Platform {
        arch: std::env::consts::ARCH,
        wsl,
        env,
        wsl_distro: None,
    }
}

impl Platform {
    /// True when the TUI itself runs on Windows (not inside WSL).
    pub fn windows_host(&self) -> bool {
        self.env == Env::WindowsHost
    }

    pub fn label(&self) -> String {
        let mut label = match self.env {
            Env::Macos => format!("macOS {}", self.arch),
            Env::LinuxDesktop => format!("Linux {} (desktop)", self.arch),
            Env::LinuxServer => format!("Linux {} (server)", self.arch),
            Env::WindowsHost => format!("Windows {}", self.arch),
        };
        if self.wsl {
            label.push_str(" · WSL");
        }
        if let Some(d) = &self.wsl_distro {
            label.push_str(&format!(" → {d}"));
        }
        label
    }

    /// List installed WSL distros (Windows host only). `wsl.exe -l -q` prints
    /// UTF-16LE, so decode manually.
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
