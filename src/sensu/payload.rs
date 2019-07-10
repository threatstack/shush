use std::env;

use serde_json::{Value,Map,Number};

use super::Expire;

/// Generic struct for any Sensu payload - can be used for clear or silence
#[derive(Debug)]
pub struct SensuPayload {
    /// Resource (node, client, or subscription)
    pub res: Option<String>,
    /// Sensu check
    pub chk: Option<String>,
    /// Time until expiration
    pub expire: Option<Expire>,
}

impl Into<Map<String, Value>> for SensuPayload {
    fn into(self) -> Map<String, Value> {
        let mut payload = Map::new();

        // Always inject USER information into payload as creator field
        let user = env::var("USER").unwrap_or("shush".to_string());
        payload.insert("creator".to_string(), Value::String(user));

        // Handle subscription for payload as Sensu client value, subscription, or all
        if let Some(string) = self.res {
            payload.insert("subscription".to_string(), Value::from(string));
        }

        // If checks specified, silence only these - otherwise silence all
        if let Some(c) = self.chk {
            payload.insert("check".to_string(), Value::String(c));
        }

        // Handle silence duration
        if let Some(Expire::NoExpiration(eor)) = self.expire {
            if eor {
                payload.insert("expire_on_resolve".to_string(), Value::Bool(true));
            }
        } else if let Some(Expire::Expire(num, eor)) = self.expire {
            payload.insert("expire".to_string(), Value::Number(Number::from(num)));
            if eor {
                payload.insert("expire_on_resolve".to_string(), Value::Bool(true));
            }
        }

        // Convert to `Map` for HTTP body
        payload
    }
}
