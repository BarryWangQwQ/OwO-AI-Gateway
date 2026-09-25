//! Row accounting and cleanup for disposable observability data.

use serde::{Deserialize, Serialize};

use crate::Result;

use super::Store;

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct StatisticsStorage {
    pub call_count: i64,
    pub trace_count: i64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StatisticsStorageScope {
    #[default]
    Details,
    All,
}

impl Store {
    pub async fn statistics_storage(&self) -> Result<StatisticsStorage> {
        let (call_count, trace_count) = sqlx::query_as::<_, (i64, i64)>(
            "SELECT (SELECT COUNT(*) FROM llm_calls), (SELECT COUNT(*) FROM cursor_run_traces)",
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(StatisticsStorage {
            call_count,
            trace_count,
        })
    }

    /// Deletes finished call records older than `max_age` (their request/chunk rows go
    /// with them). Only the latest call of a live conversation is ever read back.
    pub async fn prune_llm_calls(&self, max_age: std::time::Duration) -> Result<u64> {
        let cutoff = super::now_ms().saturating_sub(max_age.as_millis().min(i64::MAX as u128) as i64);
        let _write = self.writes.lock().await;
        let deleted = sqlx::query("DELETE FROM llm_calls WHERE created_at_ms < ? AND status != 'running'")
            .bind(cutoff)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(deleted)
    }

    pub async fn clear_statistics_storage(&self) -> Result<StatisticsStorage> {
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await?;
        Self::clear_detail_storage_tx(&mut transaction).await?;
        transaction.commit().await?;
        self.statistics_storage().await
    }

    pub async fn clear_all_statistics_storage(&self) -> Result<StatisticsStorage> {
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await?;
        Self::clear_trace_artifacts_tx(&mut transaction).await?;
        sqlx::query("DELETE FROM llm_calls")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM cursor_run_traces")
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        self.statistics_storage().await
    }

    async fn clear_detail_storage_tx(
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ) -> Result<()> {
        sqlx::query("DELETE FROM llm_call_requests")
            .execute(&mut **transaction)
            .await?;
        sqlx::query("DELETE FROM llm_call_response_chunks")
            .execute(&mut **transaction)
            .await?;
        Self::clear_trace_artifacts_tx(transaction).await
    }

    async fn clear_trace_artifacts_tx(
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ) -> Result<()> {
        sqlx::query(
            "CREATE TEMP TABLE IF NOT EXISTS clear_statistics_blob_ids(
                blob_id BLOB PRIMARY KEY
             )",
        )
        .execute(&mut **transaction)
        .await?;
        sqlx::query("DELETE FROM clear_statistics_blob_ids")
            .execute(&mut **transaction)
            .await?;
        sqlx::query(
            "INSERT OR IGNORE INTO clear_statistics_blob_ids(blob_id)
             SELECT blob_id FROM cursor_run_trace_artifacts",
        )
        .execute(&mut **transaction)
        .await?;
        sqlx::query("DELETE FROM cursor_run_trace_artifacts")
            .execute(&mut **transaction)
            .await?;
        sqlx::query(
            "DELETE FROM blobs
             WHERE blob_id IN (SELECT blob_id FROM clear_statistics_blob_ids)
               AND NOT EXISTS (
                   SELECT 1 FROM cursor_run_trace_artifacts a WHERE a.blob_id = blobs.blob_id
               )
               AND NOT EXISTS (
                   SELECT 1 FROM blob_edges e
                   WHERE e.parent_blob_id = blobs.blob_id OR e.child_blob_id = blobs.blob_id
               )",
        )
        .execute(&mut **transaction)
        .await?;
        sqlx::query("DROP TABLE clear_statistics_blob_ids")
            .execute(&mut **transaction)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::model::{NewLlmCall, ProviderType};
    use crate::store::Store;

    fn call(id: &str) -> NewLlmCall {
        NewLlmCall {
            call_id: id.into(),
            run_id: "run".into(),
            conversation_id: "conversation".into(),
            provider_call_index: 0,
            model_hash: "m".into(),
            provider_type: ProviderType::Owo,
            provider_url: "owo://router".into(),
            request_type: ProviderType::Owo,
            request_url: "owo://router".into(),
            model_id: "m".into(),
            display_name: "m".into(),
            reasoning_effort: None,
            fast: false,
            message_count: 1,
            tool_count: 0,
            detailed: false,
        }
    }

    #[tokio::test]
    async fn prunes_only_old_finished_calls() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::connect(&format!("sqlite://{}", directory.path().join("t.db").display())).await.unwrap();
        for id in ["old-done", "old-running", "recent-done"] {
            store.start_llm_call(&call(id)).await.unwrap();
        }
        store.finish_llm_call("old-done", "completed", None, 1, None, None).await.unwrap();
        store.finish_llm_call("recent-done", "completed", None, 1, None, None).await.unwrap();
        sqlx::query("UPDATE llm_calls SET created_at_ms = 0 WHERE call_id LIKE 'old-%'").execute(store.pool()).await.unwrap();

        let deleted = store.prune_llm_calls(std::time::Duration::from_secs(3600)).await.unwrap();
        assert_eq!(deleted, 1);
        let left: Vec<String> = sqlx::query_scalar("SELECT call_id FROM llm_calls ORDER BY call_id").fetch_all(store.pool()).await.unwrap();
        assert_eq!(left, ["old-running", "recent-done"]);

        let indexed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = 'llm_calls_conversation')")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert!(indexed);
    }
}
