CREATE TABLE target_groups (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE UNIQUE INDEX target_groups_name_lower_idx ON target_groups (lower(name));

ALTER TABLE targets ADD COLUMN group_id UUID
    REFERENCES target_groups(id) ON DELETE SET NULL;

CREATE INDEX targets_group_id_idx ON targets (group_id);

WITH prefixes AS (
    SELECT DISTINCT trim(substring(name FROM '^\[([^\]]+)\]')) AS name
    FROM targets
    WHERE name ~ '^\[[^\]]+\]'
      AND trim(substring(name FROM '^\[([^\]]+)\]')) <> ''
)
INSERT INTO target_groups (id, name, created_at, updated_at)
SELECT gen_random_uuid(), name, now(), now() FROM prefixes;

UPDATE targets t
SET group_id = g.id
FROM target_groups g
WHERE lower(trim(substring(t.name FROM '^\[([^\]]+)\]'))) = lower(g.name);

CREATE TABLE target_tags (
    target_id UUID NOT NULL REFERENCES targets(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    PRIMARY KEY (target_id, tag)
);

CREATE INDEX target_tags_tag_idx ON target_tags (tag);

ALTER TABLE alert_rules ADD COLUMN group_id UUID
    REFERENCES target_groups(id) ON DELETE CASCADE;

ALTER TABLE alert_rules ALTER COLUMN target_id DROP NOT NULL;

ALTER TABLE alert_rules
    ADD CONSTRAINT alert_rules_one_scope
    CHECK ((target_id IS NULL) <> (group_id IS NULL));

CREATE UNIQUE INDEX alert_rules_group_id_idx
    ON alert_rules (group_id)
    WHERE group_id IS NOT NULL;

CREATE TABLE alert_subject_states (
    rule_id UUID NOT NULL REFERENCES alert_rules(id) ON DELETE CASCADE,
    subject_kind TEXT NOT NULL CHECK (subject_kind IN ('target', 'group')),
    subject_id UUID NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('ok', 'pending', 'firing')),
    consecutive_failures INTEGER NOT NULL DEFAULT 0 CHECK (consecutive_failures >= 0),
    consecutive_successes INTEGER NOT NULL DEFAULT 0 CHECK (consecutive_successes >= 0),
    last_result_at TIMESTAMPTZ,
    last_notified_at TIMESTAMPTZ,
    opened_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (rule_id, subject_kind, subject_id)
);

INSERT INTO alert_subject_states (
    rule_id, subject_kind, subject_id, status, consecutive_failures,
    consecutive_successes, last_result_at, last_notified_at, opened_at, updated_at
)
SELECT s.rule_id, 'target', r.target_id, s.status, s.consecutive_failures,
       s.consecutive_successes, s.last_result_at, s.last_notified_at,
       s.opened_at, s.updated_at
FROM alert_states s
JOIN alert_rules r ON r.id = s.rule_id
WHERE r.target_id IS NOT NULL;

ALTER TABLE notification_outbox ALTER COLUMN target_id DROP NOT NULL;
ALTER TABLE notification_outbox ADD COLUMN group_id UUID
    REFERENCES target_groups(id) ON DELETE CASCADE;

ALTER TABLE notification_outbox
    ADD CONSTRAINT notification_outbox_one_scope
    CHECK ((target_id IS NULL) <> (group_id IS NULL));
