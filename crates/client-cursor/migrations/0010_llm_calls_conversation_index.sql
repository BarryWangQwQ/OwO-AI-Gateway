-- Every agent turn looks up its conversation's latest usage (context accounting);
-- without this index that lookup scans the whole call history.
CREATE INDEX IF NOT EXISTS llm_calls_conversation ON llm_calls(conversation_id, created_at_ms DESC);
