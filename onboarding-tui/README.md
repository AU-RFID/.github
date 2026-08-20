# Onboarding TUI

This tool sets up a developer machine for the RFID Lab.

It does three things:

1. It detects your operating system.
2. It scans your machine for the tools the lab uses.
3. It installs the tools that are missing.

The tool is a terminal application. It downloads as one prebuilt binary. You do
not need Rust, Node, or any other runtime to use it.

---

## 1. Start the tool

Use the command for your system. Each command downloads the latest binary and
runs it.

### macOS

Open **Terminal**. Enter this command:

```sh
bash <(curl -fsSL https://raw.githubusercontent.com/AU-RFID/.github/main/public-scripts/onboard.sh)
```

The tool installs command-line tools as Homebrew formulae. It installs desktop
applications as Homebrew casks.

### Linux (desktop)

Open a terminal. Enter this command:

```sh
bash <(curl -fsSL https://raw.githubusercontent.com/AU-RFID/.github/main/public-scripts/onboard.sh)
```

The tool offers command-line tools and desktop applications. Some desktop
applications are not available on Linux. The tool hides the applications that
it cannot install.

### Linux (server)

Open a terminal, or connect with SSH. Enter this command:

```sh
bash <(curl -fsSL https://raw.githubusercontent.com/AU-RFID/.github/main/public-scripts/onboard.sh)
```

The tool detects a server when there is no graphical desktop. On a server, the
tool offers only command-line tools. It does not offer desktop applications.

To set the mode manually, use the `RFID_ONBOARD_LINUX` variable:

```sh
RFID_ONBOARD_LINUX=server bash <(curl -fsSL .../public-scripts/onboard.sh)
```

Permitted values are `desktop` and `server`.

### Windows

Install WSL first. Open **PowerShell** and enter this command:

```powershell
wsl --install -d Ubuntu
```

Restart the computer if Windows asks you to do this.

Then open **PowerShell** and enter this command:

```powershell
irm https://raw.githubusercontent.com/AU-RFID/.github/main/public-scripts/onboard.ps1 | iex
```

The tool runs on Windows. It asks you to select a WSL distribution. Then it
installs the tools in two places:

| Tool type | Install location |
|---|---|
| Command-line tools | Inside the WSL distribution that you select |
| Desktop applications | On Windows, with `winget` |

### Inside WSL

You can also run the tool inside a WSL distribution. Open the distribution and
enter the macOS/Linux command. The tool then behaves as a Linux system. It does
not install desktop applications on the Windows host.

---

## 2. Use the screens

The tool has four screens. Use the keyboard to move between them.

### Welcome screen

The welcome screen shows your detected system.

| Key | Action |
|---|---|
| `←` `→` or `Tab` | Move between the buttons |
| `Enter` | Select the button |
| `q` or `Esc` | Close the tool |

Select **Get Started** to continue. Select **Exit** to stop.

### Distribution screen (Windows only)

This screen shows the WSL distributions on your computer.

| Key | Action |
|---|---|
| `↑` `↓` or `k` `j` | Move between the distributions |
| `Enter` | Select the distribution |
| `Esc` | Go back to the welcome screen |
| `q` | Close the tool |

If the list is empty, install a distribution first. Use `wsl --install -d Ubuntu`
in PowerShell. Then start the tool again.

### Scan screen

The tool scans your machine. Installed tools show a version. Missing tools show
an empty checkbox.

| Key | Action |
|---|---|
| `↑` `↓` or `k` `j` | Move between the rows |
| `→` or `l` | Open a section |
| `←` or `h` | Close a section |
| `Space` | Select or clear the tool under the cursor |
| `r` | Scan again |
| `Enter` | Start the installation (on the **Confirm** button) |
| `Esc` | Go back to the welcome screen |
| `q` | Close the tool |

The tools are in sections. Each section has one of three rules:

| Rule | Meaning |
|---|---|
| **Required** | The tool is always installed if it is missing. You cannot clear it. |
| **Pick at least one** | You must select one tool or more from the section. |
| **Optional** | You can select any number of tools, or none. |

The **Confirm** button is at the bottom of the list. Move down to the button and
press `Enter` to start.

### Install screen

The tool runs each installation step. The output pane shows the live output of
each command.

