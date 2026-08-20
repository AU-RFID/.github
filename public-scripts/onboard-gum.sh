#!/usr/bin/env bash
#
# RFID Lab onboarding TUI (PoC 1: bash + gum)
#
# Sets up a development environment for new lab members on macOS, Linux, or
# WSL. Downloads its only dependency (charmbracelet/gum) at runtime; if that
# fails it degrades to plain prompts. Safe to re-run — every step is
# idempotent.
#
#   bash <(curl -fsSL https://raw.githubusercontent.com/AU-RFID/.github/main/public-scripts/onboard-gum.sh)
#
set -euo pipefail

# --dry-run (or RFID_ONBOARD_DRY_RUN=1): walk the full UI but only print what
# would be installed/changed — nothing on the machine is touched.
DRY_RUN="${RFID_ONBOARD_DRY_RUN:-0}"
for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=1 ;;
    *) echo "Unknown option: $arg (supported: --dry-run)" >&2; exit 2 ;;
  esac
done

GUM_VERSION="0.17.0"
CACHE_DIR="${HOME}/.cache/rfid-onboard"
BIN_DIR="${CACHE_DIR}/bin"
GUM="" # resolved by bootstrap_gum

# ---------------------------------------------------------------------------
# Platform detection
# ---------------------------------------------------------------------------

OS=""    # Darwin | Linux
ARCH=""  # arm64 | x86_64
IS_WSL="no"
PKG=""   # brew | apt

detect_platform() {
  OS="$(uname -s)"
  ARCH="$(uname -m)"
  case "$ARCH" in
    aarch64) ARCH="arm64" ;;
    amd64)   ARCH="x86_64" ;;
  esac

  case "$OS" in
    Darwin) PKG="brew" ;;
    Linux)
      if grep -qi microsoft /proc/version 2>/dev/null; then
        IS_WSL="yes"
      fi
      if command -v apt-get >/dev/null 2>&1; then
        PKG="apt"
      else
        echo "Sorry, this script currently supports macOS and Debian/Ubuntu-family Linux (incl. WSL)." >&2
        exit 1
      fi
      ;;
    *)
      echo "Unsupported OS: $OS" >&2
      exit 1
      ;;
  esac
}

# ---------------------------------------------------------------------------
# gum bootstrap (with plain-prompt fallback)
# ---------------------------------------------------------------------------

bootstrap_gum() {
  # Escape hatch: RFID_ONBOARD_PLAIN=1 forces the plain-prompt UI (useful for
  # testing and terminals where gum misbehaves).
  if [ "${RFID_ONBOARD_PLAIN:-}" = "1" ]; then
    GUM=""
    return
  fi
  if command -v gum >/dev/null 2>&1; then
    GUM="$(command -v gum)"
    return
  fi
  if [ -x "${BIN_DIR}/gum" ]; then
    GUM="${BIN_DIR}/gum"
    return
  fi

  mkdir -p "$BIN_DIR"
  local tarball="gum_${GUM_VERSION}_${OS}_${ARCH}.tar.gz"
  local url="https://github.com/charmbracelet/gum/releases/download/v${GUM_VERSION}/${tarball}"
  local tmp
  tmp="$(mktemp -d)"

  echo "Downloading gum ${GUM_VERSION} (${OS}/${ARCH})..."
  if curl -fsSL "$url" -o "${tmp}/${tarball}" \
      && tar -xzf "${tmp}/${tarball}" -C "$tmp" \
      && install -m 0755 "$(find "$tmp" -type f -name gum | head -1)" "${BIN_DIR}/gum"; then
    GUM="${BIN_DIR}/gum"
  else
    echo "Could not download gum — falling back to plain prompts."
    GUM=""
  fi
  rm -rf "$tmp"
}

# ---------------------------------------------------------------------------
# UI helpers (gum when available, plain fallbacks otherwise)
# ---------------------------------------------------------------------------

banner() {
  if [ -n "$GUM" ]; then
    "$GUM" style --border rounded --margin "1 2" --padding "1 4" \
      --border-foreground 212 --bold "$@"
  else
    echo
    printf '== %s ==\n' "$@"
    echo
  fi
}

say() {
  if [ -n "$GUM" ]; then
    "$GUM" style --foreground 212 "$*"
  else
    echo "$*"
  fi
}

# choose "header" option...  -> prints selection
choose() {
  local header="$1"; shift
  if [ -n "$GUM" ]; then
    "$GUM" choose --header "$header" "$@"
  else
    echo "$header" >&2
    select opt in "$@"; do
      [ -n "$opt" ] && { echo "$opt"; return; }
    done
  fi
}

