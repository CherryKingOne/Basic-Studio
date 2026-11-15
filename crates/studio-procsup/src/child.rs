use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use studio_core::{Error, Result};
use tokio::process::{Child, Command};

use crate::signal;

/// 轮询一个 HTTP 端点直到成功或超时，不依赖具体子进程。
///
/// 用于"外部托管"的引擎（如 docker compose 里的 vLLM 容器）。
pub async fn probe_http(url: &str, timeout: Duration) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| Error::Process(format!("http client init failed: {e}")))?;

    let deadline = Instant::now() + timeout;

    loop {
        let last_error = match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            Ok(resp) => format!("HTTP {}", resp.status()),
            Err(e) => e.to_string(),
        };

        if Instant::now() >= deadline {
            return Err(Error::Process(format!(
                "endpoint {url} not ready after {:?}: {last_error}",
                timeout
            )));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// 受监督的引擎子进程。
///
/// - 生命周期绑定到 Rust 进程（`kill_on_drop`），避免僵尸进程；
/// - stdout/stderr 落盘，便于 `studio doctor` 与故障定位；
/// - 提供 HTTP/TCP 就绪探测，以及先 SIGTERM 后 SIGKILL 的优雅退出。
pub struct SupervisedChild {
    child: Child,
    label: String,
    log_path: Option<PathBuf>,
}

impl SupervisedChild {
    pub fn spawn(
        mut cmd: Command,
        label: impl Into<String>,
        log_path: Option<PathBuf>,
    ) -> Result<Self> {
        let label = label.into();

        if let Some(path) = &log_path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            let err = file.try_clone()?;
            cmd.stdout(Stdio::from(file));
            cmd.stderr(Stdio::from(err));
        } else {
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
        }
        cmd.stdin(Stdio::null());
        cmd.kill_on_drop(true);

        let child = cmd
            .spawn()
            .map_err(|e| Error::Process(format!("failed to spawn `{label}`: {e}")))?;

        Ok(Self {
            child,
            label,
            log_path,
        })
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn id(&self) -> Option<u32> {
        self.child.id()
    }

    pub fn log_path(&self) -> Option<&Path> {
        self.log_path.as_deref()
    }

    /// 非阻塞查询退出状态。
    pub fn try_wait(&mut self) -> std::io::Result<Option<i32>> {
        Ok(self.child.try_wait()?.and_then(|status| status.code()))
    }

    /// 轮询 HTTP 健康端点直到成功或超时。
    pub async fn wait_ready_http(&mut self, url: &str, timeout: Duration) -> Result<()> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|e| Error::Process(format!("http client init failed: {e}")))?;

        let deadline = Instant::now() + timeout;

        loop {
            if let Ok(Some(code)) = self.try_wait() {
                return Err(Error::Process(format!(
                    "`{}` exited during startup (code {code:?}), see {}",
                    self.label,
                    self.log_path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "<no log>".to_string())
                )));
            }

            let last_error = match client.get(url).send().await {
                Ok(resp) if resp.status().is_success() => return Ok(()),
                Ok(resp) => format!("HTTP {}", resp.status()),
                Err(e) if e.is_timeout() => "request timeout".to_string(),
                Err(e) => e.to_string(),
            };

            if Instant::now() >= deadline {
                return Err(Error::Process(format!(
                    "`{}` not ready after {:?}: {last_error}",
                    self.label, timeout
                )));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// 等待 TCP 端口可连接。
    pub async fn wait_ready_tcp(&mut self, addr: &str, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;

        loop {
            if let Ok(Some(code)) = self.try_wait() {
                return Err(Error::Process(format!(
                    "`{}` exited during startup (code {code:?})",
                    self.label
                )));
            }

            let last_error = match tokio::time::timeout(
                Duration::from_secs(2),
                tokio::net::TcpStream::connect(addr),
            )
            .await
            {
                Ok(Ok(_)) => return Ok(()),
                Ok(Err(e)) => e.to_string(),
                Err(_) => "connect timeout".to_string(),
            };

            if Instant::now() >= deadline {
                return Err(Error::Process(format!(
                    "`{}` port {addr} not reachable after {:?}: {last_error}",
                    self.label, timeout
                )));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// 先 SIGTERM，超时后 SIGKILL。
    pub async fn shutdown(&mut self, grace: Duration) -> Result<()> {
        if let Some(pid) = self.child.id() {
            signal::terminate(pid as i32);
        }

        match tokio::time::timeout(grace, self.child.wait()).await {
            Ok(Ok(_status)) => Ok(()),
            Ok(Err(e)) => Err(Error::Process(format!(
                "`{}` wait failed during shutdown: {e}",
                self.label
            ))),
            Err(_) => {
                let _ = self.child.start_kill();
                let _ = self.child.wait().await;
                tracing::warn!(label = %self.label, "process did not exit within grace period, killed");
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spawns_and_kills_short_lived_process() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 30"]);
        let mut child = SupervisedChild::spawn(cmd, "test-sleeper", None).expect("spawn ok");
        assert!(child.id().is_some());

        child
            .shutdown(Duration::from_secs(2))
            .await
            .expect("shutdown ok");
    }

    #[tokio::test]
    async fn detects_immediate_exit() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "exit 7"]);
        let mut child = SupervisedChild::spawn(cmd, "test-exit", None).expect("spawn ok");

        let err = child
            .wait_ready_tcp("127.0.0.1:1", Duration::from_secs(5))
            .await
            .expect_err("must fail fast");
        assert!(
            err.to_string().contains("exited during startup"),
            "unexpected error: {err}"
        );
    }
}
