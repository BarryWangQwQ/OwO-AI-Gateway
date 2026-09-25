CREATE TABLE calls (
    id INTEGER PRIMARY KEY,
    started_at_ms INTEGER NOT NULL,
    request_id TEXT NOT NULL,
    client TEXT,
    requested_model TEXT NOT NULL,
    model TEXT,
    provider TEXT,
    upstream_model TEXT,
    stream INTEGER NOT NULL,
    status TEXT NOT NULL,
    error_kind TEXT,
    error_message TEXT,
    upstream_status INTEGER,
    duration_ms INTEGER NOT NULL,
    first_token_ms INTEGER,
    input_tokens INTEGER,
    cached_input_tokens INTEGER,
    cache_creation_input_tokens INTEGER,
    output_tokens INTEGER,
    reasoning_tokens INTEGER,
    stop_reason TEXT
);

CREATE INDEX calls_started ON calls(started_at_ms);