# choose_multi "header" option... -> prints selections, one per line
choose_multi() {
  local header="$1"; shift
  if [ -n "$GUM" ]; then
    "$GUM" choose --no-limit --header "$header (space to toggle, enter to confirm)" "$@"
  else
    echo "$header" >&2
    echo "Enter numbers separated by spaces (e.g. '1 3'):" >&2
    local i=1 opt
    for opt in "$@"; do echo "  $i) $opt" >&2; i=$((i+1)); done
    local nums; read -r nums
    for i in $nums; do
      eval "echo \"\${$i}\""
    done
  fi
}

# ask "prompt" "default" -> prints answer
ask() {
  local prompt="$1" default="${2:-}"
  if [ -n "$GUM" ]; then
    "$GUM" input --prompt "${prompt}: " --value "$default"
  else
    local ans
    read -r -p "${prompt} [${default}]: " ans
    echo "${ans:-$default}"
  fi
}

confirm() {
  if [ -n "$GUM" ]; then
    "$GUM" confirm "$1"
  else
    local ans
    read -r -p "$1 [y/N]: " ans
    [[ "$ans" =~ ^[Yy] ]]
  fi
}

# mutate cmd... — run a state-changing command, or just print it in dry-run
mutate() {
  if [ "$DRY_RUN" = 1 ]; then
    say "  [dry-run] would run: $*"
  else
    "$@"
  fi
}

# spin "title" cmd... — show a spinner while running cmd (logs to file on failure)
spin() {
  local title="$1"; shift
  local log="${CACHE_DIR}/last-step.log"
  if [ "$DRY_RUN" = 1 ]; then
    say "  [dry-run] $title — would run: $*"
    return 0
  fi
  if [ -n "$GUM" ]; then
    if ! "$GUM" spin --spinner dot --title "$title" --show-error -- "$@"; then
      say "✗ Failed: $title"
      return 1
    fi
  else
    echo "-> $title"
    if ! "$@" >"$log" 2>&1; then
      echo "✗ Failed: $title (log: $log)"
      return 1
    fi
  fi
}

ok()   { say "  ✓ $*"; }
bad()  { if [ -n "$GUM" ]; then "$GUM" style --foreground 196 "  ✗ $*"; else echo "  ✗ $*"; fi; }

# ---------------------------------------------------------------------------
# Components: each has install_X (idempotent) and check_X (used by doctor)
# ---------------------------------------------------------------------------

# --- core: package manager, git, gh, git config, ssh key -------------------

ensure_pkg_manager() {
  if [ "$PKG" = "brew" ]; then
    if ! command -v brew >/dev/null 2>&1; then
      say "Homebrew is not installed. Its installer is interactive, so you'll drive it:"
      say '  /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"'
      if confirm "Run the Homebrew installer now?"; then
        if [ "$DRY_RUN" = 1 ]; then
          say "  [dry-run] would run the Homebrew installer"
        else
          /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
          # brew on Apple Silicon lands in /opt/homebrew
          [ -x /opt/homebrew/bin/brew ] && eval "$(/opt/homebrew/bin/brew shellenv)"
          [ -x /usr/local/bin/brew ] && eval "$(/usr/local/bin/brew shellenv)"
        fi
      else
        say "Skipping Homebrew — some installs below may fail."
      fi
    fi
  else
    spin "Updating apt package index (needs sudo)" sudo apt-get update -y
  fi
}

pkg_install() { # pkg_install <brew-name> <apt-name>
  if [ "$PKG" = "brew" ]; then
    brew list "$1" >/dev/null 2>&1 || spin "Installing $1" brew install "$1"
  else
    dpkg -s "$2" >/dev/null 2>&1 || spin "Installing $2 (needs sudo)" sudo apt-get install -y "$2"
  fi
}

