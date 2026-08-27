CREATE TABLE extended_workflow_token_budgets (
    root_thread_id TEXT PRIMARY KEY,
    base_limit_tokens INTEGER NOT NULL CHECK (base_limit_tokens >= 0),
    effective_limit_tokens INTEGER NOT NULL CHECK (effective_limit_tokens >= 0),
    workflow_tokens_used INTEGER NOT NULL DEFAULT 0 CHECK (workflow_tokens_used >= 0),
    session_tokens_used INTEGER NOT NULL DEFAULT 0 CHECK (session_tokens_used >= 0),
    active INTEGER NOT NULL DEFAULT 0 CHECK (active IN (0, 1))
);
