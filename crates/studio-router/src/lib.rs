//! 路由：把 (模型, 任务) 映射到具体引擎实例。
//!
//! M0 只做**候选排序**：按模型清单里的 `engines` 顺序，过滤掉未启用的引擎。
//! M2 再加入打分（就绪度 / 显存余量 / 队列深度）与故障转移。

use studio_core::{EngineKind, ModelCard, TaskKind};
use studio_registry::ModelIndex;

/// 一次路由结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub model_id: String,
    pub task: TaskKind,
    pub engine: EngineKind,
    /// 是否是模型的首选引擎。
    pub preferred: bool,
    pub reason: String,
}

/// 按任务与可用引擎给出候选列表，顺序即优先级。
pub fn select(index: &ModelIndex, task: TaskKind, available: &[EngineKind]) -> Vec<Selection> {
    select_for(index, None, task, available)
}

/// 指定模型 id 的候选列表；`model` 为 `None` 时等价于 [`select`]。
pub fn select_for(
    index: &ModelIndex,
    model: Option<&str>,
    task: TaskKind,
    available: &[EngineKind],
) -> Vec<Selection> {
    let mut out = Vec::new();

    for card in index.by_task(task) {
        if let Some(model) = model {
            if card.id.as_str() != model {
                continue;
            }
        }

        let preferred = card.preferred_engine();
        for engine in &card.engines {
            if !available.contains(engine) {
                continue;
            }
            out.push(Selection {
                model_id: card.id.to_string(),
                task,
                engine: *engine,
                preferred: preferred == Some(*engine),
                reason: describe(card, task, *engine),
            });
        }
    }

    out
}

/// 过滤出每个模型的最高优先级候选（每模型一个）。
pub fn best(index: &ModelIndex, task: TaskKind, available: &[EngineKind]) -> Vec<Selection> {
    let mut seen = std::collections::BTreeSet::new();
    select(index, task, available)
        .into_iter()
        .filter(|s| seen.insert(s.model_id.clone()))
        .collect()
}

fn describe(card: &ModelCard, task: TaskKind, engine: EngineKind) -> String {
    match (&card.adapter, &card.api_version) {
        (Some(adapter), Some(version)) => {
            format!("{task} via {engine} (adapter={adapter}, api={version})")
        }
        (Some(adapter), None) => format!("{task} via {engine} (adapter={adapter})"),
        _ => format!("{task} via {engine}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> ModelIndex {
        let tts = studio_core::ModelCard::parse(
            r#"
            id = "tts/index-tts2.5"
            family = "indextts"
            task = "tts"
            engines = ["transformers", "vllm"]
            adapter = "indextts_v25"
            api_version = "v25"
            "#,
        )
        .unwrap();

        ModelIndex::from_cards(vec![tts])
    }

    #[test]
    fn picks_declared_engine_order() {
        let available = [EngineKind::Vllm, EngineKind::Transformers];
        let selections = select(&index(), TaskKind::Tts, &available);
        assert_eq!(selections.len(), 2);
        assert_eq!(selections[0].engine, EngineKind::Transformers);
        assert!(selections[0].preferred);
        assert_eq!(selections[1].engine, EngineKind::Vllm);
    }

    #[test]
    fn skips_unavailable_engines() {
        let available = [EngineKind::Vllm];
        let selections = select(&index(), TaskKind::Tts, &available);
        assert_eq!(selections.len(), 1);
        assert_eq!(selections[0].engine, EngineKind::Vllm);
        assert!(!selections[0].preferred);
    }

    #[test]
    fn best_returns_one_per_model() {
        let available = [EngineKind::Vllm, EngineKind::Transformers];
        let selections = best(&index(), TaskKind::Tts, &available);
        assert_eq!(selections.len(), 1);
        assert_eq!(selections[0].engine, EngineKind::Transformers);
    }

    #[test]
    fn chat_task_has_no_candidates() {
        let available = [EngineKind::Vllm, EngineKind::Transformers];
        assert!(select(&index(), TaskKind::Chat, &available).is_empty());
    }
}
