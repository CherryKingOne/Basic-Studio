use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use studio_core::{EngineKind, Error};

/// 配置根。所有子项都带 `#[serde(default)]`，因此缺字段时回落到默认值而非报错。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub models: ModelsConfig,
    /// key 为引擎名（`vllm` / `sglang` / `transformers`），与 `EngineKind::as_str()` 对应。
    pub engines: BTreeMap<String, EngineConfig>,
    pub sidecars: SidecarsConfig,
    pub gateway: GatewayConfig,
    pub scheduler: SchedulerConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            models: ModelsConfig::default(),
            engines: default_engines(),
            sidecars: SidecarsConfig::default(),
            gateway: GatewayConfig::default(),
            scheduler: SchedulerConfig::default(),
        }
    }
}

fn default_engines() -> BTreeMap<String, EngineConfig> {
    let mut engines = BTreeMap::new();
    for kind in EngineKind::ALL {
        engines.insert(kind.as_str().to_string(), EngineConfig::default_for(kind));
    }
    engines
}

impl Config {
    pub fn engine(&self, kind: EngineKind) -> Option<&EngineConfig> {
        self.engines.get(kind.as_str())
    }

    pub fn enabled_engines(&self) -> Vec<EngineKind> {
        EngineKind::ALL
            .into_iter()
            .filter(|kind| self.engine(*kind).is_some_and(|cfg| cfg.enabled))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelsConfig {
    /// 模型仓根目录，相对路径以进程工作目录为基准。
    pub root: PathBuf,
    pub auto_discover: bool,
    pub checksum_policy: ChecksumPolicy,
}

impl Default for ModelsConfig {
    fn default() -> Self {
        Self {
            root: PathBuf::from("models"),
            auto_discover: true,
            checksum_policy: ChecksumPolicy::Warn,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChecksumPolicy {
    Off,
    #[default]
    Warn,
    Enforce,
}

impl fmt::Display for ChecksumPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ChecksumPolicy::Off => "off",
            ChecksumPolicy::Warn => "warn",
            ChecksumPolicy::Enforce => "enforce",
        };
        f.write_str(s)
    }
}

impl FromStr for ChecksumPolicy {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "none" => Ok(ChecksumPolicy::Off),
            "warn" => Ok(ChecksumPolicy::Warn),
            "enforce" => Ok(ChecksumPolicy::Enforce),
            other => Err(Error::Config(format!("unknown checksum policy: {other}"))),
        }
    }
}

/// 单个引擎的启动配置。
///
/// `command` + `args` 里可以使用占位符：`{model_path}` `{port}` `{host}` `{venv}` `{tensor_parallel}`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineConfig {
    pub enabled: bool,
    /// 该引擎的 venv；相对路径以模型仓/工作目录为基准。
    pub venv: Option<PathBuf>,
    /// 可执行文件与固定子命令，例如 `["vllm", "serve"]`。
    pub command: Vec<String>,
    /// 追加参数，可含占位符。
    pub args: Vec<String>,
    /// 健康检查路径，拼接在 `http://host:port` 之后。
    pub health_path: String,
    /// 外部托管地址（如 compose 中 `http://vllm:8000`）；设置后由 studio 之外的服务负责启动。
    pub base_url: Option<String>,
    pub port: u16,
    pub startup_timeout_s: u64,
    pub max_instances: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            venv: None,
            command: Vec::new(),
            args: Vec::new(),
            health_path: "/health".to_string(),
            base_url: None,
            port: 8000,
            startup_timeout_s: 900,
            max_instances: 1,
        }
    }
}

