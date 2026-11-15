# Pingushka

Pingushka is an open-source distributed monitoring system built around a central Hub and lightweight Linux agents (probes). Agents connect to the Hub over WebSocket and run checks from their own network locations.

The planned MVP includes HTTP(S), TCP-port and ICMP checks; a PostgreSQL-backed history; an authenticated web dashboard; one-time enrollment commands for individual agents; and a Docker Compose deployment with automatic HTTPS through Caddy.

**Status: early development.** The workspace, domain model, PostgreSQL schema/repositories, batch persistence, and cryptographic adapters are in place. The Hub exposes its authenticated REST API (first-run setup, sessions, agents with one-time enrollment, targets, history and summary), accepts agent results over WebSocket with acknowledgements, and prunes expired data. The Linux agent is implemented: scheduling with explicit timeouts, HTTP/TCP/ICMP probes with an explicit `tcp_fallback`, a bounded on-disk result buffer, batched delivery with acknowledgements, reconnect backoff with jitter and graceful shutdown. `install.sh` installs the agent and a systemd service. The dashboard and the application Docker image are not yet implemented or released.

## Supported agent platforms

- Linux x86_64
- Linux ARM64

Windows and macOS agents are out of scope for the MVP.

## Agent installation

Each agent is enrolled with its own one-time token. Create the agent in the Hub and run the generated command on the Linux host:

```sh
curl -fsSL https://raw.githubusercontent.com/vasilchenkome/pingushka/main/install.sh \
  | sudo sh -s -- --hub https://monitor.example.com --enrollment-token <token>
```

The installer downloads the release binary for x86_64 or ARM64, verifies its SHA-256 checksum, exchanges the enrollment token for a permanent credential, writes `/etc/pingushka/agent.toml` with mode `0600`, installs the `pingushka-agent.service` systemd unit, grants `CAP_NET_RAW` for ICMP and starts the service. Running the command again rotates the credential and restarts the service.

The result buffer lives in `/var/lib/pingushka/buffer.jsonl`. Logs: `journalctl -u pingushka-agent -f`.

## Development setup

Requirements: stable Rust toolchain and Docker Desktop with Docker Compose.

1. Copy `.env.example` to `.env`. Set a long random alphanumeric `POSTGRES_PASSWORD` and use the same value in `DATABASE_URL`.
2. Start the local PostgreSQL service:

   ```sh
   docker compose up -d postgres
   ```

3. Build and verify the Rust workspace:

   ```sh
   cargo build --workspace
   cargo test --workspace
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   ```

The local database port is bound to `127.0.0.1` only. Keep `.env` private; it is ignored by Git. `docker compose down -v` removes the local database volume and all its data.

The Hub reads `DATABASE_URL`, `BIND_ADDR`, `PUBLIC_URL`, `COOKIE_SECURE`, `SESSION_TTL_HOURS`, `ENROLLMENT_TTL_MINUTES`, `RETENTION_DAYS`, `MANUAL_CHECK_COOLDOWN_SECS` and `PRUNE_INTERVAL_MINUTES`.

## License

MIT. See [LICENSE](LICENSE).
