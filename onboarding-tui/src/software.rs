//! The software registry — THE one place to add or remove tools.
//!
//! Each entry is a [`Software`] with:
//!   - `section`: which part of the scan screen it appears under —
//!     `Section::Ai` (optional, multi-select) or `Section::Required`
//!     (always installed when missing, cannot be deselected)
//!   - `check`: a read-only shell command that succeeds (exit 0) and prints a
//!     version/detail line iff the tool is already installed
//!   - `install`: idempotent, NON-interactive shell steps (per platform)
//!   - `follow_up`: commands the user must run themselves afterwards
//!     (interactive logins, shell-rc reloads, ...)
//!
//! To add a tool: append one `Software` to the Vec in [`registry`].
//! To remove one: delete its block. Nothing else needs to change — the UI,
//! scanner, and installer all iterate over this list. Items install in
//! registry order, so put dependencies (e.g. Node.js before pnpm) first.

use crate::detect::{Platform, PkgManager};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Editors,
    Ai,
    Required,
}

impl Section {
    pub fn title(&self) -> &'static str {
        match self {
            Section::Editors => " Code Editors — pick at least one ",
            Section::Ai => " AI Tools — optional ",
            Section::Required => " Required ",
        }
    }
}

pub struct Step {
    pub title: &'static str,
    pub cmd: String,
}

pub struct Software {
    pub name: &'static str,
    pub description: &'static str,
    pub section: Section,
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

/// Download an official install script to /tmp and run it (if `probe` is missing).
fn script_install(probe: &str, url: &str) -> String {
    format!(
        "command -v {probe} >/dev/null 2>&1 || {{ curl -fsSL {url} -o /tmp/{probe}-install.sh && bash /tmp/{probe}-install.sh; }}"
    )
}

// One push-block per tool keeps add/remove a single-block edit, and some
// blocks are platform-conditional — clearer than one big vec![] literal.
#[allow(clippy::vec_init_then_push)]
pub fn registry(p: &Platform) -> Vec<Software> {
    let mut list = Vec::new();

    // =======================================================================
    // Code editors (multi-select, at least one required)
    // =======================================================================

    list.push(Software {
        name: "VS Code",
        description: "Visual Studio Code — the most common choice, big extension ecosystem",
        section: Section::Editors,
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
        follow_up: vec![],
    });

    list.push(Software {
        name: "VSCodium",
        description: "VS Code without Microsoft telemetry/branding",
        section: Section::Editors,
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
        follow_up: vec![],
    });

    // Desktop app (macOS cask); WSL/Linux users generally run the Windows/
    // native build outside the terminal — only offered on macOS.
    if p.pkg == PkgManager::Brew {
        list.push(Software {
            name: "Cursor",
            description: "Cursor — AI-first VS Code fork (editor app; CLI is under AI Tools)",
            section: Section::Editors,
            check: r#"test -d "/Applications/Cursor.app" && echo "Cursor installed""#.into(),
            install: vec![Step {
                title: "Install Cursor",
                cmd: r#"test -d "/Applications/Cursor.app" || brew install --cask cursor"#.into(),
            }],
            follow_up: vec![],
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
        follow_up: vec![],
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
        follow_up: vec![],
    });

    list.push(Software {
        name: "Zed",
        description: "Zed — fast collaborative editor by the Atom team",
        section: Section::Editors,
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
        follow_up: vec![],
    });

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
    });

    // =======================================================================
    // Required
    // =======================================================================

