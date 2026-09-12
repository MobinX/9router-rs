use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub id: String,
    pub provider: String,
    pub priority: i32,
    pub healthy: bool,
}

pub fn resolve_alias(model: &str, aliases: &std::collections::HashMap<String, String>) -> String {
    aliases
        .get(model)
        .cloned()
        .unwrap_or_else(|| model.to_string())
}

/// Priority routing: lowest priority first, skip unhealthy unless all unhealthy.
pub fn pick_connection(conns: &[Connection]) -> Option<&Connection> {
    let mut sorted: Vec<&Connection> = conns.iter().collect();
    sorted.sort_by_key(|c| c.priority);
    sorted
        .iter()
        .find(|c| c.healthy)
        .copied()
        .or_else(|| sorted.into_iter().next())
}

/// Retryable upstream statuses: timeouts, rate limits, server errors.
/// Client errors (except 408/429) fail fast with no fallback.
pub fn should_retry(status: u16) -> bool {
    status == 408 || status == 429 || (500..600).contains(&status)
}

/// Round-robin index helper (wrapping).
pub fn round_robin(conns: &[Connection], cursor: usize) -> Option<(usize, &Connection)> {
    if conns.is_empty() {
        return None;
    }
    let i = cursor % conns.len();
    Some((cursor.wrapping_add(1), &conns[i]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    #[test]
    fn priority_skips_unhealthy() {
        let c = vec![
            Connection {
                id: "a".into(),
                provider: "x".into(),
                priority: 0,
                healthy: false,
            },
            Connection {
                id: "b".into(),
                provider: "y".into(),
                priority: 1,
                healthy: true,
            },
        ];
        assert_eq!(pick_connection(&c).unwrap().id, "b");
    }
    #[test]
    fn alias_fallback() {
        let m = std::collections::HashMap::from([(
            "fast".to_string(),
            "openai/gpt-4o-mini".to_string(),
        )]);
        assert_eq!(resolve_alias("fast", &m), "openai/gpt-4o-mini");
        assert_eq!(resolve_alias("other", &m), "other");
    }
    #[test]
    fn retry_set() {
        for s in [200, 201, 400, 401, 403, 404] {
            assert!(!should_retry(s), "{s} must not retry");
        }
        for s in [408, 429, 500, 502, 503, 504] {
            assert!(should_retry(s), "{s} must retry");
        }
    }
    #[test]
    fn rr_wraps() {
        let c = vec![Connection {
            id: "a".into(),
            provider: "x".into(),
            priority: 0,
            healthy: true,
        }];
        let (n, conn) = round_robin(&c, 5).unwrap();
        assert_eq!((n, conn.id.as_str()), (6, "a"));
    }
    proptest::proptest! {
        #[test]
        fn rr_never_panics(cursor: usize, n in 0usize..8) {
            let c: Vec<Connection> = (0..n).map(|i| Connection { id: i.to_string(), provider: "p".into(), priority: 0, healthy: true }).collect();
            let r = round_robin(&c, cursor);
            prop_assert_eq!(r.is_some(), n > 0);
        }
    }
}
