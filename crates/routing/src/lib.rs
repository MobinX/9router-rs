use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

// ─── Connection & Basic Types ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Connection {
    pub id: String,
    pub provider: String,
    pub priority: i32,
    pub weight: u32,
    pub healthy: bool,
}

impl Connection {
    pub fn new(id: impl Into<String>, provider: impl Into<String>, priority: i32) -> Self {
        Self {
            id: id.into(),
            provider: provider.into(),
            priority,
            weight: 1,
            healthy: true,
        }
    }
}

// ─── Alias Resolution ─────────────────────────────────────────────────────

/// Resolves an alias, stopping after 1 step (backward-compatible).
pub fn resolve_alias(model: &str, aliases: &HashMap<String, String>) -> String {
    aliases
        .get(model)
        .cloned()
        .unwrap_or_else(|| model.to_string())
}

/// Resolves an alias chain with cycle detection up to a maximum depth.
pub fn resolve_alias_chain(model: &str, aliases: &HashMap<String, String>) -> String {
    let mut current = model.to_string();
    let mut visited = HashSet::new();
    visited.insert(current.clone());

    for _ in 0..10 {
        if let Some(target) = aliases.get(&current) {
            if visited.contains(target) {
                // Cycle detected; stop at current
                break;
            }
            visited.insert(target.clone());
            current = target.clone();
        } else {
            break;
        }
    }
    current
}

// ─── Combo Resolution & Multimodal Filtering ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComboTarget {
    pub id: String,
    pub name: String,
    pub models: Vec<String>,
}

/// Expands a model string if it matches a known combo name.
/// Per 9Router: if model contains '/', it is never treated as a combo name.
pub fn expand_combo(model: &str, combos: &[ComboTarget]) -> Option<Vec<String>> {
    if model.contains('/') {
        return None;
    }
    combos.iter().find(|c| c.name == model).and_then(|c| {
        if c.models.is_empty() {
            None
        } else {
            Some(c.models.clone())
        }
    })
}

/// Filters candidate models by required capability (e.g. "vision", "pdf", "audioInput", "videoInput").
pub fn filter_by_capability(
    models: &[String],
    required: &HashSet<String>,
    catalog_caps: &HashMap<String, HashSet<String>>,
) -> Vec<String> {
    if required.is_empty() {
        return models.to_vec();
    }
    let filtered: Vec<String> = models
        .iter()
        .filter(|m| {
            if let Some(caps) = catalog_caps.get(*m) {
                required.is_subset(caps)
            } else {
                false
            }
        })
        .cloned()
        .collect();

    // If filtering eliminates all models, fall back to unfiltered list to allow best-effort upstream
    if filtered.is_empty() {
        models.to_vec()
    } else {
        filtered
    }
}

// ─── Sticky Round Robin Router ────────────────────────────────────────────

/// Sticky state: (current_index, consecutive_use_count)
#[derive(Debug, Default)]
pub struct StickyRoundRobin {
    state: Mutex<HashMap<String, (usize, usize)>>,
}

impl StickyRoundRobin {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
        }
    }

    /// Rotates the slice using sticky round-robin logic matching 9Router.
    /// Returns the rotated candidates array starting with the selected index.
    pub fn select(&self, key: &str, items: &[String], sticky_limit: usize) -> Vec<String> {
        if items.is_empty() {
            return Vec::new();
        }
        if items.len() == 1 {
            return items.to_vec();
        }

        let limit = if sticky_limit == 0 { 1 } else { sticky_limit };
        let mut map = self.state.lock().unwrap();
        let (idx, count) = map.get(key).copied().unwrap_or((0, 0));
        let cur_idx = idx % items.len();

        // Rotate items so cur_idx is at index 0
        let mut rotated = Vec::with_capacity(items.len());
        for i in 0..items.len() {
            rotated.push(items[(cur_idx + i) % items.len()].clone());
        }

        let new_count = count + 1;
        if new_count >= limit {
            map.insert(key.to_string(), ((cur_idx + 1) % items.len(), 0));
        } else {
            map.insert(key.to_string(), (cur_idx, new_count));
        }

        rotated
    }

    pub fn reset(&self, key: &str) {
        let mut map = self.state.lock().unwrap();
        map.remove(key);
    }
}

