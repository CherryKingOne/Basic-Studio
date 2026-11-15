//! 准入控制：按模型限制并发，超出时排队或快速失败。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use studio_config::SchedulerConfig;
use tokio::sync::Semaphore;

/// 单个模型的并发许可。
pub struct Permit {
    _permit: tokio::sync::OwnedSemaphorePermit,
    model: String,
    queue_wait_ms: u64,
}

impl Permit {
    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn queue_wait_ms(&self) -> u64 {
        self.queue_wait_ms
    }
}

#[derive(Debug)]
pub struct AcquireError {
    pub model: String,
    pub timeout_ms: u64,
    pub queue_depth: usize,
}

impl std::fmt::Display for AcquireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "model `{}` is saturated: queue_depth={}, waited {}ms",
            self.model, self.queue_depth, self.timeout_ms
        )
    }
}

/// 调度器：每个模型一个信号量 + 队列深度统计。
pub struct Scheduler {
    max_concurrency: usize,
    queue_max_depth: usize,
    acquire_timeout: Duration,
    semaphores: Mutex<HashMap<String, Arc<Semaphore>>>,
    waiting: Mutex<HashMap<String, usize>>,
    rejected: AtomicU64,
}

impl Scheduler {
    pub fn new(config: &SchedulerConfig) -> Self {
        Self {
            max_concurrency: config.max_concurrency_per_model.max(1),
            queue_max_depth: config.queue_max_depth,
            acquire_timeout: Duration::from_secs(30),
            semaphores: Mutex::new(HashMap::new()),
            waiting: Mutex::new(HashMap::new()),
            rejected: AtomicU64::new(0),
        }
    }

    fn semaphore(&self, model: &str) -> Arc<Semaphore> {
        let mut map = self.semaphores.lock().expect("semaphores lock");
        map.entry(model.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(self.max_concurrency)))
            .clone()
    }

    /// 获取许可。队列满或超时返回错误。
    pub async fn acquire(&self, model: &str) -> Result<Permit, AcquireError> {
        let semaphore = self.semaphore(model);

        {
            let mut waiting = self.waiting.lock().expect("waiting lock");
            let depth = waiting.entry(model.to_string()).or_insert(0);
            if *depth > 0 && *depth >= self.queue_max_depth {
                self.rejected.fetch_add(1, Ordering::Relaxed);
                return Err(AcquireError {
                    model: model.to_string(),
                    timeout_ms: 0,
                    queue_depth: *depth,
                });
            }
            *depth += 1;
        }

        let start = std::time::Instant::now();
        let result =
            tokio::time::timeout(self.acquire_timeout, semaphore.clone().acquire_owned()).await;

        {
            let mut waiting = self.waiting.lock().expect("waiting lock");
            if let Some(depth) = waiting.get_mut(model) {
                *depth = depth.saturating_sub(1);
            }
        }

        match result {
            Ok(Ok(permit)) => Ok(Permit {
                _permit: permit,
                model: model.to_string(),
                queue_wait_ms: start.elapsed().as_millis() as u64,
            }),
            Ok(Err(_closed)) => Err(AcquireError {
                model: model.to_string(),
                timeout_ms: self.acquire_timeout.as_millis() as u64,
                queue_depth: 0,
            }),
            Err(_) => {
                self.rejected.fetch_add(1, Ordering::Relaxed);
                Err(AcquireError {
                    model: model.to_string(),
                    timeout_ms: self.acquire_timeout.as_millis() as u64,
                    queue_depth: self.queue_depth(model),
                })
            }
        }
    }

    pub fn queue_depth(&self, model: &str) -> usize {
        *self
            .waiting
            .lock()
            .expect("waiting lock")
            .get(model)
            .unwrap_or(&0)
    }

    pub fn rejected(&self) -> u64 {
        self.rejected.load(Ordering::Relaxed)
    }

    pub fn max_concurrency(&self) -> usize {
        self.max_concurrency
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scheduler(max: usize) -> Scheduler {
        Scheduler::new(&SchedulerConfig {
            max_concurrency_per_model: max,
            queue_max_depth: 16,
        })
    }

    #[tokio::test]
    async fn limits_concurrency_per_model() {
        let scheduler = Arc::new(scheduler(1));
        let first = scheduler.acquire("m").await.expect("first permit");

        let second = scheduler.clone();
        let handle = tokio::spawn(async move { second.acquire("m").await.map(|_| ()) });

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!handle.is_finished(), "second acquire must wait");
        assert_eq!(scheduler.queue_depth("m"), 1);

        drop(first);
        assert!(handle.await.unwrap().is_ok());
        assert_eq!(scheduler.queue_depth("m"), 0);
    }

    #[tokio::test]
    async fn rejects_when_queue_is_full() {
        let scheduler = Scheduler::new(&SchedulerConfig {
            max_concurrency_per_model: 1,
            queue_max_depth: 0,
        });
        let _held = scheduler.acquire("m").await.expect("permit");
        let err = match scheduler.acquire("m").await {
            Ok(_) => panic!("must reject"),
            Err(err) => err,
        };
        assert_eq!(err.queue_depth, 0);
        assert_eq!(scheduler.rejected(), 1);
    }

    #[tokio::test]
    async fn different_models_do_not_block_each_other() {
        let scheduler = scheduler(1);
        let _a = scheduler.acquire("a").await.expect("permit a");
        let _b = scheduler.acquire("b").await.expect("permit b");
    }
}
