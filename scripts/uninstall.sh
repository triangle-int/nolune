#!/bin/bash
# ╔══════════════════════════════════════════════╗
# ║        nolune — AI companion uninstaller       ║
# ╚══════════════════════════════════════════════╝
#
# Usage:
#   curl -fsSL https://nolune.dev/uninstall.sh | bash
#
# Options (env vars):
#   NOLUNE_DIR=/custom/path   Uninstall from custom directory (default: ~/.nolune)
#   KEEP_DATA=1              Keep user data (instances, config) — only remove binary + service
#
set -e

# ─── Colors ───────────────────────────────────────────────────────────────────
BOLD='\033[1m'
DIM='\033[2m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
RED='\033[0;31m'
NC='\033[0m'

log()  { echo -e "  ${GREEN}✓${NC} $1"; }
info() { echo -e "  ${DIM}$1${NC}"; }
warn() { echo -e "  ${YELLOW}!${NC} $1"; }
fail() { echo -e "  ${RED}✗${NC} $1"; exit 1; }
step() { echo -e "\n${CYAN}${BOLD}$1${NC}"; }

# ─── Banner ───────────────────────────────────────────────────────────────────
echo ""
echo -e "${BOLD}  ┌─────────────────────────────┐${NC}"
echo -e "${BOLD}  │${NC}     ${CYAN}nolune${NC} uninstaller      ${BOLD}│${NC}"
echo -e "${BOLD}  └─────────────────────────────┘${NC}"

# ─── Detect platform ─────────────────────────────────────────────────────────
OS="$(uname -s)"
case "$OS" in
    Linux)   PLATFORM="linux" ;;
    Darwin)  PLATFORM="macos" ;;
    *)       fail "unsupported OS: $OS" ;;
esac

# ─── Config ───────────────────────────────────────────────────────────────────
NOLUNE_DIR="${NOLUNE_DIR:-$HOME/.nolune}"
BIN_DIR="$NOLUNE_DIR/bin"
BIN="$BIN_DIR/nolune"
KEEP_DATA="${KEEP_DATA:-}"

if [ ! -d "$NOLUNE_DIR" ]; then
    fail "nolune directory not found at $NOLUNE_DIR — nothing to uninstall"
fi

# ─── Interactive prompt ──────────────────────────────────────────────────────
if [ -z "$KEEP_DATA" ] && [ -t 1 ] && [ -e /dev/tty ]; then
    # Terminal available — ask user (read from /dev/tty so it works with curl | bash)
    echo ""
    echo -e "  Your data is at ${BOLD}$NOLUNE_DIR${NC}"
    echo -e "  This includes config, memories, chats, and uploads."
    echo ""
    echo -e "  ${BOLD}1)${NC} Remove everything (binary + data)"
    echo -e "  ${BOLD}2)${NC} Keep my data (only remove binary + service)"
    echo ""
    printf "  Choose [1/2]: "
    read -r choice < /dev/tty
    case "$choice" in
        2) KEEP_DATA=1 ;;
        *) KEEP_DATA=0 ;;
    esac
elif [ -z "$KEEP_DATA" ]; then
    # No terminal at all — default to keep data (safe default)
    KEEP_DATA=1
fi

# ─── Uninstall through the binary ────────────────────────────────────────────
# The binary owns service teardown and file removal (#126). The script only keeps
# the parts that are shell-specific: the prompt above and the PATH cleanup below.
step "uninstalling nolune"

export NOLUNE_HOME="$NOLUNE_DIR"

if [ -x "$BIN" ] && "$BIN" uninstall --help >/dev/null 2>&1; then
    if [ "$KEEP_DATA" = "1" ]; then
        "$BIN" uninstall --keep-data --yes || fail "nolune uninstall failed"
    else
        "$BIN" uninstall --yes || fail "nolune uninstall failed"
    fi
else
    # Installs that predate `nolune uninstall`: nothing to delegate to.
    warn "no usable nolune binary at $BIN; removing files directly"
    if [ "$KEEP_DATA" = "1" ]; then
        rm -rf "$BIN_DIR" "$NOLUNE_DIR/nolune.log"
        log "removed binary and logs"
        info "kept user data at $NOLUNE_DIR"
    else
        rm -rf "$NOLUNE_DIR"
        log "removed $NOLUNE_DIR"
    fi
fi

# ─── Clean PATH from shell rc ────────────────────────────────────────────────
step "cleaning shell config"

SHELL_NAME=$(basename "$SHELL" 2>/dev/null || echo "bash")
RC_FILE="$HOME/.${SHELL_NAME}rc"

if [ -f "$RC_FILE" ] && grep -q "$BIN_DIR" "$RC_FILE"; then
    # Remove the nolune PATH lines (comment + export)
    sed -i.bak '/# nolune/d' "$RC_FILE"
    sed -i.bak "\|$BIN_DIR|d" "$RC_FILE"
    rm -f "${RC_FILE}.bak"
    log "removed PATH entry from $RC_FILE"
else
    info "no PATH entry found in $RC_FILE"
fi

# ─── Done ─────────────────────────────────────────────────────────────────────
echo ""
echo -e "${BOLD}  ┌─────────────────────────────┐${NC}"
echo -e "${BOLD}  │${NC}  ${GREEN}nolune uninstalled${NC}         ${BOLD}│${NC}"
echo -e "${BOLD}  └─────────────────────────────┘${NC}"
echo ""
if [ "$KEEP_DATA" = "1" ]; then
    echo -e "  ${DIM}your data is still at ${BOLD}$NOLUNE_DIR${NC}"
    echo -e "  ${DIM}to remove it: ${BOLD}rm -rf $NOLUNE_DIR${NC}"
fi
echo ""