Some steps need your input. The tool cannot run these steps, because they
conflict with the full-screen interface. Examples are the Homebrew installer,
`gh auth login`, and the Git identity questions. The tool prints these steps as
follow-up commands on the summary screen. Run these commands yourself after the
tool closes.

### Summary screen

The summary screen shows the result of each step. It also lists the follow-up
commands.

| Key | Action |
|---|---|
| `Enter` | Scan again |
| `q` or `Esc` | Close the tool |

---

## 3. Options

### Test mode

Use `--dry-run` to see the commands without any change to your machine:

```sh
bash <(curl -fsSL .../public-scripts/onboard.sh) --dry-run
```

The tool prints each command with a `[dry-run]` prefix. It does not run the
command.

### New version

The tool keeps the binary in a cache. Use `--refresh` to download the latest
version again:

```sh
bash <(curl -fsSL .../public-scripts/onboard.sh) --refresh
```

The cache locations are:

| System | Location |
|---|---|
| macOS and Linux | `~/.cache/rfid-onboard/bin` |
| Windows | `%LOCALAPPDATA%\rfid-onboard` |

### Repeated use

The tool is safe to run more than once. It does not install a tool that is
already present. Run the tool again to check your machine, or to add more tools.

---

## 4. For maintainers

### Add or remove a tool

All tools are in **`software.json`**. You do not need to write Rust to change
the list.

Each entry has these fields:

- `section` — the section identifier. It must match a `sections[].id`.
- `kind` — `gui` for a desktop application, or `cli` for a terminal tool.
- A command block per operating system — `macos`, `linux`, or `any`.
- `check` — the command that detects the tool.
- `install` — the steps that install the tool.
- `winget_id` — the Windows package identifier. This field is optional.
- `follow_up` — commands for the user to run later. This field is optional.

`src/software.rs` reads the file. It then resolves the list for the detected
system:

| System | Desktop applications | Command-line tools |
|---|---|---|
| macOS | Homebrew casks | Homebrew formulae |
| Linux desktop | `linux` block. The tool hides the entry if there is no block. | `linux` block |
| Linux server | Excluded | `linux` block |
| Windows host | `winget` on the host | Inside the selected WSL distribution |

Run `cargo test` after a change. The tests check that the file parses. They also
check that desktop applications never appear on a Linux server.

### Build and release

Build the tool for a local test:

```sh
cargo build
cargo run -- --dry-run
```

To publish a new version, push a tag with the `onboarding-tui-v*` pattern. The
workflow `.github/workflows/release-onboarding-tui.yml` then builds five
binaries and creates the release:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `x86_64-unknown-linux-musl`
- `aarch64-unknown-linux-musl`
- `x86_64-pc-windows-msvc`

The bootstrap scripts always download the latest `onboarding-tui-v*` release.
Users get a change only after you publish a new release.

### Open items

These items still need a check on real hardware:

- Confirm the `winget` identifiers and the Homebrew casks for Yaak, Tower, Lens,
  MySQL Workbench, DataGrip, 1Password (`AgileBits.1Password`), and Bitwarden
  (`Bitwarden.Bitwarden`, `snap` on Linux). Use `winget search` and `brew info`.
- Add a `linux` block for Cursor, Lens, Linear, Yaak, Bruno, the database tools,
  Tower, and GitHub Desktop. These applications are macOS and Windows only at
  the moment.
- Tailscale installs inside WSL on a Windows host. Consider an install on the
  Windows host instead. This gives cleaner networking.
- Test the tool in a clean Ubuntu container and in WSL. This verifies the `apt`
  steps.
- Run the release workflow, then test `onboard.sh` from end to end.

### Notes on the software list

The list matches the tools in the lab's EAGLE repositories:

- **Bun** is the only JavaScript package manager. The list does not contain
  pnpm, npm, or fnm.
- **Go**, **Rust**, **Vault**, and **MySQL** are in the list.
- **Kubernetes**: the lab runs a k3s cluster. The list contains `kubectl` and
  Kustomize. It also contains k9s and Lens. ArgoCD is the GitOps layer, but the
  ArgoCD CLI is not in the list yet.
- **Xcode** and the Command Line Tools are macOS only. The `eagle-rfid-app` iOS
  application needs them.
- **Cursor** and **OpenCode** are both configured in `eagle-platform`.

Dashlane is not in the list. Dashlane retired its desktop application. It is now
a browser extension only, so the tool cannot install it or detect it.
