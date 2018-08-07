//! JSON parsing for Sensu API responses

use serde_json::Value;

/// Search JSON for namespaced key and return either `&serde_json::Value` or `None`
pub fn remove_fold<'a>(json: Value, key: &str) -> Option<Value> {
    key.split(".").fold(Some(json), |acc, next| {
        acc.and_then(|mut inner| inner.get_mut(next).map(|inner| inner.take()))
    })
}
