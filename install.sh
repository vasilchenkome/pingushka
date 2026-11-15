#!/bin/sh
set -eu

HUB=""
ENROLLMENT_TOKEN=""
CONFIG_PATH="/etc/pingushka/agent.toml"
BUFFER_PATH="/var/lib/pingushka/buffer.jsonl"
BINARY_PATH="/usr/local/bin/pingushka-agent"
SERVICE_PATH="/etc/systemd/system/pingushka-agent.service"
SERVICE_USER="pingushka"
VERSION=""
BASE_URL="${PINGUSHKA_AGENT_BASE_URL:-}"

usage() {
    cat <<'EOF'
Usage: install.sh --hub <url> --enrollment-token <token> [options]

Options:
  --hub <url>                Hub public URL (http(s) or ws(s))
  --enrollment-token <token> One-time enrollment token for this agent
  --config <path>            Configuration path (default /etc/pingushka/agent.toml)
  --buffer <path>            Result buffer path (default /var/lib/pingushka/buffer.jsonl)
  --version <tag>            Release tag, for example v0.1.0
  --base-url <url>           Override the release download base URL
EOF
}

die() {
    echo "error: $*" >&2
    exit 1
}

require_value() {
    [ "$#" -ge 2 ] && [ -n "$2" ] || die "missing value for $1"
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --hub)
            require_value "$@"
            HUB="$2"
            shift 2
            ;;
        --enrollment-token)
            require_value "$@"
            ENROLLMENT_TOKEN="$2"
            shift 2
            ;;
        --config)
            require_value "$@"
            CONFIG_PATH="$2"
            shift 2
            ;;
        --buffer)
            require_value "$@"
            BUFFER_PATH="$2"
            shift 2
            ;;
        --version)
            require_value "$@"
            VERSION="$2"
            shift 2
            ;;
        --base-url)
            require_value "$@"
            BASE_URL="$2"
            shift 2
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            die "unknown option: $1"
            ;;
    esac
done

[ "$(id -u)" -eq 0 ] || die "run this installer as root"
[ -n "$HUB" ] || die "--hub is required"
if [ -z "$ENROLLMENT_TOKEN" ] && [ ! -f "$CONFIG_PATH" ]; then
    die "--enrollment-token is required for the first installation"
fi

command -v curl >/dev/null 2>&1 || die "curl is required"
command -v sha256sum >/dev/null 2>&1 || die "sha256sum is required"
command -v systemctl >/dev/null 2>&1 || die "systemd is required"

case "$(uname -m)" in
    x86_64|amd64)
        ARCH="x86_64"
        ;;
    aarch64|arm64)
        ARCH="aarch64"
        ;;
    *)
        die "unsupported architecture: $(uname -m)"
        ;;
esac

if [ -z "$BASE_URL" ]; then
    if [ -n "$VERSION" ]; then
        BASE_URL="https://github.com/vasilchenkome/pingushka/releases/download/${VERSION}"
    else
        BASE_URL="https://github.com/vasilchenkome/pingushka/releases/latest/download"
    fi
fi

ASSET="pingushka-agent-linux-${ARCH}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

echo "Downloading ${ASSET}"
curl -fsSL --retry 3 --retry-delay 2 "${BASE_URL}/${ASSET}" -o "${TMP_DIR}/${ASSET}" \
    || die "cannot download ${BASE_URL}/${ASSET}"
curl -fsSL --retry 3 --retry-delay 2 "${BASE_URL}/${ASSET}.sha256" -o "${TMP_DIR}/${ASSET}.sha256" \
    || die "cannot download checksum for ${ASSET}"

(
    cd "$TMP_DIR"
    sha256sum -c "${ASSET}.sha256" >/dev/null
) || die "checksum verification failed"

install -d -m 0755 "$(dirname "$BINARY_PATH")"
install -m 0755 "${TMP_DIR}/${ASSET}" "$BINARY_PATH"

if ! id -u "$SERVICE_USER" >/dev/null 2>&1; then
    useradd --system --home-dir /var/lib/pingushka --shell /usr/sbin/nologin "$SERVICE_USER"
fi

install -d -m 0750 -o "$SERVICE_USER" -g "$SERVICE_USER" /etc/pingushka
install -d -m 0750 -o "$SERVICE_USER" -g "$SERVICE_USER" "$(dirname "$BUFFER_PATH")"

if [ -n "$ENROLLMENT_TOKEN" ]; then
    echo "Enrolling agent with ${HUB}"
    "$BINARY_PATH" enroll \
        --hub "$HUB" \
        --enrollment-token "$ENROLLMENT_TOKEN" \
        --config "$CONFIG_PATH" \
        --buffer "$BUFFER_PATH"
fi

[ -f "$CONFIG_PATH" ] || die "configuration was not created at ${CONFIG_PATH}"
chown "$SERVICE_USER:$SERVICE_USER" "$CONFIG_PATH"
chmod 0600 "$CONFIG_PATH"

if command -v setcap >/dev/null 2>&1; then
    setcap cap_net_raw+ep "$BINARY_PATH" 2>/dev/null || true
fi

if [ -w /etc/sysctl.d ] && command -v sysctl >/dev/null 2>&1; then
    echo 'net.ipv4.ping_group_range=0 2147483647' >/etc/sysctl.d/99-pingushka.conf
    sysctl -q -p /etc/sysctl.d/99-pingushka.conf 2>/dev/null || true
fi

cat >"$SERVICE_PATH" <<EOF
[Unit]
Description=Pingushka monitoring agent
Documentation=https://github.com/vasilchenkome/pingushka
Wants=network-online.target
After=network-online.target

[Service]
Type=simple
User=${SERVICE_USER}
Group=${SERVICE_USER}
ExecStart=${BINARY_PATH} run --config ${CONFIG_PATH}
Restart=always
RestartSec=5
AmbientCapabilities=CAP_NET_RAW
CapabilityBoundingSet=CAP_NET_RAW
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
ReadWritePaths=$(dirname "$BUFFER_PATH")

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable "pingushka-agent.service" >/dev/null
systemctl restart "pingushka-agent.service"

echo "Pingushka agent installed and started."
echo "Status: systemctl status pingushka-agent"
echo "Logs:   journalctl -u pingushka-agent -f"
