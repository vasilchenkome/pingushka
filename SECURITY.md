# Security Policy

## Supported versions

Security fixes are provided for the latest `0.1.x` release. Older releases may be asked to upgrade.

## Reporting a vulnerability

Please do not open a public issue for a security problem.

Use GitHub's private vulnerability reporting:

<https://github.com/vasilchenkome/pingushka/security/advisories/new>

Include:

- affected version or image tag;
- deployment shape (Hub only, Hub with Caddy, agent installation);
- reproduction steps or a proof of concept;
- impact and any known mitigations;
- whether the report involves a real host or credentials.

Never include production passwords, agent credentials, tokens, private keys, or personal data. Redact logs and configuration before attaching them.

## What to expect

Reports are acknowledged on a best-effort basis, normally within a few days. Confirmed issues are fixed in a new release and disclosed after users have had a reasonable window to upgrade. Credit is given unless you ask to stay anonymous.

## Scope

In scope:

- the Hub HTTP/WebSocket API and session handling;
- the Linux agent, enrollment, credential storage and auto-update;
- `install.sh` and release artifacts;
- the Docker image and Compose deployment;
- authentication, authorization and tenant isolation defects.

Out of scope:

- vulnerabilities in PostgreSQL, Caddy, Docker or the host OS;
- denial of service caused by resource exhaustion on a host you do not control;
- findings that require an already compromised administrator account;
- reports from automated scanners without a reproducible impact.

## Hardening checklist

- Run the Hub behind Caddy with HTTPS and keep `COOKIE_SECURE=true`.
- Never expose PostgreSQL or the Hub port directly to the internet.
- Use a unique `POSTGRES_PASSWORD` and a strong administrator password.
- Keep the Docker image and agent releases up to date.
- Protect the repository `main` branch and release tags; see the repository rules.
- Do not commit `.env`, agent configuration, tokens or database dumps.
