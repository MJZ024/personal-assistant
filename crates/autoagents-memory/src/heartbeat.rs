//! Heartbeat scheduler for periodic tasks.

use std::time::Duration;
use tokio::time;

use super::Database;

/// Configuration for the heartbeat scheduler.
#[derive(Debug, Clone)]
pub struct HeartbeatConfig {
    pub interval_secs: u64,
    pub wal_checkpoint_interval_secs: u64,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            interval_secs: 300,
            wal_checkpoint_interval_secs: 3600,
        }
    }
}

/// Heartbeat scheduler that runs periodic checks.
pub struct Heartbeat {
    db: std::sync::Arc<Database>,
    config: HeartbeatConfig,
    /// Callback when heartbeat fires. Should return tasks needing attention.
    on_tick: Option<Box<dyn Fn(&[super::TaskRecord]) + Send + Sync>>,
}

impl Heartbeat {
    pub fn new(db: std::sync::Arc<Database>, config: HeartbeatConfig) -> Self {
        Self {
            db,
            config,
            on_tick: None,
        }
    }

    /// Register a callback for each heartbeat tick.
    pub fn on_tick<F>(mut self, f: F) -> Self
    where
        F: Fn(&[super::TaskRecord]) + Send + Sync + 'static,
    {
        self.on_tick = Some(Box::new(f));
        self
    }

    /// Start the heartbeat loop. Runs until cancelled.
    pub async fn run(&self) {
        let mut last_wal_checkpoint = time::Instant::now();
        let wal_interval = Duration::from_secs(self.config.wal_checkpoint_interval_secs);

        loop {
            time::sleep(Duration::from_secs(self.config.interval_secs)).await;

            // Log heartbeat
            let _ = self.db.log_heartbeat("heartbeat_tick", None);

            // Check tasks
            if let Ok(tasks) = self.db.get_active_tasks() {
                if let Some(ref callback) = self.on_tick {
                    callback(&tasks);
                }
            }

            // WAL checkpoint on interval
            if last_wal_checkpoint.elapsed() >= wal_interval {
                let _ = self.db.wal_checkpoint();
                let _ = self.db.log_heartbeat("wal_checkpoint", Some("automatic"));
                last_wal_checkpoint = time::Instant::now();
            }
        }
    }
}
