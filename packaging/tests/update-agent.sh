#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
UPDATER="$ROOT/packaging/update-agent.sh"
TMP="$(mktemp -d)"
TMP_BIN="$TMP/stubs"
AGENT_DIR="$TMP/agent"
RELEASE_DIR="$TMP/release"
mkdir -p "$TMP_BIN" "$AGENT_DIR" "$RELEASE_DIR"

cleanup() {
    rm -rf "$TMP"
}
trap cleanup EXIT

cat >"$TMP_BIN/systemctl" <<'EOF'
#!/bin/sh
echo "$@" >> "${SYSTEMCTL_LOG:?}"
case "$1" in
    restart) [ "${FAIL_RESTART:-0}" = "1" ] && exit 1 ;;
    is-active) [ "${FAIL_ACTIVE:-0}" = "1" ] && exit 3 ;;
esac
exit 0
EOF
cat >"$TMP_BIN/setcap" <<'EOF'
#!/bin/sh
exit 0
EOF
chmod +x "$TMP_BIN/systemctl" "$TMP_BIN/setcap"

export PATH="$TMP_BIN:$PATH"
export PINGUSHKA_UPDATE_ALLOW_NONROOT=1
export PINGUSHKA_UPDATE_LOCK="$TMP/update.lock"
export PINGUSHKA_UPDATE_BIN="$AGENT_DIR/pingushka-agent"
export PINGUSHKA_UPDATE_SERVICE="pingushka-test.service"
export PINGUSHKA_UPDATE_HEALTH_DELAY=0
export SYSTEMCTL_LOG="$TMP/systemctl.log"
export PINGUSHKA_UPDATE_TAG="v0.1.3"
if command -v cygpath >/dev/null 2>&1; then
    export PINGUSHKA_UPDATE_DOWNLOAD_BASE="file:///$(cygpath -m "$RELEASE_DIR")"
else
    export PINGUSHKA_UPDATE_DOWNLOAD_BASE="file://$RELEASE_DIR"
fi

write_agent() {
    local path="$1" version="$2"
    cat >"$path" <<EOF
#!/bin/sh
if [ "\${1:-}" = "--version" ]; then
    echo "pingushka-agent $version"
fi
EOF
    chmod +x "$path"
}

release() {
    local version="$1"
    local arch asset
    case "$(uname -m)" in
        x86_64|amd64) arch=x86_64 ;;
        aarch64|arm64) arch=aarch64 ;;
        *) echo "unsupported host" >&2; exit 1 ;;
    esac
    asset="pingushka-agent-linux-$arch"
    rm -rf "$RELEASE_DIR"
    mkdir -p "$RELEASE_DIR"
    write_agent "$RELEASE_DIR/$asset" "$version"
    (cd "$RELEASE_DIR" && sha256sum "$asset" > "$asset.sha256")
}

expect_failure() {
    if "$@" >"$TMP/out.log" 2>&1; then
        echo "expected failure: $*" >&2
        cat "$TMP/out.log" >&2
        exit 1
    fi
}

# 1. Update to a newer release.
write_agent "$PINGUSHKA_UPDATE_BIN" "0.1.2"
release "0.1.3"
if ! sh "$UPDATER" >"$TMP/out.log" 2>&1; then
    cat "$TMP/out.log" >&2
    exit 1
fi
"$PINGUSHKA_UPDATE_BIN" --version | grep -q "0.1.3"
grep -q "restart pingushka-test.service" "$SYSTEMCTL_LOG"

# 2. Same checksum is a no-op.
: >"$SYSTEMCTL_LOG"
sh "$UPDATER" >"$TMP/out.log"
[ ! -s "$SYSTEMCTL_LOG" ]

# 3. A bad checksum must not replace the binary.
write_agent "$PINGUSHKA_UPDATE_BIN" "0.1.2"
release "0.1.3"
echo "0000000000000000000000000000000000000000000000000000000000000000" >"$RELEASE_DIR"/*.sha256
expect_failure sh "$UPDATER"
"$PINGUSHKA_UPDATE_BIN" --version | grep -q "0.1.2"

# 4. A failed restart rolls back to the previous binary.
write_agent "$PINGUSHKA_UPDATE_BIN" "0.1.2"
release "0.1.3"
: >"$SYSTEMCTL_LOG"
FAIL_RESTART=1 expect_failure sh "$UPDATER"
"$PINGUSHKA_UPDATE_BIN" --version | grep -q "0.1.2"

# 5. A held lock skips the run.
write_agent "$PINGUSHKA_UPDATE_BIN" "0.1.2"
release "0.1.3"
flock "$PINGUSHKA_UPDATE_LOCK" -c "sleep 2" &
LOCK_PID=$!
sleep 0.2
sh "$UPDATER" >"$TMP/out.log"
kill "$LOCK_PID" 2>/dev/null || true
wait "$LOCK_PID" 2>/dev/null || true
grep -q "another update" "$TMP/out.log"
"$PINGUSHKA_UPDATE_BIN" --version | grep -q "0.1.2"

echo "update-agent tests passed"
