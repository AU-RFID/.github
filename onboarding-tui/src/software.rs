//! The software registry — THE one place to add or remove tools.
//!
//! Each entry is a [`Software`] with:
//!   - `section`: which box it appears in — `Editors` (pick at least one),
//!     `Ai` (optional multi-select), or `Required` (locked in when missing)
//!   - `preferred`: shows a ★ preferred tag in the UI
//!   - `location`: where it lives — `Dev` tools go in the dev environment
//!     (local shell on macOS/Linux, INSIDE the chosen WSL distro when the TUI
//!     runs on a Windows host); `Host` apps are GUI programs that must land on
//!     the host system (installed via winget on Windows, brew casks on macOS)
//!   - `winget_id`: the winget package id used when installing a `Host` app
//!     from a Windows host
//!   - `check`: a read-only command that succeeds (exit 0) and prints a
//!     version/detail line iff the tool is already installed
//!   - `install`: idempotent, NON-interactive steps (per platform)
//!   - `follow_up`: commands the user must run themselves afterwards
//!
//! To add a tool: append one `Software` to the Vec in [`registry`] — set only
//! the fields you need and end with `..Software::default()`. To remove one:
//! delete its block. The UI, scanner, and installer all iterate this list.
//! Items install in registry order, so put dependencies (Node before pnpm)
//! first. A GUI app that must live on the Windows host (e.g. Yaak) is just
//! `location: Location::Host` plus its `winget_id`.

use crate::detect::{Platform, PkgManager};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Editors,
    Containers,
    Ai,
    Required,
}

