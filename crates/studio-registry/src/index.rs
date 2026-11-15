use std::collections::BTreeMap;

use studio_core::{ModelCard, ModelId, TaskKind};

/// 内存中的模型索引。
#[derive(Debug, Clone, Default)]
pub struct ModelIndex {
    models: Vec<ModelCard>,
    by_id: BTreeMap<String, usize>,
}

impl ModelIndex {
    pub fn from_cards(cards: Vec<ModelCard>) -> Self {
        let mut by_id = BTreeMap::new();
        for (idx, card) in cards.iter().enumerate() {
            by_id.entry(card.id.to_string()).or_insert(idx);
        }
        Self {
            models: cards,
            by_id,
        }
    }

    /// 按 id 精确查找。重复 id 时返回第一个。
    pub fn get(&self, id: &str) -> Option<&ModelCard> {
        let idx = *self.by_id.get(id)?;
        self.models
            .get(idx)
            .filter(|card| card.id == ModelId::new(id))
    }

    pub fn by_task(&self, task: TaskKind) -> Vec<&ModelCard> {
        self.models
            .iter()
            .filter(|card| card.supports(task))
            .collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = &ModelCard> {
        self.models.iter()
    }

    pub fn len(&self) -> usize {
        self.models.len()
    }

    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    /// 返回出现次数 > 1 的模型 id。
    pub fn duplicate_ids(&self) -> Vec<String> {
        let mut counts = BTreeMap::<String, usize>::new();
        for card in &self.models {
            *counts.entry(card.id.to_string()).or_default() += 1;
        }
        counts
            .into_iter()
            .filter(|(_, count)| *count > 1)
            .map(|(id, _)| id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str, task: &str, engines: &str) -> ModelCard {
        toml::from_str(&format!(
            r#"
            id = "{id}"
            family = "test"
            task = "{task}"
            engines = [{engines}]
            "#
        ))
        .expect("card parses")
    }

    #[test]
    fn lookup_by_id_and_task() {
        let index = ModelIndex::from_cards(vec![
            card("tts/index-tts2.5", "tts", "\"transformers\""),
            card("dm/laya", "decision", "\"vllm\""),
        ]);

        assert_eq!(index.len(), 2);
        assert_eq!(
            index.get("dm/laya").map(|c| c.id.to_string()),
            Some("dm/laya".to_string())
        );
        assert!(index.get("tts/nope").is_none());
        assert_eq!(index.by_task(TaskKind::Tts).len(), 1);
        assert_eq!(index.by_task(TaskKind::Decision).len(), 1);
    }

    #[test]
    fn detects_duplicates() {
        let index = ModelIndex::from_cards(vec![
            card("tts/dup", "tts", "\"transformers\""),
            card("tts/dup", "tts", "\"transformers\""),
        ]);
        assert_eq!(index.duplicate_ids(), vec!["tts/dup".to_string()]);
        assert_eq!(
            index.get("tts/dup").map(|c| c.id.to_string()).unwrap(),
            "tts/dup"
        );
    }
}
