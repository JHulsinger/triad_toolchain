use anyhow::{Context, Result};
use sqlx::{sqlite::SqliteConnectOptions, ConnectOptions, SqlitePool};
use std::path::Path;

#[derive(Debug, Clone, sqlx::Type, serde::Serialize, serde::Deserialize)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskState {
    Pending,
    InProgress,
    Completed,
    Failed,
}

pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn new(db_path: &Path) -> Result<Self> {
        let _opt = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true)
            .connect()
            .await
            .context("Failed to connect to SQLite")?;

        let pool = SqlitePool::connect_with(
            SqliteConnectOptions::new()
                .filename(db_path)
        )
        .await
        .context("Failed to create connection pool")?;

        let db = Self { pool };
        db.init().await?;
        Ok(db)
    }

    async fn init(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                atomic_unit_id TEXT NOT NULL,
                state TEXT NOT NULL,
                code_rust TEXT,
                error_log TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )"
        )
        .execute(&self.pool)
        .await
        .context("Failed to create tasks table")?;

        Ok(())
    }

    pub async fn create_task(&self, id: &str, atomic_unit_id: &str) -> Result<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO tasks (id, atomic_unit_id, state) VALUES (?, ?, ?)"
        )
        .bind(id)
        .bind(atomic_unit_id)
        .bind(TaskState::Pending)
        .execute(&self.pool)
        .await
        .context("Failed to insert task")?;

        Ok(())
    }

    pub async fn update_task_state(&self, id: &str, state: TaskState, code: Option<&str>, error: Option<&str>) -> Result<()> {
        sqlx::query(
            "UPDATE tasks SET state = ?, code_rust = ?, error_log = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?"
        )
        .bind(state)
        .bind(code)
        .bind(error)
        .bind(id)
        .execute(&self.pool)
        .await
        .context("Failed to update task")?;

        Ok(())
    }

    pub async fn get_task_state(&self, id: &str) -> Result<Option<TaskState>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT state FROM tasks WHERE id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch task state")?;

        if let Some((state_str,)) = row {
            // Simple manual deserialization for brevity in this phase
            let state = match state_str.as_str() {
                "PENDING" => TaskState::Pending,
                "IN_PROGRESS" => TaskState::InProgress,
                "COMPLETED" => TaskState::Completed,
                "FAILED" => TaskState::Failed,
                _ => return Err(anyhow::anyhow!("Invalid task state in database")),
            };
            Ok(Some(state))
        } else {
            Ok(None)
        }
    }
}