impl EngineConfig {
    pub fn default_for(kind: EngineKind) -> Self {
        match kind {
            EngineKind::Vllm => Self {
                command: vec!["vllm".into(), "serve".into()],
                args: vec![
                    "{model_path}".into(),
                    "--host".into(),
                    "{host}".into(),
                    "--port".into(),
                    "{port}".into(),
                ],
                port: 8000,
                ..Self::default()
            },
            EngineKind::Sglang => Self {
                command: vec!["python".into(), "-m".into(), "sglang.launch_server".into()],
                args: vec![
                    "--model-path".into(),
                    "{model_path}".into(),
                    "--host".into(),
                    "{host}".into(),
                    "--port".into(),
                    "{port}".into(),
                ],
                port: 30000,
                ..Self::default()
            },
            EngineKind::Transformers => Self {
                command: vec![
                    "python".into(),
                    "-m".into(),
                    "studio_sidecar".into(),
                    "serve".into(),
                    "transformers".into(),
                ],
                args: vec![
                    "--model".into(),
                    "{model_path}".into(),
                    "--port".into(),
                    "{port}".into(),
                ],
                port: 9000,
                ..Self::default()
            },
        }
    }

    pub fn base_url(&self, host: &str) -> String {
        match &self.base_url {
            Some(url) => url.trim_end_matches('/').to_string(),
            None => format!("http://{host}:{}", self.port),
        }
    }

    /// 若配置了 venv 且程序是 python，则改用 venv 内的解释器。
    pub fn resolve_program(&self, program: &str) -> String {
        match &self.venv {
            Some(venv) if program == "python" || program == "python3" => venv
                .join("bin")
                .join("python")
                .to_string_lossy()
                .into_owned(),
            _ => program.to_string(),
        }
    }

    /// 渲染 `command` + `args`，得到最终可执行程序与参数列表。
    pub fn command_line(&self, vars: &[(&str, String)]) -> Result<(String, Vec<String>), Error> {
        let mut parts = self.command.iter();
        let program = parts
            .next()
            .ok_or_else(|| Error::Config("engine command is empty".to_string()))?;
        let program = self.resolve_program(&crate::render(program, vars));
        let args = parts
            .chain(self.args.iter())
            .map(|arg| crate::render(arg, vars))
            .collect();
        Ok((program, args))
    }

    pub fn validate(&self, kind: EngineKind) -> Result<(), Error> {
        if self.command.is_empty() {
            return Err(Error::Config(format!("engine `{kind}` has empty command")));
        }
        if self.port == 0 {
            return Err(Error::Config(format!("engine `{kind}` has invalid port 0")));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SidecarsConfig {
    /// key 为 sidecar 名（如 `transformers`）。
    pub entries: BTreeMap<String, SidecarConfig>,
}

impl SidecarsConfig {
    pub fn get(&self, name: &str) -> Option<&SidecarConfig> {
        self.entries.get(name)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SidecarConfig {
    pub venv: Option<PathBuf>,
    /// 入口，例如 `studio_sidecar.serve:transformers`。
    pub entry: String,
    /// 引擎 → 适配器映射，例如 `indextts_v25 = "studio_sidecar.adapters.indextts_v25"`。
    pub adapters: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayConfig {
    pub listen: String,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            listen: "0.0.0.0:8080".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SchedulerConfig {
    pub max_concurrency_per_model: usize,
    pub queue_max_depth: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrency_per_model: 8,
            queue_max_depth: 256,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_cover_all_engines() {
        let cfg = Config::default();
        for kind in EngineKind::ALL {
            let engine = cfg.engine(kind).expect("engine config present");
            assert!(engine.enabled);
            assert!(!engine.command.is_empty());
            engine.validate(kind).expect("default command is valid");
        }
        assert_eq!(cfg.enabled_engines().len(), 3);
    }

    #[test]
    fn parses_checksum_policy() {
        assert_eq!(
            "warn".parse::<ChecksumPolicy>().unwrap(),
            ChecksumPolicy::Warn
        );
        assert_eq!(
            "Enforce".parse::<ChecksumPolicy>().unwrap(),
            ChecksumPolicy::Enforce
        );
        assert!("nope".parse::<ChecksumPolicy>().is_err());
    }
}
