# Onboarding TUI PoC comparison notes

> **Decision (2026-07-23): going with the Rust/ratatui PoC.** It now has the
> Auburn-themed welcome screen, an automatic installed-vs-wanted scan, and a
> modular software registry (`src/software.rs`) — add/remove a tool by editing
> one `Software` entry there; every screen picks it up. The gum PoC below is
> kept for reference.


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