install_core() {
  banner "Core tools"
  ensure_pkg_manager
  command -v git >/dev/null 2>&1 || pkg_install git git
  command -v gh  >/dev/null 2>&1 || pkg_install gh gh
  command -v curl >/dev/null 2>&1 || pkg_install curl curl

  # git identity
  local name email
  name="$(git config --global user.name || true)"
  email="$(git config --global user.email || true)"
  if [ -z "$name" ] || [ -z "$email" ]; then
    banner "Git identity"
    name="$(ask "Your full name" "$name")"
    email="$(ask "Your school/work email" "$email")"
    mutate git config --global user.name "$name"
    mutate git config --global user.email "$email"
    mutate git config --global init.defaultBranch main
  fi
  if [ "$DRY_RUN" = 1 ]; then
    ok "git would be configured as: ${name:-<unset>} <${email:-<unset>}>"
  else
    ok "git configured as: $(git config --global user.name) <$(git config --global user.email)>"
  fi

  # SSH key
  local key="${HOME}/.ssh/id_ed25519"
  if [ ! -f "$key" ]; then
    say "Generating an SSH key for GitHub..."
    mutate mkdir -p "${HOME}/.ssh"
    mutate chmod 700 "${HOME}/.ssh"
    mutate ssh-keygen -t ed25519 -C "${email:-$(git config --global user.email || true)}" -f "$key" -N ""
  fi
  ok "SSH key: ${key}.pub"

  # GitHub auth (interactive; hand the terminal to gh)
  if command -v gh >/dev/null 2>&1 && ! gh auth status >/dev/null 2>&1; then
    if confirm "Log in to GitHub now? (opens a browser device-code flow)"; then
      mutate gh auth login --hostname github.com --git-protocol ssh || true
    fi
  fi
}

check_core() {
  local rc=0
  for t in git gh curl; do
    if command -v "$t" >/dev/null 2>&1; then
      ok "$t $("$t" --version 2>/dev/null | head -1)"
    else
      bad "$t not found"; rc=1
    fi
  done
  [ -n "$(git config --global user.name 2>/dev/null || true)" ] \
    && ok "git identity set" || { bad "git identity not set"; rc=1; }
  [ -f "${HOME}/.ssh/id_ed25519.pub" ] \
    && ok "SSH key exists" || { bad "no SSH key"; rc=1; }
  if command -v gh >/dev/null 2>&1; then
    gh auth status >/dev/null 2>&1 && ok "gh authenticated" || { bad "gh not logged in"; rc=1; }
  fi
  return $rc
}

# --- node: fnm + LTS node + pnpm -------------------------------------------

install_node() {
  banner "Node (fnm + LTS + pnpm)"
  if ! command -v fnm >/dev/null 2>&1 && [ ! -x "${HOME}/.local/share/fnm/fnm" ]; then
    pkg_install fnm fnm || {
      # apt has no fnm package; use the official installer script
      spin "Installing fnm" bash -c \
        'curl -fsSL https://fnm.vercel.app/install -o /tmp/fnm-install.sh && bash /tmp/fnm-install.sh --skip-shell'
    }
  fi
  # Make fnm usable in this shell
  export PATH="${HOME}/.local/share/fnm:${PATH}"
  command -v fnm >/dev/null 2>&1 && eval "$(fnm env)" || true

  if [ "$DRY_RUN" = 1 ] && ! command -v fnm >/dev/null 2>&1; then
    say "  [dry-run] would install Node LTS + pnpm via fnm and add fnm init to your shell rc"
    return 0
  fi
  if command -v fnm >/dev/null 2>&1; then
    spin "Installing Node LTS" fnm install --lts
    if [ "$DRY_RUN" != 1 ]; then
      fnm default lts-latest >/dev/null 2>&1 || true
      eval "$(fnm env --use-on-cd 2>/dev/null || fnm env)"
      fnm use lts-latest >/dev/null 2>&1 || true
    fi
    if command -v corepack >/dev/null 2>&1; then
      spin "Enabling pnpm via corepack" corepack enable pnpm
    fi
    # Persist fnm init in the user's shell rc
    local rc_file="${HOME}/.zshrc"
    [ "${SHELL##*/}" = "bash" ] && rc_file="${HOME}/.bashrc"
    if ! grep -q 'fnm env' "$rc_file" 2>/dev/null; then
      if [ "$DRY_RUN" = 1 ]; then
        say "  [dry-run] would add fnm init to ${rc_file}"
      else
        {
          echo ''
          echo '# fnm (added by RFID Lab onboarding)'
          echo 'export PATH="$HOME/.local/share/fnm:$PATH"'
          echo 'command -v fnm >/dev/null && eval "$(fnm env --use-on-cd)"'
        } >> "$rc_file"
        ok "Added fnm init to ${rc_file}"
      fi
    fi
  else
    bad "fnm install failed"
  fi
}

