//! JSON parsing for Sensu API responses

use serde_json::Value;

/// Newtype struct for easier JSON object key traversal
pub struct JsonRef<'a>(pub &'a Value);

impl<'a> JsonRef<'a> {
    /// Search JSON for namespaced key and return either `&serde_json::Value` or `None`
    pub fn get_fold(&'a self, key: &str) -> Option<&'a Value> {
        key.split(".").fold(Some(self.0), |acc, next| {
            acc.and_then(|inner| { inner.get(next) })
        })
    }

    /// Search JSON for namespaced key and return either value as `&str` or default
    pub fn get_fold_as_str_def(&'a self, key: &str, def: &'a str) -> &'a str {
        self.get_fold_as_str(key).unwrap_or(def)
    }

    /// Search JSON for namespaced key and return either value as `&str` or `None`
    pub fn get_fold_as_str(&'a self, key: &str) -> Option<&'a str> {
        match self.get_fold(key) {
            Some(val_ref) => {
                val_ref.as_str()
            },
            _ => None,
        }
    }

    /// Search JSON for namespaced key and return either value as `bool` or default
    pub fn get_fold_as_bool_def(&'a self, key: &str, def: bool) -> bool {
        self.get_fold_as_bool(key).unwrap_or(def)
    }

    /// Search JSON for namespaced key and return either value as `bool` or `None`
    pub fn get_fold_as_bool(&'a self, key: &str) -> Option<bool> {
        match self.get_fold(key) {
            Some(val_ref) => {
                val_ref.as_bool()
            },
            _ => None,
        }
    }

    /// Search JSON for namespaced key and return either value as `i64` or default
    pub fn get_fold_as_i64_def(&'a self, key: &str, def: i64) -> i64 {
        self.get_fold_as_i64(key).unwrap_or(def)
    }

    /// Search JSON for namespaced key and return either value as `i64` or `None`
    pub fn get_fold_as_i64(&'a self, key: &str) -> Option<i64> {
        match self.get_fold(key) {
            Some(ref_val) => {
                ref_val.as_i64()
            },
            _ => None,
        }
    }

    /// Get `&serde_json::Value` as a reference to a `Vec<Value>` or return `None` if
    /// this `Value` is not a JSON array
    pub fn get_as_vec(&'a self) -> Option<&'a Vec<Value>> {
        match *self.0 {
            Value::Array(ref vec) => Some(vec),
            _ => None,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json;

    #[test]
    fn test_get_fold() {
        let json = json!(
            {"this":
                {"is":
                    {"a": "tested thing",
                     "not": {"a": "test"}
                    }
                }
            });
        assert_eq!(JsonRef(&json).get_fold("this.is.not.a"), Some(&serde_json::Value::String("test".to_string())));
        assert_eq!(JsonRef(&json).get_fold("this.is.a"), Some(&serde_json::Value::String("tested thing".to_string())));
        assert_eq!(JsonRef(&json).get_fold("this.is.could.be.a"), None);
    }

    #[test]
    fn test_get_fold_as_str() {
        let json = json!(
            {"this":
                {"is":
                    {"a": "tested thing",
                     "not": {"a": "test"}
                    }
                }
            });
        assert_eq!(JsonRef(&json).get_fold_as_str_def("this.is.not.a", "fail"), "test");
        assert_eq!(JsonRef(&json).get_fold_as_str_def("this.is.a", "fail"), "tested thing");
        assert_eq!(JsonRef(&json).get_fold_as_str_def("this.is.could.be.a", "success"), "success");
        assert_eq!(JsonRef(&json).get_fold_as_str("this.is.could.be.a"), None);
    }

    #[test]
    fn test_get_fold_as_bool() {
        let json = json!(
            {
                "this": {
                    "is": {
                        "a": false,
                        "not": true
                    }
                }
            });
        assert_eq!(JsonRef(&json).get_fold_as_bool_def("this.is.not", false), true);
        assert_eq!(JsonRef(&json).get_fold_as_bool_def("this.is.a", true), false);
        assert_eq!(JsonRef(&json).get_fold_as_bool_def("this.is.could.be.a", false), false);
        assert_eq!(JsonRef(&json).get_fold_as_bool("this.is.could.be.a"), None);
    }

    #[test]
    fn test_get_fold_as_i64() {
        let json = json!(
            {
                "this": {
                    "is": {
                        "a": 2,
                        "not": 1
                    }
                }
            });
        assert_eq!(JsonRef(&json).get_fold_as_i64_def("this.is.not", 2), 1);
        assert_eq!(JsonRef(&json).get_fold_as_i64_def("this.is.a", 1), 2);
        assert_eq!(JsonRef(&json).get_fold_as_i64_def("this.is.could.be.a", 3), 3);
        assert_eq!(JsonRef(&json).get_fold_as_i64("this.is.could.be.a"), None);
    }

    #[test]
    fn test_get_as_vec() {
        let json = json!([1, 2, 3, 4, 5]);
        assert_eq!(JsonRef(&json).get_as_vec().and_then(|val| val[0].as_i64()), Some(1));
        assert_eq!(JsonRef(&json).get_as_vec().and_then(|val| val[2].as_i64()), Some(3));
        assert_eq!(JsonRef(&json).get_as_vec().and_then(|val| val[4].as_i64()), Some(5));
    }
}
