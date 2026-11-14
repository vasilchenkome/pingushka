CREATE TABLE users (
    id UUID PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE sessions (
    token_hash BYTEA PRIMARY KEY CHECK (octet_length(token_hash) = 32),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL CHECK (expires_at > created_at)
);

CREATE INDEX sessions_expires_at_idx ON sessions (expires_at);

CREATE TABLE agents (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    token_hash BYTEA UNIQUE CHECK (token_hash IS NULL OR octet_length(token_hash) = 32),
    created_at TIMESTAMPTZ NOT NULL,
    last_seen_at TIMESTAMPTZ,
    connected BOOLEAN NOT NULL DEFAULT FALSE,
    version TEXT
);

CREATE TABLE agent_enrollments (
    id UUID PRIMARY KEY,
    agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    token_hash BYTEA NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX agent_enrollments_expires_at_idx ON agent_enrollments (expires_at);

CREATE TABLE targets (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('http', 'tcp', 'icmp')),
    spec_json JSONB NOT NULL,
    interval_secs INTEGER NOT NULL CHECK (interval_secs BETWEEN 5 AND 3600),
    timeout_ms INTEGER NOT NULL CHECK (timeout_ms BETWEEN 100 AND 30000),
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    CHECK (timeout_ms < interval_secs * 1000)
);

CREATE TABLE target_agents (
    target_id UUID NOT NULL REFERENCES targets(id) ON DELETE CASCADE,
    agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    assigned_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (target_id, agent_id)
);

CREATE INDEX target_agents_agent_id_idx ON target_agents (agent_id, target_id);

CREATE TABLE check_results (
    result_id UUID PRIMARY KEY,
    target_id UUID NOT NULL REFERENCES targets(id) ON DELETE CASCADE,
    agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    checked_at TIMESTAMPTZ NOT NULL,
    ok BOOLEAN NOT NULL,
    latency_ms INTEGER NOT NULL CHECK (latency_ms >= 0),
    status_code SMALLINT CHECK (status_code IS NULL OR status_code BETWEEN 100 AND 599),
    check_mode TEXT NOT NULL CHECK (check_mode IN ('http', 'tcp', 'icmp', 'tcp_fallback')),
    error_kind TEXT,
    error_message TEXT
);

CREATE INDEX check_results_target_time_idx ON check_results (target_id, checked_at DESC);
CREATE INDEX check_results_agent_time_idx ON check_results (agent_id, checked_at DESC);
CREATE INDEX check_results_target_agent_time_idx ON check_results (target_id, agent_id, checked_at DESC);
CREATE INDEX check_results_checked_at_idx ON check_results (checked_at);
