# Onboarding TUI notes

> **This is the onboarding tool the lab uses.** Rust/ratatui: Auburn-themed
> welcome screen, automatic installed-vs-wanted scan, sectioned Ubuntu-style
> checklist, Windows-host + WSL support, and a Linux desktop/server split.
> Shipped as prebuilt binaries from the `onboarding-tui-v*` releases and
> launched by `public-scripts/onboard.sh` (macOS/Linux/WSL) or
> `public-scripts/onboard.ps1` (Windows host).

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

## Aligned with the EAGLE projects (`~/Developer/EAGLE`)

The registry mirrors what the lab's real repos use:
- **Bun** is the only JS package manager (bun@1.3.x) — pnpm/npm/fnm dropped.
- **Go** (1.25–1.26), **Rust**, **Vault**, **MySQL** (MySQL Workbench).
- **Kubernetes**: k3s cluster driven by kubectl + Kustomize (added), viewed
  with k9s / Lens. ArgoCD is the GitOps layer (CLI not added yet).
- **Xcode + CLT** (macOS only) for the `eagle-rfid-app` iOS app / TestFlight.
- AI editors Cursor + OpenCode are both configured in `eagle-platform`.

Not yet reflected (candidates): ArgoCD CLI, a MySQL client (mysql/mycli),
Meilisearch/SpiceDB/Keycloak are cluster-side only. Lint/format is Biome
(project dep, no system tool).

## Needs verification on real hosts

- winget IDs / casks flagged as best-effort: **Yaak**, **Tower**, Lens,
  MySQL Workbench, DataGrip, **1Password** (`AgileBits.1Password`),
  **Bitwarden** (`Bitwarden.Bitwarden`, Linux via `snap`) (confirm with
  `winget search` / `brew info`).
- Several GUI apps are macOS+Windows only (no `linux` block yet): Cursor,
  Lens, Linear, Yaak, Bruno, the DB tools, Tower, GitHub Desktop. Add a
  `linux` block (apt repo / flatpak / script) to offer them on Linux desktop.
- **Password Managers** section (optional): 1Password (preferred, free for
  students via the GitHub Student pack) and Bitwarden (open-source, free tier).
  Both are `kind: gui`, so on a **Windows host they install via winget on the
  host, not inside WSL** — which is what we want for a password manager.
  Dashlane was dropped: it retired its desktop app and is browser-extension
  only, so it can't be installed or detected cleanly.
- **Tailscale** (Required, `kind: cli`): macOS uses the `tailscale` cask; Linux
  uses the official `install.sh` (downloaded to /tmp first, per repo convention).
  On a Windows host it installs inside WSL — consider installing Tailscale on
  the Windows host instead for cleaner networking.


## Shipping

| | |
|---|---|
| Entry points | `public-scripts/onboard.sh` (macOS/Linux/WSL), `public-scripts/onboard.ps1` (Windows host) |
| Runtime deps | none — static musl / native macOS / Windows binary, cached under `~/.cache/rfid-onboard/bin` (`%LOCALAPPDATA%\rfid-onboard` on Windows); `--refresh` forces a re-download |
| Release pipeline | `.github/workflows/release-onboarding-tui.yml`, triggered by a `onboarding-tui-v*` tag |
| Interactive steps (Homebrew installer, `gh auth login`, git identity prompts) | **not** handled inside the TUI (alternate screen conflicts); printed as follow-up commands on the Doctor screen |
| UI | full-screen: step list + live streaming output pane, doctor summary |

Every change needs a new tag + release before users pick it up.

## Status

- [x] Builds clean (`cargo build`, `cargo clippy`) with ratatui 0.29
- [ ] `cargo run` interactive test
- [ ] Release workflow run + `onboard.sh` end-to-end
- [ ] Clean Ubuntu container / WSL test of apt paths
- Biggest functional gap: the TUI can't wrap interactive installers — that
  would need TUI suspend/resume around those steps.
