CREATE TABLE notification_channels (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('telegram', 'webhook', 'email')),
    config_encrypted BYTEA NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE alert_rules (
    id UUID PRIMARY KEY,
    target_id UUID NOT NULL UNIQUE REFERENCES targets(id) ON DELETE CASCADE,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    mode TEXT NOT NULL CHECK (mode IN ('all', 'any')),
    failure_threshold INTEGER NOT NULL CHECK (failure_threshold BETWEEN 1 AND 100),
    recovery_threshold INTEGER NOT NULL CHECK (recovery_threshold BETWEEN 1 AND 100),
    cooldown_secs INTEGER NOT NULL CHECK (cooldown_secs BETWEEN 0 AND 86400),
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX alert_rules_target_id_idx ON alert_rules (target_id);

CREATE TABLE alert_rule_channels (
    rule_id UUID NOT NULL REFERENCES alert_rules(id) ON DELETE CASCADE,
    channel_id UUID NOT NULL REFERENCES notification_channels(id) ON DELETE CASCADE,
    PRIMARY KEY (rule_id, channel_id)
);

CREATE TABLE alert_states (
    rule_id UUID PRIMARY KEY REFERENCES alert_rules(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('ok', 'pending', 'firing')),
    consecutive_failures INTEGER NOT NULL DEFAULT 0 CHECK (consecutive_failures >= 0),
    consecutive_successes INTEGER NOT NULL DEFAULT 0 CHECK (consecutive_successes >= 0),
    last_result_at TIMESTAMPTZ,
    last_notified_at TIMESTAMPTZ,
    opened_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE notification_outbox (
    id UUID PRIMARY KEY,
    dedupe_key TEXT NOT NULL UNIQUE,
    rule_id UUID NOT NULL REFERENCES alert_rules(id) ON DELETE CASCADE,
    channel_id UUID NOT NULL REFERENCES notification_channels(id) ON DELETE CASCADE,
    target_id UUID NOT NULL REFERENCES targets(id) ON DELETE CASCADE,
    event TEXT NOT NULL CHECK (event IN ('down', 'recovered')),
    payload JSONB NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'sending', 'sent', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_attempt_at TIMESTAMPTZ NOT NULL,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL,
    sent_at TIMESTAMPTZ
);

CREATE INDEX notification_outbox_due_idx
    ON notification_outbox (next_attempt_at)
    WHERE status IN ('pending', 'sending');

CREATE INDEX notification_outbox_created_idx ON notification_outbox (created_at DESC);
