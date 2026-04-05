#!/usr/bin/env bash
set -euo pipefail

# ─────────────────────────────────────────────────────────────
#  Rust Agent — install script
#
#  Usage:
#    curl -fsSL https://raw.githubusercontent.com/ratchanonth60/rust-agent-code/master/install.sh | bash
#    # or
#    ./install.sh
#    ./install.sh --prefix ~/.local    # custom install prefix
#    ./install.sh --uninstall           # remove rust-agent
# ─────────────────────────────────────────────────────────────

REPO="https://github.com/ratchanonth60/rust-agent-code.git"
BIN_NAME="rust-agent"
CONFIG_DIR="${HOME}/.rust-agent"
DEFAULT_PREFIX="${HOME}/.local"

# ── Colors ────────────────────────────────────────────────────
if [ -t 1 ]; then
    BOLD="\033[1m"
    DIM="\033[2m"
    CYAN="\033[36m"
    GREEN="\033[32m"
    YELLOW="\033[33m"
    RED="\033[31m"
    RESET="\033[0m"
else
    BOLD="" DIM="" CYAN="" GREEN="" YELLOW="" RED="" RESET=""
fi

info()  { printf "${CYAN}${BOLD}▸${RESET} %s\n" "$*"; }
ok()    { printf "${GREEN}${BOLD}✓${RESET} %s\n" "$*"; }
warn()  { printf "${YELLOW}${BOLD}!${RESET} %s\n" "$*"; }
err()   { printf "${RED}${BOLD}✗${RESET} %s\n" "$*" >&2; }
die()   { err "$*"; exit 1; }

# ── Parse args ────────────────────────────────────────────────
PREFIX="${DEFAULT_PREFIX}"
UNINSTALL=false

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix)   PREFIX="$2"; shift 2 ;;
        --prefix=*) PREFIX="${1#*=}"; shift ;;
        --uninstall) UNINSTALL=true; shift ;;
        -h|--help)
            echo "Usage: install.sh [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --prefix <DIR>   Install prefix (default: ~/.local)"
            echo "  --uninstall      Remove rust-agent"
            echo "  -h, --help       Show this help"
            exit 0
            ;;
        *) die "Unknown option: $1" ;;
    esac
done

BIN_DIR="${PREFIX}/bin"
INSTALL_PATH="${BIN_DIR}/${BIN_NAME}"

# ── Uninstall ─────────────────────────────────────────────────
if [ "$UNINSTALL" = true ]; then
    info "Uninstalling rust-agent..."
    if [ -f "$INSTALL_PATH" ]; then
        rm -f "$INSTALL_PATH"
        ok "Removed ${INSTALL_PATH}"
    else
        warn "Binary not found at ${INSTALL_PATH}"
    fi
    echo ""
    warn "Config directory kept at ${CONFIG_DIR}"
    warn "Remove it manually if you want a clean uninstall:"
    echo "  rm -rf ${CONFIG_DIR}"
    exit 0
fi