// ─── Selection Strategies (Priority, RoundRobin, Weighted) ────────────────

/// Pick the best connection: lowest priority value, healthy first.
pub fn pick_connection(conns: &[Connection]) -> Option<&Connection> {
    let mut sorted: Vec<&Connection> = conns.iter().collect();
    sorted.sort_by_key(|c| c.priority);
    sorted
        .iter()
        .find(|c| c.healthy)
        .copied()
        .or_else(|| sorted.into_iter().next())
}

/// Simple round-robin index helper.
pub fn round_robin(conns: &[Connection], cursor: usize) -> Option<(usize, &Connection)> {
    if conns.is_empty() {
        return None;
    }
    let i = cursor % conns.len();
    Some((cursor.wrapping_add(1), &conns[i]))
}

/// Weighted connection selection based on cumulative weights.
pub fn pick_weighted(conns: &[Connection], seed: u64) -> Option<&Connection> {
    let healthy: Vec<&Connection> = conns.iter().filter(|c| c.healthy).collect();
    let pool = if healthy.is_empty() {
        conns.iter().collect::<Vec<&Connection>>()
    } else {
        healthy
    };
    if pool.is_empty() {
        return None;
    }
    let total_weight: u64 = pool.iter().map(|c| c.weight.max(1) as u64).sum();
    if total_weight == 0 {
        return pool.first().copied();
    }
    let target = seed % total_weight;
    let mut accum = 0;
    for c in &pool {
        accum += c.weight.max(1) as u64;
        if target < accum {
            return Some(*c);
        }
    }
    pool.last().copied()
}

// ─── Fallback & Retry Logic ───────────────────────────────────────────────

/// 9Router backoff settings matching upstream config.
pub const BACKOFF_BASE_MS: u64 = 2_000;
pub const BACKOFF_MAX_MS: u64 = 300_000;
pub const BACKOFF_MAX_LEVEL: u32 = 15;

