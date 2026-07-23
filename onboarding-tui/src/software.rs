//! The software registry — THE one place to add or remove tools.
//!
//! Each entry is a [`Software`] with:
//!   - `check`: a read-only shell command that succeeds (exit 0) and prints a
//!     version/detail line iff the tool is already installed
//!   - `install`: idempotent, NON-interactive shell steps (per platform)
//!   - `follow_up`: commands the user must run themselves afterwards
//!     (interactive logins, shell-rc reloads, ...)
//!
//! To add a tool: append one `Software` to the Vec in [`registry`].
//! To remove one: delete its block. Nothing else needs to change — the UI,
//! scanner, and installer all iterate over this list.

use crate::detect::{Platform, PkgManager};

pub struct Step {
    pub title: &'static str,
    pub cmd: String,
}

pub struct Software {
    pub name: &'static str,
    pub description: &'static str,
    pub check: String,
    pub install: Vec<Step>,
    pub follow_up: Vec<&'static str>,
}

/// `command -v <probe> || <brew|apt install>` for simple package-manager tools.
fn pkg_install(p: &Platform, brew: &str, apt: &str, probe: &str) -> String {
    match p.pkg {
        PkgManager::Brew => {
            format!("command -v {probe} >/dev/null 2>&1 || brew install {brew}")
        }
        PkgManager::Apt => format!(
            "command -v {probe} >/dev/null 2>&1 || {{ sudo apt-get update -y && sudo apt-get install -y {apt}; }}"
        ),
    }
}

pub fn registry(p: &Platform) -> Vec<Software> {
    let mut list = Vec::new();

    // -- Homebrew (macOS only) ----------------------------------------------
    if p.pkg == PkgManager::Brew {
        list.push(Software {
            name: "Homebrew",
            description: "macOS package manager — everything else installs through it",
            check: "brew --version | head -1".into(),
            install: vec![Step {
                title: "Install Homebrew (non-interactive)",
                cmd: r#"command -v brew >/dev/null 2>&1 || { curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh -o /tmp/brew-install.sh && NONINTERACTIVE=1 /bin/bash /tmp/brew-install.sh; }"#.into(),
            }],
            follow_up: vec![r#"eval "$(/opt/homebrew/bin/brew shellenv)"   # then reopen your terminal"#],
        });
    }

    // -- git ----------------------------------------------------------------
    list.push(Software {
        name: "git",
        description: "version control — required for all lab projects",
        check: "git --version".into(),
        install: vec![Step { title: "Install git", cmd: pkg_install(p, "git", "git", "git") }],
        follow_up: vec![
            "git config --global user.name \"Your Name\"",
            "git config --global user.email \"you@auburn.edu\"",
        ],
    });

    // -- GitHub CLI ---------------------------------------------------------
    list.push(Software {
        name: "GitHub CLI",
        description: "gh — log in to GitHub and clone lab repos",
        check: "gh --version | head -1".into(),
        install: vec![Step { title: "Install GitHub CLI", cmd: pkg_install(p, "gh", "gh", "gh") }],
        follow_up: vec!["gh auth login --git-protocol ssh   # interactive browser login"],
    });

    // -- SSH key ------------------------------------------------------------
    list.push(Software {
        name: "SSH key",
        description: "ed25519 keypair for authenticating with GitHub",
        check: r#"test -f "$HOME/.ssh/id_ed25519.pub" && echo "present ($HOME/.ssh/id_ed25519.pub)""#.into(),
        install: vec![Step {
            title: "Generate SSH key",
            cmd: r#"[ -f "$HOME/.ssh/id_ed25519" ] || { mkdir -p "$HOME/.ssh" && chmod 700 "$HOME/.ssh" && ssh-keygen -t ed25519 -C "$(git config --global user.email 2>/dev/null || echo rfid-lab)" -f "$HOME/.ssh/id_ed25519" -N ""; }"#.into(),
        }],
        follow_up: vec!["gh ssh-key add ~/.ssh/id_ed25519.pub   # after gh auth login"],
    });

    // -- fnm + Node LTS + pnpm ---------------------------------------------
    list.push(Software {
        name: "Node.js (fnm + pnpm)",
        description: "Node LTS via fnm, pnpm via corepack — for React/TS apps",
        check: r#"export PATH="$HOME/.local/share/fnm:$PATH"; eval "$(fnm env 2>/dev/null)"; node --version"#.into(),
        install: vec![
            Step {
                title: "Install fnm",
                cmd: match p.pkg {
                    PkgManager::Brew => "command -v fnm >/dev/null 2>&1 || brew install fnm".into(),
                    PkgManager::Apt => r#"command -v fnm >/dev/null 2>&1 || [ -x "$HOME/.local/share/fnm/fnm" ] || { curl -fsSL https://fnm.vercel.app/install -o /tmp/fnm-install.sh && bash /tmp/fnm-install.sh --skip-shell; }"#.into(),
                },
            },
            Step {
                title: "Install Node LTS + pnpm",
                cmd: r#"export PATH="$HOME/.local/share/fnm:$PATH"; eval "$(fnm env)"; fnm install --lts && fnm default lts-latest; eval "$(fnm env)"; corepack enable pnpm"#.into(),
            },
        ],
        follow_up: vec![r#"echo 'eval "$(fnm env --use-on-cd)"' >> ~/.zshrc   # node in new shells"#],
    });

    // -- Rust ---------------------------------------------------------------
    list.push(Software {
        name: "Rust",
        description: "rustc + cargo via rustup",
        check: r#". "$HOME/.cargo/env" 2>/dev/null; rustc --version"#.into(),
        install: vec![Step {
            title: "Install rustup toolchain",
            cmd: r#"[ -x "$HOME/.cargo/bin/rustc" ] || { curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o /tmp/rustup-init.sh && sh /tmp/rustup-init.sh -y --no-modify-path; }"#.into(),
        }],
        follow_up: vec![r#"echo '. "$HOME/.cargo/env"' >> ~/.zshrc"#],
    });

    // -- Go -----------------------------------------------------------------
    list.push(Software {
        name: "Go",
        description: "Go toolchain for lab services",
        check: "go version".into(),
        install: vec![Step { title: "Install Go", cmd: pkg_install(p, "go", "golang-go", "go") }],
        follow_up: vec![],
    });

    // ---- Add new software above this line ---------------------------------

    list
}