    // -- Homebrew (macOS only) ----------------------------------------------
    if p.pkg == PkgManager::Brew {
        list.push(Software {
            name: "Homebrew",
            description: "macOS package manager — everything else installs through it",
            section: Section::Required,
            check: "brew --version | head -1".into(),
            install: vec![Step {
                title: "Install Homebrew (non-interactive)",
                cmd: r#"command -v brew >/dev/null 2>&1 || { curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh -o /tmp/brew-install.sh && NONINTERACTIVE=1 /bin/bash /tmp/brew-install.sh; }"#.into(),
            }],
            follow_up: vec![r#"eval "$(/opt/homebrew/bin/brew shellenv)"   # then reopen your terminal"#],
        });
    }

    // -- GitHub CLI + git ---------------------------------------------------
    list.push(Software {
        name: "GitHub CLI + git",
        description: "git version control and gh for GitHub auth/cloning",
        section: Section::Required,
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
    });

    // -- Linear (desktop app; macOS only) -----------------------------------
    if p.pkg == PkgManager::Brew {
        list.push(Software {
            name: "Linear",
            description: "Linear desktop app — lab issue tracking",
            section: Section::Required,
            check: r#"test -d "/Applications/Linear.app" && echo "Linear.app installed""#.into(),
            install: vec![Step {
                title: "Install Linear",
                cmd: r#"test -d "/Applications/Linear.app" || brew install --cask linear-linear"#.into(),
            }],
            follow_up: vec![],
        });
    }

    // -- Vault --------------------------------------------------------------
    list.push(Software {
        name: "Vault",
        description: "HashiCorp Vault CLI — lab secrets",
        section: Section::Required,
        check: "vault --version".into(),
        install: vec![Step {
            title: "Install Vault",
            cmd: match p.pkg {
                PkgManager::Brew => "command -v vault >/dev/null 2>&1 || brew install hashicorp/tap/vault".into(),
                PkgManager::Apt => r#"command -v vault >/dev/null 2>&1 || { curl -fsSL https://apt.releases.hashicorp.com/gpg | sudo gpg --dearmor -o /usr/share/keyrings/hashicorp-archive-keyring.gpg && echo "deb [signed-by=/usr/share/keyrings/hashicorp-archive-keyring.gpg] https://apt.releases.hashicorp.com $(lsb_release -cs) main" | sudo tee /etc/apt/sources.list.d/hashicorp.list && sudo apt-get update -y && sudo apt-get install -y vault; }"#.into(),
            },
        }],
        follow_up: vec![],
    });

    // -- Go -----------------------------------------------------------------
    list.push(Software {
        name: "Go",
        description: "Go toolchain for lab services",
        section: Section::Required,
        check: "go version".into(),
        install: vec![Step { title: "Install Go", cmd: pkg_install(p, "go", "golang-go", "go") }],
        follow_up: vec![],
    });

    // -- Rust ---------------------------------------------------------------
    list.push(Software {
        name: "Rust",
        description: "rustc + cargo via rustup",
        section: Section::Required,
        check: r#". "$HOME/.cargo/env" 2>/dev/null; rustc --version"#.into(),
        install: vec![Step {
            title: "Install rustup toolchain",
            cmd: r#"[ -x "$HOME/.cargo/bin/rustc" ] || { curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o /tmp/rustup-init.sh && sh /tmp/rustup-init.sh -y --no-modify-path; }"#.into(),
        }],
        follow_up: vec![r#"echo '. "$HOME/.cargo/env"' >> ~/.zshrc"#],
    });

    // -- Node.js (fnm) ------------------------------------------------------
    list.push(Software {
        name: "Node.js",
        description: "Node LTS via fnm — for React/TS apps",
        section: Section::Required,
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
    });

    // -- Bun ----------------------------------------------------------------
    list.push(Software {
        name: "Bun",
        description: "bun — fast JS runtime/bundler used by some lab projects",
        section: Section::Required,
        check: r#"export PATH="$HOME/.bun/bin:$PATH"; bun --version"#.into(),
        install: vec![Step {
            title: "Install Bun",
            cmd: r#"export PATH="$HOME/.bun/bin:$PATH"; command -v bun >/dev/null 2>&1 || { curl -fsSL https://bun.sh/install -o /tmp/bun-install.sh && bash /tmp/bun-install.sh; }"#.into(),
        }],
        follow_up: vec![],
    });

    // -- pnpm ---------------------------------------------------------------
    list.push(Software {
        name: "pnpm",
        description: "pnpm package manager (via corepack — needs Node.js)",
        section: Section::Required,
        check: r#"export PATH="$HOME/.local/share/fnm:$PATH"; eval "$(fnm env 2>/dev/null)"; pnpm --version"#.into(),
        install: vec![Step {
            title: "Enable pnpm via corepack",
            cmd: r#"export PATH="$HOME/.local/share/fnm:$PATH"; eval "$(fnm env)"; corepack enable pnpm"#.into(),
        }],
        follow_up: vec![],
    });

    // ---- Add new software above this line ---------------------------------

    list
}