impl Section {
    pub fn title(&self) -> &'static str {
        match self {
            Section::Editors => " Code Editors — pick at least one ",
            Section::Containers => " Docker / Kubernetes — optional ",
            Section::Ai => " AI Tools — optional ",
            Section::Required => " Required ",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Location {
    /// Dev-environment tool: local shell, or inside the WSL distro on Windows.
    Dev,
    /// Host GUI app: winget on a Windows host, cask on macOS.
    Host,
}

pub struct Step {
    pub title: &'static str,
    pub cmd: String,
}

pub struct Software {
    pub name: &'static str,
    pub description: &'static str,
    pub section: Section,
    pub preferred: bool,
    pub location: Location,
    pub winget_id: Option<&'static str>,
    pub check: String,
    pub install: Vec<Step>,
    pub follow_up: Vec<&'static str>,
}

impl Default for Software {
    fn default() -> Self {
        Software {
            name: "",
            description: "",
            section: Section::Required,
            preferred: false,
            location: Location::Dev,
            winget_id: None,
            check: String::new(),
            install: Vec::new(),
            follow_up: Vec::new(),
        }
    }
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

/// Download an official install script to /tmp and run it (if `probe` is missing).
fn script_install(probe: &str, url: &str) -> String {
    format!(
        "command -v {probe} >/dev/null 2>&1 || {{ curl -fsSL {url} -o /tmp/{probe}-install.sh && bash /tmp/{probe}-install.sh; }}"
    )
}

/// Linux arch string used by most GitHub release assets.
fn linux_arch(p: &Platform) -> &'static str {
    match p.arch {
        "aarch64" => "arm64",
        _ => "amd64",
    }
}

/// Install a single static binary from a GitHub `latest/download` tarball that
/// contains a file named `probe` (if `probe` is missing).
fn github_bin_install(probe: &str, repo: &str, asset: &str) -> String {
    format!(
        "command -v {probe} >/dev/null 2>&1 || {{ curl -fsSL https://github.com/{repo}/releases/latest/download/{asset} -o /tmp/{probe}.tgz && tar -xzf /tmp/{probe}.tgz -C /tmp {probe} && sudo install /tmp/{probe} /usr/local/bin/{probe}; }}"
    )
}

// One push-block per tool keeps add/remove a single-block edit, and some
// blocks are platform-conditional — clearer than one big vec![] literal.
#[allow(clippy::vec_init_then_push)]
pub fn registry(p: &Platform) -> Vec<Software> {
    let mut list = Vec::new();
    let windows_host = p.windows_host();

    // =======================================================================
    // Code editors (multi-select, at least one required)
    // =======================================================================

    list.push(Software {
        name: "VS Code",
        description: "Visual Studio Code — the most common choice, big extension ecosystem",
        section: Section::Editors,
        location: Location::Host,
        winget_id: Some("Microsoft.VisualStudioCode"),
        check: match p.pkg {
            PkgManager::Brew => r#"test -d "/Applications/Visual Studio Code.app" && echo "VS Code installed" || code --version | head -1"#.into(),
            PkgManager::Apt => "code --version | head -1".into(),
        },
        install: vec![Step {
            title: "Install VS Code",
            cmd: match p.pkg {
                PkgManager::Brew => r#"test -d "/Applications/Visual Studio Code.app" || brew install --cask visual-studio-code"#.into(),
                PkgManager::Apt => r#"command -v code >/dev/null 2>&1 || { curl -fsSL https://packages.microsoft.com/keys/microsoft.asc | sudo gpg --dearmor -o /usr/share/keyrings/ms-vscode-keyring.gpg && echo "deb [signed-by=/usr/share/keyrings/ms-vscode-keyring.gpg] https://packages.microsoft.com/repos/code stable main" | sudo tee /etc/apt/sources.list.d/vscode.list && sudo apt-get update -y && sudo apt-get install -y code; }"#.into(),
            },
        }],
        ..Software::default()
    });

    list.push(Software {
        name: "VSCodium",
        description: "VS Code without Microsoft telemetry/branding",
        section: Section::Editors,
        location: Location::Host,
        winget_id: Some("VSCodium.VSCodium"),
        check: match p.pkg {
            PkgManager::Brew => r#"test -d "/Applications/VSCodium.app" && echo "VSCodium installed" || codium --version | head -1"#.into(),
            PkgManager::Apt => "codium --version | head -1".into(),
        },
        install: vec![Step {
            title: "Install VSCodium",
            cmd: match p.pkg {
                PkgManager::Brew => r#"test -d "/Applications/VSCodium.app" || brew install --cask vscodium"#.into(),
                PkgManager::Apt => r#"command -v codium >/dev/null 2>&1 || { curl -fsSL https://gitlab.com/paulcarroty/vscodium-deb-rpm-repo/raw/master/pub.gpg | sudo gpg --dearmor -o /usr/share/keyrings/vscodium-archive-keyring.gpg && echo "deb [signed-by=/usr/share/keyrings/vscodium-archive-keyring.gpg] https://download.vscodium.com/debs vscodium main" | sudo tee /etc/apt/sources.list.d/vscodium.list && sudo apt-get update -y && sudo apt-get install -y codium; }"#.into(),
            },
        }],
        ..Software::default()
    });

    // Desktop app: macOS cask or Windows-host winget (no Linux build offered).
    if p.pkg == PkgManager::Brew || windows_host {
        list.push(Software {
            name: "Cursor",
            description: "Cursor — AI-first VS Code fork (editor app; CLI is under AI Tools)",
            section: Section::Editors,
            preferred: true,
            location: Location::Host,
            winget_id: Some("Anysphere.Cursor"),
            check: r#"test -d "/Applications/Cursor.app" && echo "Cursor installed""#.into(),
            install: vec![Step {
                title: "Install Cursor",
                cmd: r#"test -d "/Applications/Cursor.app" || brew install --cask cursor"#.into(),
            }],
            ..Software::default()
        });
    }

    list.push(Software {
        name: "Neovim",
        description: "nvim — modal terminal editor",
        section: Section::Editors,
        check: "nvim --version | head -1".into(),
        install: vec![Step {
            title: "Install Neovim",
            cmd: pkg_install(p, "neovim", "neovim", "nvim"),
        }],
        ..Software::default()
    });

    list.push(Software {
        name: "Helix",
        description: "hx — modern modal terminal editor, batteries included",
        section: Section::Editors,
        check: "hx --version | head -1".into(),
        install: vec![Step {
            title: "Install Helix",
            cmd: match p.pkg {
                PkgManager::Brew => "command -v hx >/dev/null 2>&1 || brew install helix".into(),
                PkgManager::Apt => r#"command -v hx >/dev/null 2>&1 || { sudo apt-get install -y software-properties-common && sudo add-apt-repository -y ppa:maveonair/helix-editor && sudo apt-get update -y && sudo apt-get install -y helix; }"#.into(),
            },
        }],
        ..Software::default()
    });

    list.push(Software {
        name: "Zed",
        description: "Zed — fast collaborative editor by the Atom team",
        section: Section::Editors,
        preferred: true,
        location: Location::Host,
        winget_id: Some("ZedIndustries.Zed"),
        check: match p.pkg {
            PkgManager::Brew => r#"test -d "/Applications/Zed.app" && echo "Zed installed" || zed --version | head -1"#.into(),
            PkgManager::Apt => r#"export PATH="$HOME/.local/bin:$PATH"; zed --version | head -1"#.into(),
        },
        install: vec![Step {
            title: "Install Zed",
            cmd: match p.pkg {
                PkgManager::Brew => r#"test -d "/Applications/Zed.app" || brew install --cask zed"#.into(),
                PkgManager::Apt => script_install("zed", "https://zed.dev/install.sh"),
            },
        }],
        ..Software::default()
    });

    // =======================================================================
    // Docker / Kubernetes (optional, multi-select — GUIs and TUIs)
    //
    // OrbStack (which bundles the Docker daemon) is required on macOS and
    // lives in the Required section below.
    // =======================================================================

    // k9s — Kubernetes TUI (cross-platform).
    list.push(Software {
        name: "k9s",
        description: "k9s — terminal UI for Kubernetes clusters",
        section: Section::Containers,
        preferred: true,
        check: "k9s version 2>/dev/null | head -1".into(),
        install: vec![Step {
            title: "Install k9s",
            cmd: match p.pkg {
                PkgManager::Brew => "command -v k9s >/dev/null 2>&1 || brew install k9s".into(),
                PkgManager::Apt => {
                    github_bin_install("k9s", "derailed/k9s", &format!("k9s_Linux_{}.tar.gz", linux_arch(p)))
                }
            },
        }],
        ..Software::default()
    });

    // lazydocker — Docker TUI (cross-platform).
    list.push(Software {
        name: "lazydocker",
        description: "lazydocker — terminal UI for Docker and docker-compose",
        section: Section::Containers,
        check: r#"export PATH="$HOME/.local/bin:$PATH"; lazydocker --version | head -1"#.into(),
        install: vec![Step {
            title: "Install lazydocker",
            cmd: match p.pkg {
                PkgManager::Brew => "command -v lazydocker >/dev/null 2>&1 || brew install lazydocker".into(),
                PkgManager::Apt => script_install(
                    "lazydocker",
                    "https://raw.githubusercontent.com/jesseduffield/lazydocker/master/scripts/install_update_linux.sh",
                ),
            },
        }],
        ..Software::default()
    });

    // Lens — Kubernetes GUI (macOS + Windows host; GUI app).
    if p.pkg == PkgManager::Brew || windows_host {
        list.push(Software {
            name: "Lens",
            description: "Lens Desktop — GUI for managing Kubernetes clusters",
            section: Section::Containers,
            location: Location::Host,
            winget_id: Some("Mirantis.Lens"),
            check: r#"test -d "/Applications/Lens.app" && echo "Lens installed""#.into(),
            install: vec![Step {
                title: "Install Lens",
                cmd: r#"test -d "/Applications/Lens.app" || brew install --cask lens"#.into(),
            }],
            ..Software::default()
        });
    }

    // =======================================================================
    // AI tools (optional, multi-select)
    // =======================================================================

    list.push(Software {
        name: "Cursor CLI",
        description: "cursor-agent — Cursor's terminal coding agent",
        section: Section::Ai,
        check: r#"export PATH="$HOME/.local/bin:$PATH"; cursor-agent --version"#.into(),
        install: vec![Step {
            title: "Install Cursor CLI",
            cmd: script_install("cursor-agent", "https://cursor.com/install"),
        }],
        follow_up: vec!["cursor-agent login"],
        ..Software::default()
    });

    list.push(Software {
        name: "OpenCode",
        description: "opencode — open-source terminal coding agent by SST",
        section: Section::Ai,
        check: r#"export PATH="$HOME/.opencode/bin:$PATH"; opencode --version"#.into(),
        install: vec![Step {
            title: "Install OpenCode",
            cmd: match p.pkg {
                PkgManager::Brew => "command -v opencode >/dev/null 2>&1 || brew install sst/tap/opencode".into(),
                PkgManager::Apt => script_install("opencode", "https://opencode.ai/install"),
            },
        }],
        follow_up: vec!["opencode auth login"],
        ..Software::default()
    });

    list.push(Software {
        name: "Claude Code",
        description: "claude — Anthropic's terminal coding agent",
        section: Section::Ai,
        check: r#"export PATH="$HOME/.local/bin:$PATH"; claude --version"#.into(),
        install: vec![Step {
            title: "Install Claude Code",
            cmd: script_install("claude", "https://claude.ai/install.sh"),
        }],
        follow_up: vec!["claude   # log in on first run"],
        ..Software::default()
    });

    list.push(Software {
        name: "Codex",
        description: "codex — OpenAI's terminal coding agent (needs Node.js on Linux)",
        section: Section::Ai,
        check: r#"export PATH="$HOME/.local/share/fnm:$PATH"; eval "$(fnm env 2>/dev/null)"; codex --version"#.into(),
        install: vec![Step {
            title: "Install Codex",
            cmd: match p.pkg {
                PkgManager::Brew => "command -v codex >/dev/null 2>&1 || brew install codex".into(),
                PkgManager::Apt => r#"export PATH="$HOME/.local/share/fnm:$PATH"; eval "$(fnm env)"; command -v codex >/dev/null 2>&1 || npm install -g @openai/codex"#.into(),
            },
        }],
        follow_up: vec!["codex login"],
        ..Software::default()
    });

    // =======================================================================
    // Required
    // =======================================================================

    // -- Homebrew (macOS only) ----------------------------------------------
    if p.pkg == PkgManager::Brew && !windows_host {
        list.push(Software {
            name: "Homebrew",
            description: "macOS package manager — everything else installs through it",
            check: "brew --version | head -1".into(),
            install: vec![Step {
                title: "Install Homebrew (non-interactive)",
                cmd: r#"command -v brew >/dev/null 2>&1 || { curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh -o /tmp/brew-install.sh && NONINTERACTIVE=1 /bin/bash /tmp/brew-install.sh; }"#.into(),
            }],
            follow_up: vec![r#"eval "$(/opt/homebrew/bin/brew shellenv)"   # then reopen your terminal"#],
            ..Software::default()
        });
    }

    // -- OrbStack (macOS only; bundles the Docker daemon + Kubernetes) ------
    if p.pkg == PkgManager::Brew && !windows_host {
        list.push(Software {
            name: "OrbStack",
            description: "OrbStack — Docker daemon + Kubernetes for macOS (replaces Docker Desktop)",
            preferred: true,
            location: Location::Host,
            check: r#"test -d "/Applications/OrbStack.app" && echo "OrbStack installed""#.into(),
            install: vec![Step {
                title: "Install OrbStack",
                cmd: r#"test -d "/Applications/OrbStack.app" || brew install --cask orbstack"#.into(),
            }],
            follow_up: vec!["Open OrbStack once to start the Docker daemon"],
            ..Software::default()
        });
    }

    // -- GitHub CLI + git ---------------------------------------------------
    list.push(Software {
        name: "GitHub CLI + git",
        description: "git version control and gh for GitHub auth/cloning",
        check: r#"git --version && gh --version | head -1"#.into(),
        install: vec![
            Step { title: "Install git", cmd: pkg_install(p, "git", "git", "git") },
            Step { title: "Install GitHub CLI", cmd: pkg_install(p, "gh", "gh", "gh") },
        ],
        follow_up: vec![
            "git config --global user.name \"Your Name\"",
            "git config --global user.email \"you@auburn.edu\"",
            "gh auth login --git-protocol ssh   # interactive browser login",
        ],
        ..Software::default()
    });

    // -- Linear (desktop app; macOS + Windows host) --------------------------
    if p.pkg == PkgManager::Brew || windows_host {
        list.push(Software {
            name: "Linear",
            description: "Linear desktop app — lab issue tracking",
            location: Location::Host,
            winget_id: Some("Linear.Linear"),
            check: r#"test -d "/Applications/Linear.app" && echo "Linear.app installed""#.into(),
            install: vec![Step {
                title: "Install Linear",
                cmd: r#"test -d "/Applications/Linear.app" || brew install --cask linear-linear"#.into(),
            }],
            ..Software::default()
        });
    }

    // -- Vault --------------------------------------------------------------
    list.push(Software {
        name: "Vault",
        description: "HashiCorp Vault CLI — lab secrets",
        check: "vault --version".into(),
        install: vec![Step {
            title: "Install Vault",
            cmd: match p.pkg {
                PkgManager::Brew => "command -v vault >/dev/null 2>&1 || brew install hashicorp/tap/vault".into(),
                PkgManager::Apt => r#"command -v vault >/dev/null 2>&1 || { curl -fsSL https://apt.releases.hashicorp.com/gpg | sudo gpg --dearmor -o /usr/share/keyrings/hashicorp-archive-keyring.gpg && echo "deb [signed-by=/usr/share/keyrings/hashicorp-archive-keyring.gpg] https://apt.releases.hashicorp.com $(lsb_release -cs) main" | sudo tee /etc/apt/sources.list.d/hashicorp.list && sudo apt-get update -y && sudo apt-get install -y vault; }"#.into(),
            },
        }],
        ..Software::default()
    });

    // -- Go -----------------------------------------------------------------
    list.push(Software {
        name: "Go",
        description: "Go toolchain for lab services",
        check: "go version".into(),
        install: vec![Step { title: "Install Go", cmd: pkg_install(p, "go", "golang-go", "go") }],
        ..Software::default()
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
        ..Software::default()
    });

    // -- Node.js (fnm) ------------------------------------------------------
    list.push(Software {
        name: "Node.js",
        description: "Node LTS via fnm — for React/TS apps",
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
                title: "Install Node LTS",
                cmd: r#"export PATH="$HOME/.local/share/fnm:$PATH"; eval "$(fnm env)"; fnm install --lts && fnm default lts-latest"#.into(),
            },
        ],
        follow_up: vec![r#"echo 'eval "$(fnm env --use-on-cd)"' >> ~/.zshrc   # node in new shells"#],
        ..Software::default()
    });

    // -- Bun ----------------------------------------------------------------
    list.push(Software {
        name: "Bun",
        description: "bun — fast JS runtime/bundler used by some lab projects",
        check: r#"export PATH="$HOME/.bun/bin:$PATH"; bun --version"#.into(),
        install: vec![Step {
            title: "Install Bun",
            cmd: r#"export PATH="$HOME/.bun/bin:$PATH"; command -v bun >/dev/null 2>&1 || { curl -fsSL https://bun.sh/install -o /tmp/bun-install.sh && bash /tmp/bun-install.sh; }"#.into(),
        }],
        ..Software::default()
    });

    // -- pnpm ---------------------------------------------------------------
    list.push(Software {
        name: "pnpm",
        description: "pnpm package manager (via corepack — needs Node.js)",
        check: r#"export PATH="$HOME/.local/share/fnm:$PATH"; eval "$(fnm env 2>/dev/null)"; pnpm --version"#.into(),
        install: vec![Step {
            title: "Enable pnpm via corepack",
            cmd: r#"export PATH="$HOME/.local/share/fnm:$PATH"; eval "$(fnm env)"; corepack enable pnpm"#.into(),
        }],
        ..Software::default()
    });

    // ---- Add new software above this line ---------------------------------
    // Host GUI apps (e.g. Yaak) → location: Location::Host + winget_id.

    // On a Windows host, Host apps check/install through winget instead of
    // the brew/apt commands written above.
    if windows_host {
        for sw in &mut list {
            if sw.location == Location::Host {
                if let Some(id) = sw.winget_id {
                    sw.check = format!("winget list -e --id {id}");
                    sw.install = vec![Step {
                        title: "Install via winget",
                        cmd: format!(
                            "winget install -e --id {id} --accept-package-agreements --accept-source-agreements"
                        ),
                    }];
                }
            }
        }
    }

    list
}
