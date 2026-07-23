# Onboarding TUI PoC comparison notes

> **Decision (2026-07-23): going with the Rust/ratatui PoC.** Auburn-themed
> welcome screen, automatic installed-vs-wanted scan, sectioned Ubuntu-style
> checklist, Windows-host + WSL support, and a Linux desktop/server split.

## Editing the software list

All tools live in **`software.json`** — no Rust needed. Each entry has a
`section`, a `kind` (`gui` or `cli`), per-OS command blocks (`macos` / `linux`
/ `any`), a `check`, `install` steps, and optional `winget_id` / `follow_up`.
`src/software.rs` loads and resolves it for the detected environment:

| Environment | GUI apps | CLI/TUI tools |
|---|---|---|
| macOS | brew casks | brew formulae |
| Linux **desktop** | `linux` block (skipped if none) | `linux` block |
| Linux **server** | **excluded** | `linux` block |
| Windows host | winget on the host | run inside the chosen WSL distro |

Force a Linux mode for testing with `RFID_ONBOARD_LINUX=desktop|server`.
`cargo test` verifies parsing + that GUI apps never leak onto a server.

## Needs verification on real hosts

- winget IDs / casks flagged as best-effort: **Yaak**, **Tower**, Lens,
  MySQL Workbench, DataGrip (confirm with `winget search` / `brew info`).
- Several GUI apps are macOS+Windows only (no `linux` block yet): Cursor,
  Lens, Linear, Yaak, Bruno, the DB tools, Tower, GitHub Desktop. Add a
  `linux` block (apt repo / flatpak / script) to offer them on Linux desktop.


Two PoCs with the same feature set (platform detection, full/pick/doctor modes,
idempotent installs of core+node+rust+go):

| | PoC 1: bash + gum | PoC 2: Rust + ratatui |
|---|---|---|
| Entry point | `public-scripts/onboard-gum.sh` (self-contained) | `public-scripts/onboard-rust.sh` → prebuilt binary from GitHub Releases |
| Runtime deps | downloads pinned `gum` v0.17.0 (~5 MB); degrades to plain prompts if that fails (`RFID_ONBOARD_PLAIN=1` forces it) | none at runtime (static musl / native macOS binary) |
| Build/release pipeline | none | `.github/workflows/release-onboarding-tui.yml`, tag `onboarding-tui-v*` |
| Interactive steps (Homebrew installer, `gh auth login`, git identity prompts) | handled inline — script hands the terminal over | **not** handled inside the TUI (alternate screen conflicts); printed as follow-up commands on the Doctor screen |
| UI | sequential prompts/spinners | full-screen: step list + live streaming output pane, doctor summary |
| Maintainability | one bash file, matches repo conventions, anyone on the team can patch it | ~500 lines Rust across `main.rs`/`tasks.rs`; richer but needs Rust knowledge + a release for every change |

## Findings so far (fill in as we test)

- [x] gum v0.17.0 release URL + tarball layout verified on macOS arm64
- [x] PoC 1 doctor mode runs end-to-end (plain fallback UI) on macOS
- [x] PoC 2 builds clean (`cargo build`, `cargo clippy`) with ratatui 0.29
- [ ] PoC 1 interactive gum UI on a real terminal
- [ ] PoC 2 `cargo run` interactive test
- [ ] Release workflow run + `onboard-rust.sh` end-to-end
- [ ] Clean Ubuntu container / WSL test of apt paths
- Startup time / size: gum path pays a one-time ~5 MB download; Rust path pays a one-time binary download (~2-4 MB stripped) but needs a published release first.
- Biggest functional gap: PoC 2 can't wrap interactive installers — a real onboarding flow would need TUI suspend/resume around those steps.
