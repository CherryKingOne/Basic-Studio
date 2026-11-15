use std::fs;
use std::path::Path;

use studio_core::{Error, Result};

use crate::schema::Config;

/// 默认配置的唯一来源。
const DEFAULT_TOML: &str = include_str!("../../../config/default.toml");

/// 读取配置：以 `config/default.toml` 为基线，用传入文件做深合并覆盖。
///
/// `path` 为 `None` 时等价于纯默认配置。
pub fn load(path: Option<&Path>) -> Result<Config> {
    let raw = match path {
        Some(path) => fs::read_to_string(path)
            .map_err(|e| Error::Config(format!("cannot read config {}: {e}", path.display())))?,
        None => DEFAULT_TOML.to_string(),
    };
    load_from_str(&raw)
}

/// 从 TOML 文本加载配置（测试与嵌入场景使用）。
pub fn load_from_str(raw: &str) -> Result<Config> {
    let base: toml::Value = toml::from_str(DEFAULT_TOML)
        .map_err(|e| Error::Config(format!("built-in default config is invalid: {e}")))?;
    let over: toml::Value =
        toml::from_str(raw).map_err(|e| Error::Config(format!("invalid config: {e}")))?;

    let merged = merge(base, over);
    let config: Config = merged
        .try_into()
        .map_err(|e| Error::Config(format!("invalid config: {e}")))?;

    validate(&config)?;
    Ok(config)
}

/// 配置校验：引擎名必须是已知引擎，且每个引擎的启动参数自洽。
///
/// 未知引擎名直接报错，避免 `[engines.nope]` 被静默忽略导致"配了却不生效"。
pub fn validate(config: &Config) -> Result<()> {
    for (key, engine) in &config.engines {
        let kind = key
            .parse::<studio_core::EngineKind>()
            .map_err(|_| Error::Config(format!("unknown engine `{key}` in [engines]")))?;
        engine.validate(kind)?;
    }
    Ok(())
}

/// TOML 深合并：表递归合并，其余类型直接覆盖。
fn merge(base: toml::Value, over: toml::Value) -> toml::Value {
    match (base, over) {
        (toml::Value::Table(mut base), toml::Value::Table(over)) => {
            for (key, value) in over {
                let merged = match base.remove(&key) {
                    Some(existing) => merge(existing, value),
                    None => value,
                };
                base.insert(key, merged);
            }
            toml::Value::Table(base)
        }
        (_, over) => over,
    }
}

/// 替换 `{name}` 占位符。
pub fn render(template: &str, vars: &[(&str, String)]) -> String {
    let mut out = template.to_string();
    for (key, value) in vars {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::EngineKind;

    #[test]
    fn loads_defaults() {
        let cfg = load(None).expect("default config loads");
        assert_eq!(cfg.models.root, std::path::PathBuf::from("models"));
        assert_eq!(cfg.enabled_engines().len(), 4);
        assert_eq!(cfg.gateway.listen, "0.0.0.0:17283");
    }

    #[test]
    fn override_is_deep_merged() {
        let cfg = load_from_str(
            r#"
            [engines.vllm]
            port = 8001
            "#,
        )
        .expect("override loads");

        let vllm = cfg.engine(EngineKind::Vllm).unwrap();
        assert_eq!(vllm.port, 8001);
        // 未覆盖的字段仍来自默认值。
        assert!(!vllm.command.is_empty());
        assert_eq!(vllm.health_path, "/health");
        // 其他引擎不受影响。
        assert_eq!(cfg.engine(EngineKind::Sglang).unwrap().port, 30000);
    }

    #[test]
    fn rejects_unknown_engine_key() {
        let err = load_from_str("[engines.nope]\nenabled = true").expect_err("must fail");
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn renders_placeholders() {
        let vars = vec![("port", "8000".to_string())];
        assert_eq!(render("--port {port}", &vars), "--port 8000");
    }
}
