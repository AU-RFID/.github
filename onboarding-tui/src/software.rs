//! Loads and resolves the software registry from `software.json`.
//!
//! The data lives in `software.json` (compiled in via `include_str!`, so the
//! binary stays self-contained). This module parses it and resolves each entry
//! against the detected [`Env`] into the runtime [`Software`] the UI consumes:
//!
//!   - `cli` tools install everywhere (and inside WSL from a Windows host)
//!   - `gui` apps install on macOS / Linux-desktop / Windows-host, and are
//!     skipped on Linux servers
//!   - a tool is dropped when no applicable command block exists for the target
//!
//! To change what gets installed, edit `software.json` — not this file.

use serde::Deserialize;

use crate::detect::{Env, Platform};

// ---------------------------------------------------------------------------
// Runtime model (what the UI uses)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rule {
    PickOne,
    Optional,
    Required,
}

pub struct SectionDef {
    pub title: String,
    pub rule: Rule,
    /// Collapsible sections start collapsed (just a header) and can be
    /// expanded — for tooling not everyone needs (e.g. Kubernetes).
    pub collapsible: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Location {
    /// Dev tool: local shell, or inside the WSL distro from a Windows host.
    Dev,
    /// GUI app: on the host (winget on Windows, cask/desktop install elsewhere).
    Host,
}

pub struct Step {
    pub title: String,
    pub cmd: String,
}

pub struct Software {
    pub name: String,
    pub description: String,
    pub section: usize, // index into the sections vec
    pub preferred: bool,
    pub location: Location,
    pub check: String,
    pub install: Vec<Step>,
    pub follow_up: Vec<String>,
}

// ---------------------------------------------------------------------------
// JSON shapes (serde ignores unknown keys, e.g. the "_help" block)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawSection {
    id: String,
    title: String,
    rule: String,
    #[serde(default)]
    collapsed: bool,
}

#[derive(Deserialize)]
struct RawStep {
    title: Option<String>,
    cmd: String,
}

#[derive(Deserialize)]
struct RawBlock {
    check: Option<String>,
    install: Vec<RawStep>,
}

#[derive(Deserialize)]
struct RawSoftware {
    name: String,
    description: String,
    section: String,
    kind: String, // "gui" | "cli"
    #[serde(default)]
    preferred: bool,
    #[serde(default)]
    check: Option<String>,
    #[serde(default)]
    follow_up: Vec<String>,
    #[serde(default)]
    winget_id: Option<String>,
    #[serde(default)]
    macos: Option<RawBlock>,
    #[serde(default)]
    linux: Option<RawBlock>,
    #[serde(default)]
    any: Option<RawBlock>,
}

#[derive(Deserialize)]
struct RawData {
    sections: Vec<RawSection>,
    software: Vec<RawSoftware>,
}

fn parse_rule(s: &str) -> Rule {
    match s {
        "pick-one" => Rule::PickOne,
        "required" => Rule::Required,
        _ => Rule::Optional,
    }
}

/// Parse `software.json` and resolve it for this platform. Returns the section
/// definitions (in display order) and the applicable software list.
pub fn load(platform: &Platform) -> (Vec<SectionDef>, Vec<Software>) {
    let data: RawData = serde_json::from_str(include_str!("../software.json"))
        .expect("software.json is malformed — check the JSON syntax");

    let sections: Vec<SectionDef> = data
        .sections
        .iter()
        .map(|s| SectionDef {
            title: s.title.clone(),
            rule: parse_rule(&s.rule),
            collapsible: s.collapsed,
        })
        .collect();
    let section_index = |id: &str| data.sections.iter().position(|s| s.id == id);

    let mut items = Vec::new();
    for raw in &data.software {
        let Some(section) = section_index(&raw.section) else {
            continue; // unknown section id — skip rather than crash
        };
        let gui = raw.kind == "gui";
        if let Some(sw) = resolve(raw, section, gui, platform) {
            items.push(sw);
        }
    }
    (sections, items)
}

