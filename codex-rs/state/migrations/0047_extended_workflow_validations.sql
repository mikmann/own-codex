CREATE TABLE extended_workflow_validations (
    root_thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    validation_key TEXT NOT NULL,
    result_kind TEXT NOT NULL,
    agent_thread_id TEXT,
    reviewer_role TEXT,
    submissions INTEGER NOT NULL CHECK (submissions BETWEEN 1 AND 2),
    accepted INTEGER NOT NULL DEFAULT 0 CHECK (accepted IN (0, 1)),
    output_revision INTEGER NOT NULL,
    output_sha256 TEXT NOT NULL,
    reviewer_pass INTEGER NOT NULL DEFAULT 0 CHECK (reviewer_pass IN (0, 1)),
    review_cycles INTEGER NOT NULL DEFAULT 0 CHECK (review_cycles BETWEEN 0 AND 2),
    has_high_or_critical INTEGER NOT NULL DEFAULT 0 CHECK (has_high_or_critical IN (0, 1)),
    PRIMARY KEY (root_thread_id, turn_id, validation_key)
);

CREATE INDEX idx_extended_workflow_reviewer_validations
    ON extended_workflow_validations(root_thread_id, turn_id, result_kind, accepted);