/// Computes backoff cooldown duration matching 9Router formula:
/// min(base * 2^(level-1), max)
pub fn backoff_cooldown_ms(level: u32) -> u64 {
    if level == 0 {
        return BACKOFF_BASE_MS;
    }
    let shift = (level - 1).min(10); // cap shift to prevent overflow
    let calculated = BACKOFF_BASE_MS.saturating_mul(1u64 << shift);
    calculated.min(BACKOFF_MAX_MS)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackDecision {
    pub should_fallback: bool,
    pub cooldown_ms: u64,
    pub new_backoff_level: u32,
}

/// Comprehensive fallback decider matching 9Router upstream rules.
/// Evaluates status code and error text for rate limits, capacity, or billing/auth errors.
pub fn evaluate_fallback(
    status: u16,
    body_text: &str,
    current_backoff_level: u32,
) -> FallbackDecision {
    let text = body_text.to_lowercase();

    // 1. Backoff triggers: rate limit, quota, capacity, overloaded
    let is_backoff = status == 429
        || text.contains("rate limit")
        || text.contains("too many requests")
        || text.contains("quota exceeded")
        || text.contains("capacity")
        || text.contains("overloaded");

    if is_backoff {
        let new_level = (current_backoff_level + 1).min(BACKOFF_MAX_LEVEL);
        return FallbackDecision {
            should_fallback: true,
            cooldown_ms: backoff_cooldown_ms(new_level),
            new_backoff_level: new_level,
        };
    }

    // 2. Fixed cooldown errors (120s): 401, 402, 403, 404, or specific phrases
    if status == 401
        || status == 402
        || status == 403
        || status == 404
        || text.contains("no credentials")
        || text.contains("improperly formed request")
    {
        return FallbackDecision {
            should_fallback: true,
            cooldown_ms: 120_000,
            new_backoff_level: current_backoff_level,
        };
    }

    // 3. Short cooldown (5s): request not allowed
    if text.contains("request not allowed") {
        return FallbackDecision {
            should_fallback: true,
            cooldown_ms: 5_000,
            new_backoff_level: current_backoff_level,
        };
    }

    // 4. Server errors: 500, 502, 503, 504, 408
    if status == 408 || (500..600).contains(&status) {
        return FallbackDecision {
            should_fallback: true,
            cooldown_ms: 30_000,
            new_backoff_level: current_backoff_level,
        };
    }

    // 5. Success or client error that should NOT fallback
    FallbackDecision {
        should_fallback: false,
        cooldown_ms: 0,
        new_backoff_level: 0,
    }
}

/// Backward-compatible retry predicate.
pub fn should_retry(status: u16) -> bool {
    status == 408 || status == 429 || (500..600).contains(&status)
}

// ─── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn alias_single_and_chain() {
        let mut map = HashMap::new();
        map.insert("smart".into(), "gpt-4o".into());
        map.insert("fast".into(), "smart".into());
        map.insert("default".into(), "fast".into());

        assert_eq!(resolve_alias("smart", &map), "gpt-4o");
        assert_eq!(resolve_alias_chain("default", &map), "gpt-4o");
        assert_eq!(resolve_alias_chain("unknown", &map), "unknown");
    }

    #[test]
    fn alias_cycle_protection() {
        let mut map = HashMap::new();
        map.insert("a".into(), "b".into());
        map.insert("b".into(), "a".into());

        // Must terminate safely and return one of the loop nodes
        let res = resolve_alias_chain("a", &map);
        assert!(res == "a" || res == "b");
    }

    #[test]
    fn combo_expansion_rules() {
        let combos = vec![
            ComboTarget {
                id: "c1".into(),
                name: "Try".into(),
                models: vec!["openai/gpt-4o".into(), "anthropic/claude-sonnet".into()],
            },
            ComboTarget {
                id: "c2".into(),
                name: "Empty".into(),
                models: vec![],
            },
        ];

        // Normal combo expands
        assert_eq!(
            expand_combo("Try", &combos),
            Some(vec![
                "openai/gpt-4o".into(),
                "anthropic/claude-sonnet".into()
            ])
        );

        // Path syntax with slash is never a combo
        assert_eq!(expand_combo("provider/Try", &combos), None);

        // Empty models returns None
        assert_eq!(expand_combo("Empty", &combos), None);

        // Non-existent combo returns None
        assert_eq!(expand_combo("Missing", &combos), None);
    }

    #[test]
    fn sticky_round_robin_sequence() {
        let srr = StickyRoundRobin::new();
        let items = vec!["m1".to_string(), "m2".to_string(), "m3".to_string()];

        // Sticky limit 2: repeats each model twice before moving to next
        let r1 = srr.select("combo_a", &items, 2);
        assert_eq!(r1[0], "m1");
        let r2 = srr.select("combo_a", &items, 2);
        assert_eq!(r2[0], "m1");

        let r3 = srr.select("combo_a", &items, 2);
        assert_eq!(r3[0], "m2");
        let r4 = srr.select("combo_a", &items, 2);
        assert_eq!(r4[0], "m2");

        let r5 = srr.select("combo_a", &items, 2);
        assert_eq!(r5[0], "m3");

        // Independent keys
        let rb1 = srr.select("combo_b", &items, 1);
        assert_eq!(rb1[0], "m1");
        let rb2 = srr.select("combo_b", &items, 1);
        assert_eq!(rb2[0], "m2");
    }

    #[test]
    fn multimodal_filtering() {
        let models = vec![
            "text-only".into(),
            "vision-model".into(),
            "omni-model".into(),
        ];
        let mut caps = HashMap::new();
        caps.insert("vision-model".into(), HashSet::from(["vision".into()]));
        caps.insert(
            "omni-model".into(),
            HashSet::from(["vision".into(), "pdf".into()]),
        );

        let mut req_vision = HashSet::new();
        req_vision.insert("vision".into());

        let res = filter_by_capability(&models, &req_vision, &caps);
        assert_eq!(res, vec!["vision-model", "omni-model"]);

        let mut req_pdf = HashSet::new();
        req_pdf.insert("pdf".into());
        let res_pdf = filter_by_capability(&models, &req_pdf, &caps);
        assert_eq!(res_pdf, vec!["omni-model"]);

        // When no models match, falls back to full list
        let mut req_audio = HashSet::new();
        req_audio.insert("audioInput".into());
        let res_fallback = filter_by_capability(&models, &req_audio, &caps);
        assert_eq!(res_fallback, models);
    }

    #[test]
    fn weighted_selection() {
        let conns = vec![
            Connection {
                id: "c1".into(),
                provider: "p1".into(),
                priority: 0,
                weight: 10,
                healthy: true,
            },
            Connection {
                id: "c2".into(),
                provider: "p2".into(),
                priority: 0,
                weight: 90,
                healthy: true,
            },
        ];
        // Total weight = 100. Target 5 (<10) selects c1, Target 50 (>=10) selects c2.
        assert_eq!(pick_weighted(&conns, 5).unwrap().id, "c1");
        assert_eq!(pick_weighted(&conns, 50).unwrap().id, "c2");
    }

    #[test]
    fn fallback_evaluation_scenarios() {
        // 429 rate limit triggers exponential backoff
        let f1 = evaluate_fallback(429, "", 0);
        assert!(f1.should_fallback);
        assert_eq!(f1.new_backoff_level, 1);
        assert_eq!(f1.cooldown_ms, 2_000);

        let f2 = evaluate_fallback(200, "rate limit exceeded", 1);
        assert!(f2.should_fallback);
        assert_eq!(f2.new_backoff_level, 2);
        assert_eq!(f2.cooldown_ms, 4_000);

        // 401 invalid key triggers fixed 120s cooldown
        let f3 = evaluate_fallback(401, "invalid_api_key", 0);
        assert!(f3.should_fallback);
        assert_eq!(f3.cooldown_ms, 120_000);

        // 500 server error triggers 30s cooldown
        let f4 = evaluate_fallback(502, "bad gateway", 0);
        assert!(f4.should_fallback);
        assert_eq!(f4.cooldown_ms, 30_000);

        // 200 normal response does not fallback
        let f5 = evaluate_fallback(200, r#"{"choices":[]}"#, 0);
        assert!(!f5.should_fallback);
    }

    #[test]
    fn priority_skips_unhealthy() {
        let c = vec![
            Connection {
                id: "a".into(),
                provider: "x".into(),
                priority: 0,
                weight: 1,
                healthy: false,
            },
            Connection {
                id: "b".into(),
                provider: "y".into(),
                priority: 1,
                weight: 1,
                healthy: true,
            },
        ];
        assert_eq!(pick_connection(&c).unwrap().id, "b");
    }

    proptest::proptest! {
        #[test]
        fn backoff_monotonic(level in 0u32..30u32) {
            let cd = backoff_cooldown_ms(level);
            prop_assert!(cd >= BACKOFF_BASE_MS);
            prop_assert!(cd <= BACKOFF_MAX_MS);
        }

        #[test]
        fn rr_never_panics(cursor: usize, n in 0usize..8) {
            let c: Vec<Connection> = (0..n).map(|i| Connection::new(i.to_string(), "p", 0)).collect();
            let r = round_robin(&c, cursor);
            prop_assert_eq!(r.is_some(), n > 0);
        }
    }
}
