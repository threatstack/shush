//! Sensu API related request and response-parsing logic

use std::env;
use std::fmt::{self,Display};
use serde_json::{Value,Map,Number};
use teatime::RequestTarget;

/// Enum representing the endpoints in Sensu as a type that shush accesses
#[derive(Clone)]
pub enum SensuEndpoint {
    /// Endpoint for listing silences
    Silenced,
    /// Endpoint for clearing silences
    Clear,
    /// Endpoint for getting clients
    Clients,
}

impl<'a> Into<RequestTarget<'a>> for SensuEndpoint {
    fn into(self) -> RequestTarget<'a> {
        match self {
            SensuEndpoint::Silenced => RequestTarget::Path("/silenced"),
            SensuEndpoint::Clear => RequestTarget::Path("/silenced/clear"),
            SensuEndpoint::Clients => RequestTarget::Path("/clients"),
        }
    }
}

/// Generic struct for any Sensu payload - can be used for clear or silence
#[derive(Debug)]
pub struct SensuPayload {
    /// Resource (node, client, or subscription)
    pub res: Option<SensuResource>,
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
        if let Some(SensuResource::Sub(s)) = self.res {
            payload.insert("subscription".to_string(), Value::String(s));
        } else if let Some(SensuResource::Client(c)) = self.res {
            payload.insert("subscription".to_string(), Value::String(format!("client:{}", c)));
        }

        // If checks specified, silence only these - otherwise silence all
        if let Some(c) = self.chk {
            payload.insert("check".to_string(), Value::String(c));
        }

        // Handle silence duration
        if let Some(Expire::ExpireOnResolve) = self.expire {
            payload.insert("expire_on_resolve".to_string(), Value::Bool(true));
        } else if let Some(Expire::Expire(num)) = self.expire {
            payload.insert("expire".to_string(), Value::Number(Number::from(num)));
        }

        // Convert to `Map` for HTTP body
        payload
    }
}

/// Enum for typing clients vs. subscriptions in Sensu
#[derive(Clone,Debug)]
pub enum SensuResource {
    /// Sets the subscription field in the JSON payload
    Sub(String),
    /// Same field in JSON but handles appending `client:` to the beginning
    Client(String),
}

impl Into<String> for SensuResource {
    fn into(self) -> String {
        match self {
            SensuResource::Sub(s) => s,
            SensuResource::Client(s) => format!("client:{}", s),
        }
    }
}

/// Enum for all types of duration of silences - only used in silences
#[derive(Debug,PartialEq,Clone)]
pub enum Expire {
    /// Never expires
    NoExpiration,
    /// Expires in `usize` seconds
    Expire(usize),
    /// Expires when check resolves
    ExpireOnResolve,
}

impl Display for Expire {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Expire::NoExpiration => write!(f, "never expire\n"),
            Expire::Expire(sz) => write!(f, "expire in {} seconds\n", sz),
            Expire::ExpireOnResolve => write!(f, "expire on resolution of the checks\n"),
        }
    }
}
