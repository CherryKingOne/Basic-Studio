//! API Key 鉴权。
//!
//! 极简实现：配置里给出若干 key，请求带 `Authorization: Bearer <key>` 才放行。
//! 空列表表示关闭鉴权（仅用于本地开发）。

use std::collections::HashSet;

#[derive(Debug, Clone, Default)]
pub struct ApiKeys {
    keys: HashSet<String>,
}

impl ApiKeys {
    pub fn new(keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            keys: keys
                .into_iter()
                .map(Into::into)
                .filter(|k| !k.is_empty())
                .collect(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// 校验 `Authorization` 头。空配置直接放行。
    pub fn check(&self, header: Option<&str>) -> bool {
        if self.is_open() {
            return true;
        }
        match header {
            Some(value) => match extract_bearer(value) {
                Some(token) => self.keys.contains(token),
                None => false,
            },
            None => false,
        }
    }

    /// 常量时间比较的 Bearer 提取。
    pub fn token(header: &str) -> Option<&str> {
        extract_bearer(header)
    }
}

fn extract_bearer(header: &str) -> Option<&str> {
    let (scheme, token) = header.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    (!token.is_empty()).then_some(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_allows_everything() {
        let keys = ApiKeys::new(Vec::<String>::new());
        assert!(keys.is_open());
        assert!(keys.check(None));
        assert!(keys.check(Some("Bearer whatever")));
    }

    #[test]
    fn validates_bearer_token() {
        let keys = ApiKeys::new(["k1", "k2"]);
        assert!(keys.check(Some("Bearer k1")));
        assert!(keys.check(Some("bearer k2")));
        assert!(!keys.check(Some("Bearer k3")));
        assert!(!keys.check(Some("k1")));
        assert!(!keys.check(None));
    }

    #[test]
    fn ignores_empty_keys() {
        let keys = ApiKeys::new(["", "k"]);
        assert_eq!(keys.len(), 1);
    }
}
