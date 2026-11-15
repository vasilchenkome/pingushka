#!/bin/sh
set -eu

GITHUB="${PINGUSHKA_UPDATE_GITHUB:-https://github.com/vasilchenkome/pingushka}"
BIN="${PINGUSHKA_UPDATE_BIN:-/usr/local/bin/pingushka-agent}"
SERVICE="${PINGUSHKA_UPDATE_SERVICE:-pingushka-agent.service}"
LOCK="${PINGUSHKA_UPDATE_LOCK:-/run/pingushka-agent-update.lock}"
TAG="${PINGUSHKA_UPDATE_TAG:-}"
HEALTH_DELAY="${PINGUSHKA_UPDATE_HEALTH_DELAY:-5}"

log() {
    echo "pingushka-update: $*"
}

die() {
    log "ERROR: $*"
    exit 1
}

[ -x "$BIN" ] || die "agent binary not found: $BIN"
if [ "$(id -u)" -ne 0 ] && [ "${PINGUSHKA_UPDATE_ALLOW_NONROOT:-0}" != "1" ]; then
    die "run as root"
fi

command -v curl >/dev/null 2>&1 || die "curl is required"
command -v sha256sum >/dev/null 2>&1 || die "sha256sum is required"
command -v systemctl >/dev/null 2>&1 || die "systemctl is required"

case "$(uname -m)" in
    x86_64|amd64) ARCH="x86_64" ;;
    aarch64|arm64) ARCH="aarch64" ;;
    *) die "unsupported architecture: $(uname -m)" ;;
esac

if command -v flock >/dev/null 2>&1; then
    exec 9>"$LOCK"
    flock -n 9 || { log "another update is already running"; exit 0; }
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

if [ -z "$TAG" ]; then
    TAG="$(curl -fsSL -o /dev/null -w '%{url_effective}' "$GITHUB/releases/latest" | sed -n 's#.*/tag/##p')"
fi
case "$TAG" in
    v[0-9]*.[0-9]*.[0-9]*) ;;
    *) die "unexpected release tag: ${TAG:-<empty>}" ;;
esac
LATEST="${TAG#v}"

BASE="${PINGUSHKA_UPDATE_DOWNLOAD_BASE:-$GITHUB/releases/download/$TAG}"
ASSET="pingushka-agent-linux-${ARCH}"

log "checking ${TAG} (${ARCH})"
curl -fsSL --retry 3 --retry-delay 2 "$BASE/$ASSET" -o "$TMP_DIR/$ASSET" \
    || die "cannot download $BASE/$ASSET"
curl -fsSL --retry 3 --retry-delay 2 "$BASE/$ASSET.sha256" -o "$TMP_DIR/$ASSET.sha256" \
    || die "cannot download checksum"
(
    cd "$TMP_DIR"
    sha256sum -c "$ASSET.sha256" >/dev/null
) || die "checksum verification failed"

SELF_PATH="${PINGUSHKA_UPDATE_SELF:-/usr/local/lib/pingushka/update-agent.sh}"
if [ -f "$SELF_PATH" ]; then
    if curl -fsSL --retry 2 "$BASE/update-agent.sh" -o "$TMP_DIR/update-agent.sh" \
        && curl -fsSL --retry 2 "$BASE/update-agent.sh.sha256" -o "$TMP_DIR/update-agent.sh.sha256"; then
        if (
            cd "$TMP_DIR"
            sha256sum -c "update-agent.sh.sha256" >/dev/null
        ); then
            if ! cmp -s "$TMP_DIR/update-agent.sh" "$SELF_PATH"; then
                install -m 0755 "$TMP_DIR/update-agent.sh" "$SELF_PATH.tmp"
                mv -f "$SELF_PATH.tmp" "$SELF_PATH"
                log "updater script refreshed"
            fi
        else
            log "warning: updater checksum mismatch; keeping the installed updater"
        fi
    fi
fi

CURRENT=""
CURRENT="$("$BIN" --version 2>/dev/null | awk '{print $2}' || true)"
CURRENT_SHA="$(sha256sum "$BIN" | awk '{print $1}')"
NEW_SHA="$(sha256sum "$TMP_DIR/$ASSET" | awk '{print $1}')"

if [ "$CURRENT_SHA" = "$NEW_SHA" ]; then
    log "already running ${TAG}; nothing to do"
    exit 0
fi

if [ -n "$CURRENT" ]; then
    HIGHEST="$(printf '%s\n%s\n' "$CURRENT" "$LATEST" | sort -V | tail -n 1)"
    if [ "$HIGHEST" != "$LATEST" ]; then
        log "installed version ${CURRENT} is newer than ${LATEST}"
        exit 0
    fi
    if [ "$CURRENT" = "$LATEST" ]; then
        log "installed version is already ${CURRENT}"
        exit 0
    fi
fi

rollback() {
    if [ -f "$BIN.previous" ]; then
        mv -f "$BIN.previous" "$BIN"
        if command -v setcap >/dev/null 2>&1; then
            setcap cap_net_raw+ep "$BIN" 2>/dev/null || true
        fi
        systemctl restart "$SERVICE" >/dev/null 2>&1 || true
        log "rolled back to the previous binary"
    fi
}

cp -p "$BIN" "$BIN.previous"
install -m 0755 "$TMP_DIR/$ASSET" "$BIN.tmp"
mv -f "$BIN.tmp" "$BIN"

if command -v setcap >/dev/null 2>&1; then
    setcap cap_net_raw+ep "$BIN" 2>/dev/null || log "warning: cannot set CAP_NET_RAW"
fi

log "installed ${LATEST}; restarting ${SERVICE}"
if ! systemctl restart "$SERVICE"; then
    rollback
    die "service restart failed"
fi

sleep "$HEALTH_DELAY"
if ! systemctl is-active --quiet "$SERVICE"; then
    rollback
    die "service is not active after update"
fi

log "updated to ${LATEST}"
