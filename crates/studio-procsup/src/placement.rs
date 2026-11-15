use studio_config::EngineConfig;

/// 把设备分配应用到子进程：环境变量 + 并行度参数。
///
/// 未做分配时保持原样，便于"外部托管"与 CPU-only 部署。
pub fn apply(cmd: &mut tokio::process::Command, cfg: &EngineConfig) {
    let Some(placement) = &cfg.placement else {
        return;
    };

    let kind = cfg
        .accelerator
        .unwrap_or(studio_core::AcceleratorKind::Nvidia);
    let plan = studio_device::env::plan(kind, placement);

    for (key, value) in plan.env {
        cmd.env(key, value);
    }
    if !plan.args.is_empty() {
        cmd.args(plan.args);
    }
}
