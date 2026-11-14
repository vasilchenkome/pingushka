# Pingushka

Pingushka is an open-source distributed monitoring system built around a central Hub and lightweight Linux agents (probes). Agents connect to the Hub over WebSocket and run checks from their own network locations.

The planned MVP includes HTTP(S), TCP-port and ICMP checks; a PostgreSQL-backed history; an authenticated web dashboard; one-time enrollment commands for individual agents; and a Docker Compose deployment with automatic HTTPS through Caddy.

**Status: early development.** The repository currently contains the workspace bootstrap only. The Hub, agents, dashboard, installer and deployment image are not yet implemented or released.

## Supported agent platforms

- Linux x86_64
- Linux ARM64

Windows and macOS agents are out of scope for the MVP.

## Development setup

Requirements: stable Rust toolchain and Docker Desktop with Docker Compose.

1. Copy `.env.example` to `.env` and replace `POSTGRES_PASSWORD` with a long random local password.
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

## License

MIT. See [LICENSE](LICENSE).
