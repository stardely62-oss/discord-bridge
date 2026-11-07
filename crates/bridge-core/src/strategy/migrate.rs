//! Schema migrations for strategy documents.
//!
//! Documents are first read as untyped JSON, upgraded step by step to
//! [`CURRENT_SCHEMA_VERSION`], then deserialized strictly. Newer, unknown
//! versions are rejected instead of being silently truncated.

use super::{Strategy, CURRENT_SCHEMA_VERSION};
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    #[error("документ не является объектом JSON")]
    NotObject,
    #[error("версия схемы {0} новее поддерживаемой ({CURRENT_SCHEMA_VERSION}); обновите Bridge")]
    TooNew(u64),
    #[error("некорректная версия схемы")]
    BadVersion,
    #[error("ошибка структуры: {0}")]
    Shape(#[from] serde_json::Error),
}

/// Result of loading: the strategy and whether it was upgraded (so the caller
/// can back up the original file before rewriting it).
pub struct Loaded {
    pub strategy: Strategy,
    pub migrated_from: Option<u64>,
}

type Step = fn(&mut serde_json::Map<String, Value>);

/// `STEPS[n]` upgrades version `n` to `n + 1`.
///
/// Version 0 is the pre-release draft format used during development, where
/// the rule list was called `profiles` and the global options `capture`.
const STEPS: &[Step] = &[v0_to_v1];

fn v0_to_v1(m: &mut serde_json::Map<String, Value>) {
    if let Some(p) = m.remove("profiles") {
        m.entry("rules").or_insert(p);
    }
    if let Some(c) = m.remove("capture") {
        m.entry("global").or_insert(c);
    }
}

pub fn load_value(mut v: Value) -> Result<Loaded, MigrateError> {
    let m = v.as_object_mut().ok_or(MigrateError::NotObject)?;
    let ver = match m.get("schemaVersion") {
        None => 0,
        Some(x) => x.as_u64().ok_or(MigrateError::BadVersion)?,
    };
    if ver > CURRENT_SCHEMA_VERSION as u64 {
        return Err(MigrateError::TooNew(ver));
    }
    for step in &STEPS[ver as usize..] {
        step(m);
    }
    m.insert("schemaVersion".into(), Value::from(CURRENT_SCHEMA_VERSION));
    let strategy: Strategy = serde_json::from_value(v)?;
    Ok(Loaded {
        strategy,
        migrated_from: (ver != CURRENT_SCHEMA_VERSION as u64).then_some(ver),
    })
}

pub fn load_json_str(s: &str) -> Result<Loaded, MigrateError> {
    load_value(serde_json::from_str(s)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_v0() {
        let doc = r#"{"id":"old","name":"Old","source":"custom",
            "capture":[{"opt":"--wf-tcp","value":"443"}],
            "profiles":[{"args":[{"opt":"--filter-tcp","value":"443"},{"opt":"--dpi-desync","value":"fake"}]}]}"#;
        let l = load_json_str(doc).unwrap();
        assert_eq!(l.migrated_from, Some(0));
        assert_eq!(l.strategy.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(l.strategy.rules.len(), 1);
        assert_eq!(l.strategy.global[0].opt, "--wf-tcp");
    }

    #[test]
    fn current_is_untouched() {
        let s = Strategy::new_empty("a", "A");
        let l = load_json_str(&s.to_json_pretty()).unwrap();
        assert!(l.migrated_from.is_none());
        assert_eq!(l.strategy, s);
    }

    #[test]
    fn rejects_future() {
        let doc = r#"{"schemaVersion":99,"id":"a","name":"a","source":"custom","rules":[]}"#;
        assert!(matches!(load_json_str(doc), Err(MigrateError::TooNew(99))));
    }

    #[test]
    fn rejects_garbage() {
        assert!(load_json_str("[]").is_err());
        assert!(load_json_str("{\"schemaVersion\":\"1\"}").is_err());
        assert!(load_json_str("{not json").is_err());
    }
}
