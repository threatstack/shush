//! Sensu API related request and response-parsing logic

use std::convert::TryInto;
use std::env;
use std::fmt::{self,Display};

use serde_json::{Value,Map,Number};
use hyper::Uri;

mod client;
pub use self::client::*;

/// Enum representing the endpoints in Sensu as a type that shush accesses
#[derive(Clone)]
pub enum SensuEndpoint<'a> {
    /// Endpoint for listing silences
    Silenced,
    /// Endpoint for clearing silences
    Clear,
    /// Endpoint for getting clients
    Clients,
    /// Endpoint for getting a single client
    Client(&'a str),
    /// Endpoint for getting check results
    Results,
}

impl<'a> TryInto<Uri> for SensuEndpoint<'a> {
    type Error = String;

    fn try_into(self) -> Result<Uri, Self::Error> {
        match self {
            SensuEndpoint::Silenced => "/silenced".parse::<Uri>().map_err(|e| format!("{}", e)),
            SensuEndpoint::Clear => "/silenced/clear".parse::<Uri>().map_err(|e| format!("{}", e)),
            SensuEndpoint::Client(c) => format!("/clients/{}", c).parse::<Uri>()
                .map_err(|e| format!("{}", e)),
            SensuEndpoint::Clients => "/clients".parse::<Uri>().map_err(|e| format!("{}", e)),
            SensuEndpoint::Results => "/results".parse::<Uri>().map_err(|e| format!("{}", e)),
        }
    }
}

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

/// Enum for all types of duration of silences - only used in silences
#[derive(Debug,PartialEq,Clone)]
pub enum Expire {
    /// No time expiration with optional expire on resolve
    NoExpiration(bool),
    /// Expires in `usize` seconds with optional expire on resolve
    Expire(usize, bool),
}

impl Display for Expire {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Expire::NoExpiration(true) => write!(f, "not expire until resolution"),
            Expire::NoExpiration(false) => write!(f, "never expire"),
            Expire::Expire(sz, true) => write!(f, "expire in {} seconds or on resolution", sz),
            Expire::Expire(sz, false) => write!(f, "expire in {} seconds", sz),
        }
    }
}

/// Sensu resource for conversion to payload
#[derive(Debug,Clone)]
pub enum SensuResource {
    Client(String),
    Subscription(String),
}

impl PartialEq<String> for SensuResource {
    fn eq(&self, rhs: &String) -> bool {
        match *self {
            SensuResource::Client(ref st) => st == rhs,
            SensuResource::Subscription(ref st) => st == rhs,
        }
    }
}

impl Display for SensuResource {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            SensuResource::Client(ref s) => write!(f, "client:{}", s),
            SensuResource::Subscription(ref s) => write!(f, "{}", s),
        }
    }
}
