//! Platform detection and component definitions.
//!
//! The install/check commands intentionally mirror public-scripts/onboard-gum.sh
//! so the two PoCs stay behaviorally comparable.

use std::fs;

pub struct Platform {
    pub os: &'static str,   // "macos" | "linux"
    pub arch: &'static str, // "aarch64" | "x86_64" | ...
    pub wsl: bool,
    pub pkg: &'static str, // "brew" | "apt"
}

pub fn detect() -> Platform {
    let wsl = fs::read_to_string("/proc/version")
        .map(|v| v.to_lowercase().contains("microsoft"))
        .unwrap_or(false);
    Platform {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        wsl,
        pkg: if std::env::consts::OS == "macos" { "brew" } else { "apt" },
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

/// One shell step of a component install. All steps must be idempotent and
/// non-interactive (interactive tools like `gh auth login` are surfaced as
/// follow-up instructions instead — see `Component::follow_up`).
pub struct Step {
    pub title: &'static str,
    pub cmd: String,
}

pub struct Check {
    pub label: &'static str,
    pub cmd: String,
}

pub struct Component {
    pub name: &'static str,
    pub steps: Vec<Step>,
    pub checks: Vec<Check>,
    pub follow_up: Vec<&'static str>,
}

fn pkg_install(p: &Platform, brew: &str, apt: &str, probe: &str) -> String {
    if p.pkg == "brew" {
        format!("command -v {probe} >/dev/null 2>&1 || brew install {brew}")
    } else {
        format!("command -v {probe} >/dev/null 2>&1 || sudo apt-get install -y {apt}")
    }
}

pub fn components(p: &Platform) -> Vec<Component> {
    let mut core_steps = Vec::new();
    if p.pkg == "apt" {
        core_steps.push(Step {
            title: "Update apt package index",
            cmd: "sudo apt-get update -y".into(),
        });
    }
    core_steps.push(Step {
        title: "Install git",
        cmd: pkg_install(p, "git", "git", "git"),
    });
    core_steps.push(Step {
        title: "Install GitHub CLI",
        cmd: pkg_install(p, "gh", "gh", "gh"),
    });
    core_steps.push(Step {
        title: "Generate SSH key (if missing)",
        cmd: r#"[ -f "$HOME/.ssh/id_ed25519" ] || { mkdir -p "$HOME/.ssh" && chmod 700 "$HOME/.ssh" && ssh-keygen -t ed25519 -C "$(git config --global user.email || echo rfid-lab)" -f "$HOME/.ssh/id_ed25519" -N ""; }"#.into(),
    });

    let core = Component {
        name: "Core (git, gh, SSH)",
        steps: core_steps,
        checks: vec![
            Check { label: "git", cmd: "git --version".into() },
            Check { label: "gh", cmd: "gh --version | head -1".into() },
            Check { label: "SSH key", cmd: r#"test -f "$HOME/.ssh/id_ed25519.pub" && echo present"#.into() },
            Check { label: "gh auth", cmd: "gh auth status >/dev/null 2>&1 && echo logged-in".into() },
        ],
        follow_up: vec![
            "git config --global user.name \"Your Name\"",
            "git config --global user.email \"you@auburn.edu\"",
            "gh auth login --git-protocol ssh   # interactive browser login",
        ],
    };

    let node = Component {
        name: "Node (fnm, LTS, pnpm)",
        steps: vec![
            Step {
                title: "Install fnm",
                cmd: if p.pkg == "brew" {
                    "command -v fnm >/dev/null 2>&1 || brew install fnm".into()
                } else {
                    r#"command -v fnm >/dev/null 2>&1 || [ -x "$HOME/.local/share/fnm/fnm" ] || { curl -fsSL https://fnm.vercel.app/install -o /tmp/fnm-install.sh && bash /tmp/fnm-install.sh --skip-shell; }"#.into()
                },
            },
            Step {
                title: "Install Node LTS + pnpm",
                cmd: r#"export PATH="$HOME/.local/share/fnm:$PATH"; eval "$(fnm env)"; fnm install --lts && fnm default lts-latest; eval "$(fnm env)"; corepack enable pnpm"#.into(),
            },
        ],
        checks: vec![
            Check { label: "fnm", cmd: r#"export PATH="$HOME/.local/share/fnm:$PATH"; fnm --version"#.into() },
            Check { label: "node", cmd: r#"export PATH="$HOME/.local/share/fnm:$PATH"; eval "$(fnm env)"; node --version"#.into() },
            Check { label: "pnpm", cmd: r#"export PATH="$HOME/.local/share/fnm:$PATH"; eval "$(fnm env)"; pnpm --version"#.into() },
        ],
        follow_up: vec![
            r#"echo 'eval "$(fnm env --use-on-cd)"' >> ~/.zshrc   # make node available in new shells"#,
        ],
    };

    let rust = Component {
        name: "Rust (rustup)",
        steps: vec![Step {
            title: "Install rustup toolchain",
            cmd: r#"[ -x "$HOME/.cargo/bin/rustc" ] || { curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o /tmp/rustup-init.sh && sh /tmp/rustup-init.sh -y --no-modify-path; }"#.into(),
        }],
        checks: vec![
            Check { label: "rustc", cmd: r#". "$HOME/.cargo/env" 2>/dev/null; rustc --version"#.into() },
            Check { label: "cargo", cmd: r#". "$HOME/.cargo/env" 2>/dev/null; cargo --version"#.into() },
        ],
        follow_up: vec![r#"echo '. "$HOME/.cargo/env"' >> ~/.zshrc"#],
    };

    let go = Component {
        name: "Go",
        steps: vec![Step {
            title: "Install Go",
            cmd: pkg_install(p, "go", "golang-go", "go"),
        }],
        checks: vec![Check { label: "go", cmd: "go version".into() }],
        follow_up: vec![],
    };

    vec![core, node, rust, go]
}