/// Turn a JSON entry into a runtime [`Software`] for this env, or `None` if it
/// doesn't apply here (e.g. a GUI app on a Linux server, or a tool with no
/// command block for this OS).
fn resolve(raw: &RawSoftware, section: usize, gui: bool, platform: &Platform) -> Option<Software> {
    let location = if gui { Location::Host } else { Location::Dev };

    let (check, install): (String, Vec<Step>) = match platform.env {
        // Windows host: GUI apps go through winget; CLI tools run in WSL using
        // the linux block.
        Env::WindowsHost => {
            if gui {
                let id = raw.winget_id.as_deref()?;
                (
                    format!("winget list -e --id {id}"),
                    vec![Step {
                        title: format!("Install {} via winget", raw.name),
                        cmd: format!(
                            "winget install -e --id {id} --accept-package-agreements --accept-source-agreements"
                        ),
                    }],
                )
            } else {
                block_commands(raw, os_block(raw, Env::LinuxDesktop))?
            }
        }
        // Linux server: no GUI apps.
        Env::LinuxServer if gui => return None,
        // Everyone else: use the matching OS block (or `any`).
        env => block_commands(raw, os_block(raw, env))?,
    };

    Some(Software {
        name: raw.name.clone(),
        description: raw.description.clone(),
        section,
        preferred: raw.preferred,
        location,
        check,
        install,
        follow_up: raw.follow_up.clone(),
    })
}

/// The command block that applies to `env`: the OS-specific one, else `any`.
fn os_block(raw: &RawSoftware, env: Env) -> Option<&RawBlock> {
    let os_specific = if env.is_linux() { &raw.linux } else { &raw.macos };
    os_specific.as_ref().or(raw.any.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::Platform;

    fn platform(env: Env) -> Platform {
        Platform { arch: "x86_64", wsl: false, env, wsl_distro: None }
    }

    fn names(env: Env) -> Vec<String> {
        load(&platform(env)).1.into_iter().map(|s| s.name).collect()
    }

    #[test]
    fn json_parses_and_all_envs_resolve() {
        for env in [Env::Macos, Env::LinuxDesktop, Env::LinuxServer, Env::WindowsHost] {
            assert!(!load(&platform(env)).0.is_empty(), "sections missing");
            assert!(!names(env).is_empty(), "no software resolved");
        }
    }

    #[test]
    fn linux_server_excludes_gui_apps() {
        let server = names(Env::LinuxServer);
        // GUI-only apps must not appear on a headless server...
        for gui in ["VS Code", "Lens", "Yaak", "DBeaver", "Tower", "OrbStack", "1Password", "Bitwarden"] {
            assert!(!server.contains(&gui.to_string()), "{gui} leaked onto server");
        }
        // ...but CLI/TUI tools must.
        for cli in ["Neovim", "k9s", "lazygit", "Rust", "Node.js", "Tailscale"] {
            assert!(server.contains(&cli.to_string()), "{cli} missing on server");
        }
    }

    #[test]
    fn linux_desktop_includes_supported_gui() {
        let desktop = names(Env::LinuxDesktop);
        assert!(desktop.contains(&"VS Code".to_string()));
        assert!(desktop.contains(&"Zed".to_string()));
    }

    #[test]
    fn kubernetes_section_is_collapsible_and_teams_required() {
        let (sections, items) = load(&platform(Env::Macos));
        // Exactly the Kubernetes section is collapsible.
        let collapsible: Vec<&SectionDef> = sections.iter().filter(|s| s.collapsible).collect();
        assert_eq!(collapsible.len(), 1);
        assert!(collapsible[0].title.contains("Kubernetes"));
        // Microsoft Teams and Tailscale are present and live in a Required section.
        for name in ["Microsoft Teams", "Tailscale"] {
            let sw = items.iter().find(|s| s.name == name).unwrap_or_else(|| panic!("{name} missing"));
            assert_eq!(sections[sw.section].rule, Rule::Required, "{name} should be required");
        }
        // 1Password is preferred within the Password Managers section.
        let onepw = items.iter().find(|s| s.name == "1Password").expect("1Password missing");
        assert!(onepw.preferred);
        assert!(sections[onepw.section].title.contains("Password Managers"));
    }

    #[test]
    fn every_resolved_tool_has_a_check_and_install() {
        for env in [Env::Macos, Env::LinuxDesktop, Env::LinuxServer, Env::WindowsHost] {
            for sw in load(&platform(env)).1 {
                assert!(!sw.check.trim().is_empty(), "{} has empty check on {:?}", sw.name, env as u8);
                assert!(!sw.install.is_empty(), "{} has no install steps", sw.name);
                assert!(sw.install.iter().all(|s| !s.cmd.trim().is_empty()));
            }
        }
    }
}

/// Resolve a block into (check, steps), applying the default check and default
/// step titles. `None` when there's no block (tool not available here).
fn block_commands(raw: &RawSoftware, block: Option<&RawBlock>) -> Option<(String, Vec<Step>)> {
    let block = block?;
    let check = block
        .check
        .clone()
        .or_else(|| raw.check.clone())
        .unwrap_or_default();
    let install = block
        .install
        .iter()
        .map(|s| Step {
            title: s.title.clone().unwrap_or_else(|| format!("Install {}", raw.name)),
            cmd: s.cmd.clone(),
        })
        .collect();
    Some((check, install))
}