# ── Banner ────────────────────────────────────────────────────
echo ""
printf "${CYAN}${BOLD}"
cat << 'ART'
    ____             __     ___                    __
   / __ \__  _______/ /_   /   | ____ ____  ____  / /_
  / /_/ / / / / ___/ __/  / /| |/ __ `/ _ \/ __ \/ __/
 / _, _/ /_/ (__  ) /_   / ___ / /_/ /  __/ / / / /_
/_/ |_|\__,_/____/\__/  /_/  |_\__, /\___/_/ /_/\__/
                               /____/
ART
printf "${RESET}"
echo ""

# ── Dependency checks ─────────────────────────────────────────
check_cmd() {
    if ! command -v "$1" &>/dev/null; then
        return 1
    fi
    return 0
}

info "Checking dependencies..."

# Rust toolchain
if ! check_cmd rustc || ! check_cmd cargo; then
    warn "Rust toolchain not found."
    echo ""
    info "Installing Rust via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "${HOME}/.cargo/env" 2>/dev/null || true
    check_cmd cargo || die "Failed to install Rust. Please install manually: https://rustup.rs"
fi
ok "Rust $(rustc --version | awk '{print $2}')"

# ripgrep (required by GrepTool)
if ! check_cmd rg; then
    warn "ripgrep (rg) not found — required by GrepTool"
    echo ""
    echo "  Install it with your package manager:"
    echo "    Arch:   sudo pacman -S ripgrep"
    echo "    Ubuntu: sudo apt install ripgrep"
    echo "    macOS:  brew install ripgrep"
    echo "    Cargo:  cargo install ripgrep"
    echo ""
    read -rp "  Continue without ripgrep? [y/N] " yn
    case "$yn" in
        [yY]*) warn "Continuing — GrepTool will not work until rg is installed." ;;
        *)     die "Install ripgrep first, then re-run this script." ;;
    esac
else
    ok "ripgrep $(rg --version | head -1 | awk '{print $2}')"
fi

# git
check_cmd git || die "git is required but not found."
ok "git $(git --version | awk '{print $3}')"

# ── Build ─────────────────────────────────────────────────────
WORK_DIR=""
BUILD_IN_PLACE=false

# If we're already inside the repo, build in place
if [ -f "Cargo.toml" ] && grep -q 'name = "rust-agent"' Cargo.toml 2>/dev/null; then
    BUILD_IN_PLACE=true
    WORK_DIR="$(pwd)"
    info "Building in current directory..."
else
    WORK_DIR="$(mktemp -d)"
    info "Cloning repository..."
    git clone --depth 1 "$REPO" "$WORK_DIR" 2>&1 | tail -1
    ok "Cloned to ${WORK_DIR}"
fi

info "Building release binary (this may take a few minutes)..."
cd "$WORK_DIR"
cargo build --release 2>&1 | tail -5

BUILT_BIN="target/release/${BIN_NAME}"
[ -f "$BUILT_BIN" ] || die "Build failed — binary not found at ${BUILT_BIN}"
ok "Build complete"

# ── Install ───────────────────────────────────────────────────
mkdir -p "$BIN_DIR"
cp "$BUILT_BIN" "$INSTALL_PATH"
chmod +x "$INSTALL_PATH"
ok "Installed to ${INSTALL_PATH}"

# ── Config directory ──────────────────────────────────────────
if [ ! -d "$CONFIG_DIR" ]; then
    mkdir -p "${CONFIG_DIR}/memory"
    mkdir -p "${CONFIG_DIR}/sessions"
    mkdir -p "${CONFIG_DIR}/plugins"
    mkdir -p "${CONFIG_DIR}/skills"
    mkdir -p "${CONFIG_DIR}/output-styles"
    ok "Created config directory at ${CONFIG_DIR}"
else
    ok "Config directory already exists at ${CONFIG_DIR}"
fi

# ── Cleanup temp directory ────────────────────────────────────
if [ "$BUILD_IN_PLACE" = false ] && [ -n "$WORK_DIR" ]; then
    rm -rf "$WORK_DIR"
fi

# ── PATH check ────────────────────────────────────────────────
if ! echo "$PATH" | tr ':' '\n' | grep -qx "$BIN_DIR"; then
    echo ""
    warn "${BIN_DIR} is not in your PATH."
    echo ""
    echo "  Add it to your shell profile:"
    echo ""
    echo "    # bash"
    echo "    echo 'export PATH=\"${BIN_DIR}:\$PATH\"' >> ~/.bashrc"
    echo ""
    echo "    # zsh"
    echo "    echo 'export PATH=\"${BIN_DIR}:\$PATH\"' >> ~/.zshrc"
    echo ""
    echo "    # fish"
    echo "    fish_add_path ${BIN_DIR}"
    echo ""
fi

# ── API key reminder ─────────────────────────────────────────
echo ""
echo "────────────────────────────────────────────────"
printf "${BOLD}  Setup complete!${RESET}\n"
echo "────────────────────────────────────────────────"
echo ""
echo "  Set an API key to get started:"
echo ""
printf "    ${DIM}# Gemini (default provider)${RESET}\n"
echo "    export GEMINI_API_KEY=your-key"
echo ""
printf "    ${DIM}# Claude${RESET}\n"
echo "    export ANTHROPIC_API_KEY=your-key"
echo ""
printf "    ${DIM}# OpenAI${RESET}\n"
echo "    export OPENAI_API_KEY=your-key"
echo ""
echo "  Then run:"
echo ""
printf "    ${CYAN}${BOLD}${BIN_NAME}${RESET}\n"
echo ""
