//! Sensu API related request and response-parsing logic

use tokio_core::reactor::Core;
use futures::{Future,Stream};
use hyper::{Client,Method,Request,Uri};
use hyper::header::ContentLength;
use hyper::client::HttpConnector;
use hyper_tls::HttpsConnector;

use std::env;
use std::str;
use std::fmt::{self,Display};
use std::collections::HashMap;
use serde_json::{self,Value,Map,Number};

use sensu_json::JsonRef;
use sensu_err::SensuError;

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

impl Into<&'static str> for SensuEndpoint {
    fn into(self) -> &'static str {
        match self {
            SensuEndpoint::Silenced => "/silenced",
            SensuEndpoint::Clear => "/silenced/clear",
            SensuEndpoint::Clients => "/clients",
        }
    }
}

/// Generic struct for any Sensu payload - can be used for clear or silence
#[derive(Debug)]
pub struct SensuPayload {
    pub res: Option<SensuResource>,
    pub chk: Option<String>,
    pub expire: Option<Expire>,
}

impl Into<String> for SensuPayload {
    fn into(self) -> String {
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

        // Convert to string for HTTP body
        Value::Object(payload).to_string()
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

/// Struct for Sensu client built on hyper HTTP client
pub struct SensuClient {
    core: Core,
    host: String,
    port: u16,
    client: Client<HttpsConnector<HttpConnector>>,
}

/// Methods for Sensu client
impl SensuClient {
    /// Create new HTTP connection for Sensu and return client
    pub fn new(host: &str, port: u16) -> Result<Self, SensuError> {
        let core = try!(Core::new());
        let client = Client::configure()
                .connector(try!(HttpsConnector::new(4, &core.handle())))
                .build(&core.handle());
        Ok(SensuClient{
            core,
            host: host.to_string(),
            port,
            client,
        })
    }

    /// Generic request interface for Sensu REST API
    pub fn sensu_request<'a>(&mut self, method: Method, endpoint: SensuEndpoint,
                             body: Option<SensuPayload>)
                             -> Result<Option<Value>, SensuError> {
        let sendpoint: &'static str = endpoint.into();
        let mut req = Request::new(
            method,
            try!(format!("{}:{}{}", self.host.clone(), self.port, sendpoint).parse::<Uri>())
        );
        if let Some(b) = body {
            let sbody: String = b.into();
            // Set content length to work around Hyper defaulting to chunked HTTP
            {
                let hdrs = req.headers_mut();
                hdrs.set::<ContentLength>(ContentLength(sbody.len() as u64));
            }
            req.set_body(sbody);
        } else {
            let hdrs = req.headers_mut();
            // Set content length to 0 when there is no body
            hdrs.set::<ContentLength>(ContentLength(0));
        };
        // Concatenate all data and parse text response to JSON on request future completion
        let req_fut = self.client.request(req).and_then(|r| {
            r.body().concat2().and_then(|c| match serde_json::from_str(
                match str::from_utf8(&c).ok() {
                    Some("") => "{}",
                    Some(s) => s,
                    None => "{}"
                }
            ) {
                Ok(json) => Ok(json),
                Err(_) => {
                    println!("Failure parsing JSON API response");
                    Ok(json!({}))
                },
            })
        });
        Ok(try!(self.core.run(req_fut))).map(Some)
    }

    /// Mapping function for translating instance IDs in AWS to Sensu client IDs
    pub fn map_iids_to_clients(&mut self, iids: Vec<String>) -> Result<Vec<String>, SensuError> {
        let clients = match try!(self.sensu_request(Method::Get, SensuEndpoint::Clients, None)) {
            Some(json) => json,
            None => { return Err(SensuError::new("No response from client endpoint")); },
        };

        // Generate map from array of JSON objects - from [{"name": CLIENT_ID, "instance_id": IID},..] to
        // {IID1: CLIENT_ID1, IID2: CLIENT_ID2,...}
        let mut map = HashMap::new();
        if let Some(v) = JsonRef(&clients).get_as_vec() {
            for item in v.iter() {
                let iid = JsonRef(item).get_fold_as_str("instance_id")
                    .map(|val| val.to_string());
                let client = JsonRef(item).get_fold_as_str("name").map(|val| val.to_string());
                if let (Some(i), Some(c)) = (iid, client) {
                    map.insert(i, c);
                }
            }
            Ok(iids.iter().fold(Vec::new(), |mut acc, v| {
                if let Some(val) = map.remove(v) {
                    acc.push(val);
                } else {
                    println!(r#"WARNING: instance ID "{}" not associated with Sensu client ID"#, v);
                    println!("If you recently provisioned an instance, please wait for it to register with Sensu");
                    println!();
                }
                acc
            }))
        } else {
            Ok(vec![])
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_sensu_payload_into_string() {
        let mut plstring: String = SensuPayload {
            res: None,
            chk: Some("this-is-a-test".to_string()),
            expire: Some(Expire::NoExpiration),
        }.into();
        assert_eq!(plstring, format!(r#"{{"check":"this-is-a-test","creator":"{}"}}"#,
                                     env!("USER")));
        plstring = SensuPayload {
            res: Some(SensuResource::Client("test-resource".to_string())),
            chk: Some("this-is-a-test".to_string()),
            expire: Some(Expire::NoExpiration),
        }.into();
        assert_eq!(plstring, format!(r#"{{"check":"this-is-a-test","creator":"{}","subscription":"client:test-resource"}}"#, env!("USER")));
        plstring = SensuPayload {
            res: Some(SensuResource::Sub("test-resource".to_string())),
            chk: Some("this-is-a-test".to_string()),
            expire: Some(Expire::Expire(100)),
        }.into();
        assert_eq!(plstring, format!(r#"{{"check":"this-is-a-test","creator":"{}","expire":100,"subscription":"test-resource"}}"#, env!("USER")));
        plstring = SensuPayload {
            res: Some(SensuResource::Sub("test-resource".to_string())),
            chk: None,
            expire: Some(Expire::ExpireOnResolve),
        }.into();
        assert_eq!(plstring, format!(r#"{{"creator":"{}","expire_on_resolve":true,"subscription":"test-resource"}}"#, env!("USER")));
        plstring = SensuPayload {
            res: Some(SensuResource::Client("resource".to_string())),
            chk: None,
            expire: None,
        }.into();
        assert_eq!(plstring, format!(r#"{{"creator":"{}","subscription":"client:resource"}}"#, env!("USER")));
    }
}
