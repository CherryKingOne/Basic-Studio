//! 运行时指标。
//!
//! 使用进程内计数器 + Prometheus 文本格式导出，避免引入重型依赖。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

#[derive(Debug, Default)]
pub struct Counters {
    pub requests_total: AtomicU64,
    pub requests_failed: AtomicU64,
    pub tokens_prompt: AtomicU64,
    pub tokens_completion: AtomicU64,
    pub audio_bytes: AtomicU64,
    pub engine_starts: AtomicU64,
    pub engine_failures: AtomicU64,
    pub queue_rejects: AtomicU64,
    /// TTFT 累计毫秒，配合 requests_total 计算均值。
    pub ttft_ms_total: AtomicU64,
    pub ttft_samples: AtomicU64,
}

impl Counters {
    pub fn inc_requests(&self) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_failed(&self) {
        self.requests_failed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_usage(&self, prompt: u64, completion: u64) {
        self.tokens_prompt.fetch_add(prompt, Ordering::Relaxed);
        self.tokens_completion
            .fetch_add(completion, Ordering::Relaxed);
    }

    pub fn add_audio(&self, bytes: u64) {
        self.audio_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn record_ttft(&self, ms: u64) {
        self.ttft_ms_total.fetch_add(ms, Ordering::Relaxed);
        self.ttft_samples.fetch_add(1, Ordering::Relaxed);
    }

    pub fn avg_ttft(&self) -> f64 {
        let samples = self.ttft_samples.load(Ordering::Relaxed);
        if samples == 0 {
            return 0.0;
        }
        self.ttft_ms_total.load(Ordering::Relaxed) as f64 / samples as f64
    }

    /// Prometheus 文本格式。
    pub fn render(&self) -> String {
        let mut out = String::new();
        let mut gauge = |name: &str, help: &str, value: u64| {
            out.push_str(&format!(
                "# HELP {name} {help}\n# TYPE {name} counter\n{name} {value}\n"
            ));
        };

        gauge(
            "studio_requests_total",
            "Total inference requests",
            self.requests_total.load(Ordering::Relaxed),
        );
        gauge(
            "studio_requests_failed_total",
            "Failed inference requests",
            self.requests_failed.load(Ordering::Relaxed),
        );
        gauge(
            "studio_tokens_prompt_total",
            "Prompt tokens",
            self.tokens_prompt.load(Ordering::Relaxed),
        );
        gauge(
            "studio_tokens_completion_total",
            "Completion tokens",
            self.tokens_completion.load(Ordering::Relaxed),
        );
        gauge(
            "studio_audio_bytes_total",
            "Audio bytes emitted",
            self.audio_bytes.load(Ordering::Relaxed),
        );
        gauge(
            "studio_engine_starts_total",
            "Engine start attempts",
            self.engine_starts.load(Ordering::Relaxed),
        );
        gauge(
            "studio_engine_failures_total",
            "Engine start failures",
            self.engine_failures.load(Ordering::Relaxed),
        );
        gauge(
            "studio_queue_rejects_total",
            "Rejected by scheduler",
            self.queue_rejects.load(Ordering::Relaxed),
        );

        out.push_str(&format!(
            "# HELP studio_ttft_ms_avg Average time to first token (ms)\n# TYPE studio_ttft_ms_avg gauge\nstudio_ttft_ms_avg {}\n",
            self.avg_ttft()
        ));

        out
    }
}

/// 全局指标实例。
pub fn metrics() -> &'static Counters {
    static METRICS: OnceLock<Counters> = OnceLock::new();
    METRICS.get_or_init(Counters::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_contains_metric_names() {
        let counters = Counters::default();
        counters.inc_requests();
        counters.record_ttft(12);
        let text = counters.render();
        assert!(text.contains("studio_requests_total 1"));
        assert!(text.contains("studio_ttft_ms_avg"));
    }

    #[test]
    fn avg_ttft_is_zero_without_samples() {
        assert_eq!(Counters::default().avg_ttft(), 0.0);
    }
}
