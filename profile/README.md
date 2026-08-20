## Radio Frequency Identification

The RFID Lab at Auburn University is a research institute focusing on the business case and technical implementation of RFID and other emerging technologies in retail, aviation, supply chain, and manufacturing.

## New here? Set up your dev environment

Paste the line for your machine — it scans what you already have, then installs
the rest (git, GitHub login, Bun, Go, Rust, Vault, kubectl, and the lab's GUI apps):

**macOS, Linux, or WSL**

```sh
bash <(curl -fsSL https://raw.githubusercontent.com/AU-RFID/.github/main/public-scripts/onboard-rust.sh)
```

**Windows (PowerShell, on the host — it will ask which WSL distro to use)**

```powershell
irm https://raw.githubusercontent.com/AU-RFID/.github/main/public-scripts/onboard.ps1 | iex
```

Both bootstrappers download the prebuilt onboarding TUI from the latest
`onboarding-tui-v*` [release](https://github.com/AU-RFID/.github/releases) and
cache it, so re-running is cheap. Pass `--refresh` to force a re-download.
Safe to re-run at any time; nothing is reinstalled if it's already there.
