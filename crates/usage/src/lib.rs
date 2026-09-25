//! Call history and token usage in `state/usage.db`. The gateway appends one row per
//! routed call through [`channel`] and [`write_all`]; the CLI reads them back.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use owo_routing::{CallRecord, CallSink};
use sqlx::Row;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteRow};
use tokio::sync::mpsc;

pub const FILE_NAME: &str = "usage.db";

pub type Result<T> = std::result::Result<T, sqlx::Error>;

#[derive(Clone)]
pub struct UsageLog {
    pool: SqlitePool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupBy {
    Model,
    App,
    Provider,
    Day,
}

impl GroupBy {
    fn key(self) -> &'static str {
        match self {
            GroupBy::Model => "COALESCE(model, requested_model)",
            GroupBy::App => "COALESCE(client, '-')",
            GroupBy::Provider => "COALESCE(provider, '-')",
            GroupBy::Day => "date(started_at_ms / 1000, 'unixepoch', 'localtime')",
        }
    }
}

/// Call counts and token totals for one group (or for everything, with an empty key).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Summary {
    pub key: String,
    pub calls: u64,
    pub failed: u64,
    pub cancelled: u64,
    /// Includes cached input.
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    /// Includes reasoning.
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    /// Estimated dollars over the calls whose model had a price; `None` when none did.
    pub cost_usd: Option<f64>,
    /// Calls that used tokens but had no price, so `cost_usd` leaves them out.
    pub unpriced: u64,
}

impl Summary {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }

    pub fn add(&mut self, other: &Summary) {
        self.calls += other.calls;
        self.failed += other.failed;
        self.cancelled += other.cancelled;
        self.input_tokens += other.input_tokens;
        self.cached_input_tokens += other.cached_input_tokens;
        self.cache_creation_input_tokens += other.cache_creation_input_tokens;
        self.output_tokens += other.output_tokens;
        self.reasoning_tokens += other.reasoning_tokens;
        self.cost_usd = match (self.cost_usd, other.cost_usd) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
        };
        self.unpriced += other.unpriced;
    }
}

#[derive(Debug, Clone, Default)]
pub struct CallFilter {
    pub failed_only: bool,
    /// Matches the routed model id or the id the client asked for.
    pub model: Option<String>,
    pub client: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct StoredCall {
    pub id: i64,
    /// Local time, `YYYY-MM-DD HH:MM:SS`.
    pub time: String,
    pub request_id: String,
    pub client: Option<String>,
    pub requested_model: String,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub upstream_model: Option<String>,
    pub stream: bool,
    /// `ok`, `error`, or `cancelled`.
    pub status: String,
    pub error_kind: Option<String>,
    pub error_message: Option<String>,
    pub upstream_status: Option<u16>,
    pub duration_ms: u64,
    pub first_token_ms: Option<u64>,
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub stop_reason: Option<String>,
}

const SUMS: &str = "COUNT(*) AS calls,
    COALESCE(SUM(status = 'error'), 0) AS failed,
    COALESCE(SUM(status = 'cancelled'), 0) AS cancelled,
    COALESCE(SUM(input_tokens), 0) AS input_tokens,
    COALESCE(SUM(cached_input_tokens), 0) AS cached_input_tokens,
    COALESCE(SUM(cache_creation_input_tokens), 0) AS cache_creation_input_tokens,
    COALESCE(SUM(output_tokens), 0) AS output_tokens,
    COALESCE(SUM(reasoning_tokens), 0) AS reasoning_tokens,
    SUM(cost_usd) AS cost_usd,
    COALESCE(SUM(cost_usd IS NULL AND COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0) > 0), 0) AS unpriced";

/// Epoch milliseconds of local midnight, `?` days back (a modifier such as `-6 days`).
const LOCAL_DAY_START_MS: &str = "CAST(strftime('%s', 'now', 'localtime', 'start of day', ?, 'utc') AS INTEGER) * 1000";

const CALL_COLUMNS: &str = "id, strftime('%Y-%m-%d %H:%M:%S', started_at_ms / 1000, 'unixepoch', 'localtime') AS time,
    request_id, client, requested_model, model, provider, upstream_model, stream, status, error_kind,
    error_message, upstream_status, duration_ms, first_token_ms, input_tokens, cached_input_tokens,
    cache_creation_input_tokens, output_tokens, reasoning_tokens, cost_usd, stop_reason";

