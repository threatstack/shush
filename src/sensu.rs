//! Sensu API related request and response-parsing logic

use std::env;
use std::error::Error;
use std::fmt::{self,Display};

use serde_json::{self,Value,Map,Number};
use hyper::{Body,Client,Method,Request,StatusCode,Uri};
use hyper::client::HttpConnector;
use hyper::header::{self,HeaderValue};
use hyper::rt::{Future,Stream};
use tokio::runtime::Runtime;

use err::SensuError;
use opts::{ClearOpts,ListOpts,SilenceOpts};

pub struct SensuClient(Client<HttpConnector>, Runtime, Uri);

impl SensuClient {
    pub fn new(base_url: String) -> Result<Self, Box<Error>> {
        let runtime = Runtime::new()?;
        Ok(SensuClient(Client::builder().build(HttpConnector::new(4)), runtime, base_url.parse::<Uri>()?))
    }

    pub fn request<B>(&mut self, method: Method, uri: Uri, body: Option<B>)
            -> Result<Option<Value>, SensuError> where B: ToString {
        let mut full_uri = uri;
        if full_uri.authority_part().is_none() {
            let mut parts = full_uri.into_parts();
            let base_uri = self.2.clone().into_parts();
            parts.scheme = base_uri.scheme;
            parts.authority = base_uri.authority;
            full_uri = Uri::from_parts(parts).map_err(|e| SensuError::new(e.description()))?;
        }

        let mut builder = Request::builder();
        builder.method(method).uri(full_uri);
        let req = if let Some(b) = body {
            builder.header(header::CONTENT_LENGTH, b.to_string().len())
            .header(header::CONTENT_TYPE, HeaderValue::from_static("application/json"))
            .body(Body::from(b.to_string()))
            .map_err(|e| SensuError::new(e.description()))?
        } else {
            builder.body(Body::empty()).map_err(|e| SensuError::new(e.description()))?
        };

        let resp = self.0.request(req).map_err(|e| SensuError::new(e.description()))
            .and_then(|resp| {
                if resp.status() == StatusCode::NOT_FOUND {
                    return Err(SensuError::not_found());
                }
            Ok(resp)
        });
        let response = self.1.block_on(resp)?;
        let json = response.into_body().concat2().and_then(|chunk| {
            Ok(serde_json::from_slice::<Value>(&chunk).ok())
        });
        Ok(self.1.block_on(json)?)
    }

    pub fn silence(&mut self, s: &SilenceOpts) -> Result<(), Box<Error>> {
        Ok(())
    }

    pub fn clear(&mut self, s: &ClearOpts) -> Result<(), Box<Error>> {
        Ok(())
    }

    pub fn list(&mut self, s: &ListOpts) -> Result<(), Box<Error>> {
        Ok(())
    }
}

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
    Client(&'a String),
    /// Endpoint for getting check results
    Results,
}

impl<'a> Into<Uri> for SensuEndpoint<'a> {
    fn into(self) -> Uri {
        match self {
            SensuEndpoint::Silenced => "/silenced".parse::<Uri>().expect("Should not get here"),
            SensuEndpoint::Clear => "/silenced/clear".parse::<Uri>().expect("Should not get here"),
            SensuEndpoint::Client(c) => format!("/clients/{}", c).parse::<Uri>().expect("Should not get here"),
            SensuEndpoint::Clients => "/clients".parse::<Uri>().expect("Should not get here"),
            SensuEndpoint::Results => "/results".parse::<Uri>().expect("Should not get here"),
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