check_node() {
  local rc=0
  export PATH="${HOME}/.local/share/fnm:${PATH}"
  command -v fnm >/dev/null 2>&1 && eval "$(fnm env)" 2>/dev/null || true
  for t in fnm node pnpm; do
    if command -v "$t" >/dev/null 2>&1; then
      ok "$t $("$t" --version 2>/dev/null | head -1)"
    else
      bad "$t not found"; rc=1
    fi
  done
  return $rc
}

# --- rust: rustup ----------------------------------------------------------

install_rust() {
  banner "Rust (rustup)"
  if [ -x "${HOME}/.cargo/bin/rustc" ]; then
    ok "Rust already installed"
  else
    spin "Installing Rust via rustup (this takes a minute)" bash -c \
      'curl --proto "=https" --tlsv1.2 -fsSL https://sh.rustup.rs -o /tmp/rustup-init.sh && sh /tmp/rustup-init.sh -y --no-modify-path'
    # rustup -y adds ~/.cargo/bin to PATH via ~/.cargo/env; source it for this session
    # shellcheck disable=SC1091
    [ -f "${HOME}/.cargo/env" ] && . "${HOME}/.cargo/env"
    local rc_file="${HOME}/.zshrc"
    [ "${SHELL##*/}" = "bash" ] && rc_file="${HOME}/.bashrc"
    if ! grep -q '.cargo/env' "$rc_file" 2>/dev/null; then
      if [ "$DRY_RUN" = 1 ]; then
        say "  [dry-run] would add cargo env to ${rc_file}"
      else
        printf '\n# Rust (added by RFID Lab onboarding)\n. "$HOME/.cargo/env"\n' >> "$rc_file"
        ok "Added cargo env to ${rc_file}"
      fi
    fi
  fi
}

check_rust() {
  # shellcheck disable=SC1091
  [ -f "${HOME}/.cargo/env" ] && . "${HOME}/.cargo/env"
  if command -v rustc >/dev/null 2>&1; then
    ok "rustc $(rustc --version)"
    ok "cargo $(cargo --version)"
  else
    bad "rust not found"
    return 1
  fi
}

# --- go --------------------------------------------------------------------

install_go() {
  banner "Go"
  if command -v go >/dev/null 2>&1; then
    ok "Go already installed"
  else
    pkg_install go golang-go
  fi
}

check_go() {
  if command -v go >/dev/null 2>&1; then
    ok "$(go version)"
  else
    bad "go not found"
    return 1
  fi
}

# ---------------------------------------------------------------------------
# Modes
# ---------------------------------------------------------------------------

ALL_COMPONENTS=("Core (git, gh, SSH, GitHub login)" "Node (fnm, LTS, pnpm)" "Rust (rustup)" "Go")

run_component() {
  case "$1" in
    Core*) install_core ;;
    Node*) install_node ;;
    Rust*) install_rust ;;
    Go*)   install_go ;;
  esac
}

doctor() {
  banner "Doctor — environment check"
  local rc=0
  say "Core:";  check_core  || rc=1
  say "Node:";  check_node  || rc=1
  say "Rust:";  check_rust  || rc=1
  say "Go:";    check_go    || rc=1
  echo
  if [ $rc -eq 0 ]; then
    banner "All checks passed — you're ready to go! 🎉"
  else
    banner "Some checks failed" "Re-run this script and pick the failing components."
  fi
  return 0
}

main() {
  detect_platform
  mkdir -p "$CACHE_DIR"
  bootstrap_gum

  local wsl_note="" dry_note=""
  [ "$IS_WSL" = "yes" ] && wsl_note=" (WSL)"
  [ "$DRY_RUN" = 1 ] && dry_note="DRY RUN — nothing will be installed"
  banner "RFID Lab Onboarding" "Welcome! Detected: ${OS} ${ARCH}${wsl_note}" ${dry_note:+"$dry_note"}

  local mode
  mode="$(choose "What would you like to do?" \
    "Full setup (everything)" \
    "Pick components" \
    "Doctor (check my environment)")"

  case "$mode" in
    Full*)
      for c in "${ALL_COMPONENTS[@]}"; do run_component "$c"; done
      doctor
      ;;
    Pick*)
      local picked
      picked="$(choose_multi "Which components?" "${ALL_COMPONENTS[@]}")"
      while IFS= read -r c; do
        [ -n "$c" ] && run_component "$c"
      done <<< "$picked"
      doctor
      ;;
    Doctor*)
      doctor
      ;;
  esac

  say ""
  say "Tip: open a NEW terminal so PATH changes take effect."
}

main "$@"