impl UsageLog {
    pub async fn open(path: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new().max_connections(2).connect_with(options).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn insert(&self, call: &CallRecord) -> Result<()> {
        let usage = call.usage.as_ref();
        let error = call.error.as_ref();
        sqlx::query(
            "INSERT INTO calls (started_at_ms, request_id, client, requested_model, model, provider, upstream_model,
                stream, status, error_kind, error_message, upstream_status, duration_ms, first_token_ms, input_tokens,
                cached_input_tokens, cache_creation_input_tokens, output_tokens, reasoning_tokens, cost_usd, stop_reason)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(call.started_at_ms)
        .bind(&call.request_id)
        .bind(&call.client)
        .bind(&call.requested_model)
        .bind(&call.model)
        .bind(&call.provider)
        .bind(&call.upstream_model)
        .bind(call.stream)
        .bind(call.status.as_str())
        .bind(error.map(|e| e.kind.as_str()))
        .bind(error.map(|e| e.message.as_str()))
        .bind(error.and_then(|e| e.upstream_status))
        .bind(to_i64(call.duration_ms))
        .bind(call.first_token_ms.map(to_i64))
        .bind(usage.map(|u| to_i64(u.input_tokens)))
        .bind(usage.and_then(|u| u.cached_input_tokens).map(to_i64))
        .bind(usage.and_then(|u| u.cache_creation_input_tokens).map(to_i64))
        .bind(usage.map(|u| to_i64(u.output_tokens)))
        .bind(usage.and_then(|u| u.reasoning_tokens).map(to_i64))
        .bind(call.cost_usd)
        .bind(&call.stop_reason)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Deletes calls older than `max_age`.
    pub async fn prune(&self, max_age: Duration) -> Result<u64> {
        let cutoff_ms = now_ms() - to_i64(max_age.as_millis().min(u64::MAX as u128) as u64);
        Ok(sqlx::query("DELETE FROM calls WHERE started_at_ms < ?").bind(cutoff_ms).execute(&self.pool).await?.rows_affected())
    }

    /// Deletes every call and returns how many there were. The file is compacted afterwards
    /// when nobody else holds it open; a failure there does not fail the clear.
    pub async fn clear(&self) -> Result<u64> {
        let deleted = sqlx::query("DELETE FROM calls").execute(&self.pool).await?.rows_affected();
        if let Err(error) = sqlx::query("VACUUM").execute(&self.pool).await {
            tracing::debug!(%error, "could not compact usage.db after clearing it");
        }
        Ok(deleted)
    }

    /// Totals per group over today and the `days - 1` days before it (local time).
    pub async fn summary(&self, days: u32, by: GroupBy) -> Result<Vec<Summary>> {
        let order = if by == GroupBy::Day { "key" } else { "SUM(input_tokens) + SUM(output_tokens) DESC, calls DESC" };
        let sql = format!(
            "SELECT {key} AS key, {SUMS} FROM calls WHERE started_at_ms >= {LOCAL_DAY_START_MS} GROUP BY 1 ORDER BY {order}",
            key = by.key()
        );
        let rows = sqlx::query(&sql).bind(days_back(days)).fetch_all(&self.pool).await?;
        rows.iter().map(summary).collect()
    }

    /// The local date the `days`-day period starts on (`YYYY-MM-DD`).
    pub async fn period_start(&self, days: u32) -> Result<String> {
        sqlx::query_scalar("SELECT date('now', 'localtime', 'start of day', ?)").bind(days_back(days)).fetch_one(&self.pool).await
    }

    /// Totals since local midnight.
    pub async fn today(&self) -> Result<Summary> {
        let sql = format!("SELECT '' AS key, {SUMS} FROM calls WHERE started_at_ms >= {LOCAL_DAY_START_MS}");
        summary(&sqlx::query(&sql).bind(days_back(1)).fetch_one(&self.pool).await?)
    }

    /// The most recent calls first.
    pub async fn calls(&self, filter: &CallFilter) -> Result<Vec<StoredCall>> {
        let sql = format!(
            "SELECT {CALL_COLUMNS} FROM calls
             WHERE (?1 = 0 OR status = 'error')
               AND (?2 IS NULL OR model = ?2 OR requested_model = ?2)
               AND (?3 IS NULL OR client = ?3)
             ORDER BY id DESC LIMIT ?4"
        );
        let rows = sqlx::query(&sql)
            .bind(filter.failed_only)
            .bind(&filter.model)
            .bind(&filter.client)
            .bind(filter.limit)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(stored_call).collect()
    }

    pub async fn call(&self, id: i64) -> Result<Option<StoredCall>> {
        let sql = format!("SELECT {CALL_COLUMNS} FROM calls WHERE id = ?");
        sqlx::query(&sql).bind(id).fetch_optional(&self.pool).await?.as_ref().map(stored_call).transpose()
    }
}

/// The sink handed to the router: queues records for [`write_all`] without blocking.
pub struct Recorder(mpsc::UnboundedSender<CallRecord>);

impl CallSink for Recorder {
    fn record(&self, call: CallRecord) {
        let _ = self.0.send(call);
    }
}

pub fn channel() -> (Arc<Recorder>, mpsc::UnboundedReceiver<CallRecord>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (Arc::new(Recorder(tx)), rx)
}

/// Writes queued records until every [`Recorder`] is gone.
pub async fn write_all(log: UsageLog, mut calls: mpsc::UnboundedReceiver<CallRecord>) {
    while let Some(call) = calls.recv().await {
        if let Err(error) = log.insert(&call).await {
            tracing::warn!(%error, "could not record a model call");
        }
    }
}

fn days_back(days: u32) -> String {
    format!("-{} days", days.max(1) - 1)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or_default()
}

fn to_i64(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn to_u64(n: i64) -> u64 {
    u64::try_from(n).unwrap_or(0)
}

fn summary(row: &SqliteRow) -> Result<Summary> {
    let n = |column: &str| row.try_get::<i64, _>(column).map(to_u64);
    Ok(Summary {
        key: row.try_get::<Option<String>, _>("key")?.unwrap_or_default(),
        calls: n("calls")?,
        failed: n("failed")?,
        cancelled: n("cancelled")?,
        input_tokens: n("input_tokens")?,
        cached_input_tokens: n("cached_input_tokens")?,
        cache_creation_input_tokens: n("cache_creation_input_tokens")?,
        output_tokens: n("output_tokens")?,
        reasoning_tokens: n("reasoning_tokens")?,
        cost_usd: row.try_get("cost_usd")?,
        unpriced: n("unpriced")?,
    })
}

fn stored_call(row: &SqliteRow) -> Result<StoredCall> {
    let opt = |column: &str| row.try_get::<Option<i64>, _>(column).map(|v| v.map(to_u64));
    Ok(StoredCall {
        id: row.try_get("id")?,
        time: row.try_get("time")?,
        request_id: row.try_get("request_id")?,
        client: row.try_get("client")?,
        requested_model: row.try_get("requested_model")?,
        model: row.try_get("model")?,
        provider: row.try_get("provider")?,
        upstream_model: row.try_get("upstream_model")?,
        stream: row.try_get("stream")?,
        status: row.try_get("status")?,
        error_kind: row.try_get("error_kind")?,
        error_message: row.try_get("error_message")?,
        upstream_status: opt("upstream_status")?.and_then(|s| u16::try_from(s).ok()),
        duration_ms: to_u64(row.try_get("duration_ms")?),
        first_token_ms: opt("first_token_ms")?,
        input_tokens: opt("input_tokens")?,
        cached_input_tokens: opt("cached_input_tokens")?,
        cache_creation_input_tokens: opt("cache_creation_input_tokens")?,
        output_tokens: opt("output_tokens")?,
        reasoning_tokens: opt("reasoning_tokens")?,
        cost_usd: row.try_get("cost_usd")?,
        stop_reason: row.try_get("stop_reason")?,
    })
}

#[cfg(test)]
mod tests {
    use owo_core::Usage;
    use owo_routing::{CallError, CallStatus};

    use super::*;

    fn call(model: &str, client: &str, status: CallStatus, input: u64, output: u64) -> CallRecord {
        CallRecord {
            started_at_ms: now_ms(),
            request_id: "r".into(),
            client: Some(client.into()),
            requested_model: model.into(),
            model: Some(model.into()),
            provider: Some("p".into()),
            upstream_model: Some(model.into()),
            stream: true,
            status,
            error: (status == CallStatus::Error).then(|| CallError {
                kind: "rate_limited".into(),
                message: "slow down".into(),
                upstream_status: Some(429),
            }),
            duration_ms: 1200,
            first_token_ms: Some(300),
            usage: (input + output > 0).then(|| Usage {
                input_tokens: input,
                output_tokens: output,
                cached_input_tokens: Some(input / 2),
                ..Default::default()
            }),
            // Model `c` has no price.
            cost_usd: (model != "c" && input + output > 0).then(|| (input + output) as f64 / 100.0),
            stop_reason: None,
        }
    }

    async fn log() -> (tempfile::TempDir, UsageLog) {
        let dir = tempfile::tempdir().unwrap();
        let log = UsageLog::open(&dir.path().join(FILE_NAME)).await.unwrap();
        (dir, log)
    }

    #[tokio::test]
    async fn summarizes_by_model_and_app() {
        let (_dir, log) = log().await;
        log.insert(&call("a", "codex", CallStatus::Ok, 100, 10)).await.unwrap();
        log.insert(&call("a", "cursor", CallStatus::Ok, 200, 20)).await.unwrap();
        log.insert(&call("b", "codex", CallStatus::Error, 0, 0)).await.unwrap();

        let by_model = log.summary(7, GroupBy::Model).await.unwrap();
        assert_eq!(by_model[0].key, "a");
        assert_eq!((by_model[0].calls, by_model[0].input_tokens, by_model[0].cached_input_tokens), (2, 300, 150));
        assert_eq!((by_model[1].key.as_str(), by_model[1].failed, by_model[1].total_tokens()), ("b", 1, 0));

        let by_app = log.summary(7, GroupBy::App).await.unwrap();
        assert_eq!(by_app.iter().map(|s| s.key.as_str()).collect::<Vec<_>>(), ["cursor", "codex"]);
        assert_eq!(log.summary(1, GroupBy::Day).await.unwrap().len(), 1);

        let today = log.today().await.unwrap();
        assert_eq!((today.calls, today.failed, today.output_tokens), (3, 1, 30));
        let dollars = |s: &Summary| (s.cost_usd.unwrap() * 100.0).round() / 100.0;
        assert_eq!((dollars(&today), today.unpriced), (3.3, 0));
        assert_eq!(by_model[1].cost_usd, None, "an error without tokens costs nothing");

        log.insert(&call("c", "codex", CallStatus::Ok, 50, 5)).await.unwrap();
        let today = log.today().await.unwrap();
        assert_eq!((dollars(&today), today.unpriced), (3.3, 1));
    }

    #[tokio::test]
    async fn lists_and_filters_calls() {
        let (_dir, log) = log().await;
        log.insert(&call("a", "codex", CallStatus::Ok, 1, 1)).await.unwrap();
        log.insert(&call("b", "claude_code", CallStatus::Error, 0, 0)).await.unwrap();
        log.insert(&call("a", "cursor", CallStatus::Cancelled, 0, 0)).await.unwrap();

        let all = log.calls(&CallFilter { limit: 10, ..Default::default() }).await.unwrap();
        assert_eq!(all.iter().map(|c| c.status.as_str()).collect::<Vec<_>>(), ["cancelled", "error", "ok"]);
        let failed = log.calls(&CallFilter { failed_only: true, limit: 10, ..Default::default() }).await.unwrap();
        assert_eq!(failed.len(), 1);
        assert_eq!((failed[0].error_kind.as_deref(), failed[0].upstream_status), (Some("rate_limited"), Some(429)));
        let codex_a = CallFilter { model: Some("a".into()), client: Some("codex".into()), limit: 10, ..Default::default() };
        assert_eq!(log.calls(&codex_a).await.unwrap().len(), 1);

        let one = log.call(failed[0].id).await.unwrap().unwrap();
        assert_eq!(one.error_message.as_deref(), Some("slow down"));
        assert!(log.call(9999).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn prunes_old_calls_and_writes_through_the_channel() {
        let (_dir, log) = log().await;
        let mut old = call("a", "codex", CallStatus::Ok, 1, 1);
        old.started_at_ms -= 100 * 24 * 3600 * 1000;
        log.insert(&old).await.unwrap();
        assert_eq!(log.prune(Duration::from_secs(90 * 24 * 3600)).await.unwrap(), 1);

        let (recorder, rx) = channel();
        recorder.record(call("b", "codex", CallStatus::Ok, 5, 5));
        drop(recorder);
        write_all(log.clone(), rx).await;
        assert_eq!(log.calls(&CallFilter { limit: 10, ..Default::default() }).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn clears_every_call_and_keeps_recording() {
        let (_dir, log) = log().await;
        assert_eq!(log.clear().await.unwrap(), 0, "an empty log has nothing to clear");
        log.insert(&call("a", "codex", CallStatus::Ok, 100, 10)).await.unwrap();
        log.insert(&call("b", "cursor", CallStatus::Error, 0, 0)).await.unwrap();
        let mut old = call("c", "codex", CallStatus::Ok, 1, 1);
        old.started_at_ms -= 400 * 24 * 3600 * 1000;
        log.insert(&old).await.unwrap();

        assert_eq!(log.clear().await.unwrap(), 3, "old calls go too, unlike prune");
        assert!(log.calls(&CallFilter { limit: 10, ..Default::default() }).await.unwrap().is_empty());
        let today = log.today().await.unwrap();
        assert_eq!((today.calls, today.cost_usd), (0, None));

        // The log stays usable after the VACUUM.
        log.insert(&call("a", "codex", CallStatus::Ok, 5, 5)).await.unwrap();
        assert_eq!(log.today().await.unwrap().calls, 1);
    }
}
