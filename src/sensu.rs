//! Sensu API related request and response-parsing logic

use std::collections::HashMap;
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
use resources::{ShushResources,ShushResourceType};

pub struct SensuClient(Client<HttpConnector>, Runtime, Uri);

impl SensuClient {
    pub fn new(base_url: String) -> Result<Self, Box<dyn Error>> {
        Ok(SensuClient(Client::builder().build(HttpConnector::new(4)), Runtime::new()?,
                       base_url.parse::<Uri>()?))
    }

    pub fn request<U>(&mut self, method: Method, uri: U, body: Option<SensuPayload>)
            -> Result<Option<Value>, SensuError> where U: Into<Uri> {
        let mut full_uri = uri.into();
        let map: Option<Map<String, Value>> = body.map(|b| b.into());
        if full_uri.authority_part().is_none() {
            let mut parts = full_uri.into_parts();
            let base_uri = self.2.clone().into_parts();
            parts.scheme = base_uri.scheme;
            parts.authority = base_uri.authority;
            full_uri = Uri::from_parts(parts).map_err(|e| SensuError::new(e.description()))?;
        }

        let mut builder = Request::builder();
        builder.method(method).uri(full_uri);
        let req = if let Some(ref m) = map {
            let body_string = serde_json::to_string(m).map_err(|e| {
                SensuError::new(e.description())
            })?;
            builder.header(header::CONTENT_LENGTH, body_string.len())
            .header(header::CONTENT_TYPE, HeaderValue::from_static("application/json"))
            .body(Body::from(body_string))
            .map_err(|e| SensuError::new(e.description()))?
        } else {
            builder.body(Body::empty()).map_err(|e| SensuError::new(e.description()))?
        };

        self.1.block_on(self.0.request(req).map_err(|e| {
            SensuError::from(e)
        }).and_then(|resp| {
            if resp.status() == StatusCode::NOT_FOUND {
                return Err(SensuError::not_found());
            }
            Ok(resp)
        }).and_then(|resp| {
            resp.into_body().concat2().map_err(|e| SensuError::from(e))
        }).and_then(|chunk| {
            serde_json::from_slice::<Value>(&chunk).map_err(|e| {
                println!("Response: {}", match std::str::from_utf8(&chunk).map_err(|e| {
                    SensuError::new(&e.to_string())
                }) {
                    Ok(j) => j,
                    Err(e) => return e,
                });
                SensuError::new(&e.to_string())
            }).map(Some)
        }))
    }

    pub fn get_node_to_client_map(&mut self) -> Result<HashMap<String, String>, Box<dyn Error>> {
        let clients = self.request(Method::GET, SensuEndpoint::Clients, None)?;

        let mut client_map = HashMap::new();
        if let Some(Value::Array(v)) = clients {
            for mut item in v {
                let iid = item.as_object_mut().and_then(|obj| obj.remove("instance_id"));
                let iid_string = match iid {
                    Some(Value::String(string)) => Some(string),
                    _ => None,
                };

                let client = item.as_object_mut().and_then(|client| client.remove("name"));
                let client_string = match client {
                    Some(Value::String(string)) => Some(string),
                    _ => None,
                };

                if let (Some(i), Some(c)) = (iid_string, client_string) {
                    client_map.insert(i, c);
                }
            }
        }

        Ok(client_map)
    }

    pub fn map_to_sensu_resources(&mut self, res: ShushResources)
            -> Result<Vec<SensuResource>, Box<dyn Error>>{
        let (resource_type, resources) = (res.res_type, res.resources);
        let mut map = self.get_node_to_client_map()?;
        let mapped_resources = match resource_type {
            ShushResourceType::Node => resources.iter().fold(Vec::new(), |mut acc, v| {
                if let Some(val) = map.remove(v) {
                    acc.push(SensuResource::Client(val));
                } else {
                    println!(r#"WARNING: instance ID "{}" not associated with Sensu client ID"#, v);
                    println!("If you recently provisioned an instance, please wait for it to \
                             register with Sensu");
                    println!();
                }
                acc
            }),
            ShushResourceType::Sub => resources.into_iter()
                .map(SensuResource::Subscription).collect(),
            ShushResourceType::Client => resources.into_iter()
                .map(SensuResource::Client).collect(),
        };
        Ok(mapped_resources)
    }

    pub fn silence(&mut self, s: SilenceOpts) -> Result<(), Box<dyn Error>> {
        let resources: Option<Vec<String>> = s.resources
                .and_then(|res| match self.map_to_sensu_resources(res) {
            Ok(vec) => Some(vec.into_iter().map(|r| format!("{}", r)).collect()),
            Err(e) => {
                println!("{}", e);
                None
            },
        });
        let checks = s.checks;
        let expire = s.expire;
        match (resources, checks) {
            (Some(res), Some(chk)) => iproduct!(res, chk).for_each(|(r, c)| {
                let _ = self.request(Method::POST, SensuEndpoint::Silenced, Some(SensuPayload {
                    res: Some(r),
                    chk: Some(c),
                    expire: Some(expire.clone()),
                })).map_err(|e| {
                    println!("{}", e);
                });
            }),
            (Some(res), None) => res.into_iter().for_each(|r| {
                let _ = self.request(Method::POST, SensuEndpoint::Silenced, Some(SensuPayload {
                    res: Some(r),
                    chk: None,
                    expire: Some(expire.clone()),
                })).map_err(|e| {
                    println!("{}", e);
                });
            }),
            (None, Some(chk)) => chk.into_iter().for_each(|c| {
                let _ = self.request(Method::POST, SensuEndpoint::Silenced, Some(SensuPayload {
                    res: None,
                    chk: Some(c),
                    expire: Some(expire.clone()),
                })).map_err(|e| {
                    println!("{}", e);
                });
            }),
            (_, _) => unreachable!(),
        };
        Ok(())
    }

    pub fn clear(&mut self, s: ClearOpts) -> Result<(), Box<dyn Error>> {
        let resources: Option<Vec<String>> = s.resources
                .and_then(|res| match self.map_to_sensu_resources(res) {
            Ok(vec) => Some(vec.into_iter().map(|r| format!("{}", r)).collect()),
            Err(e) => {
                println!("{}", e);
                None
            },
        });
        let checks = s.checks;
        match (resources, checks) {
            (Some(res), Some(chk)) => iproduct!(res, chk).for_each(|(r, c)| {
                let _ = self.request(Method::POST, SensuEndpoint::Clear, Some(SensuPayload {
                    res: Some(r),
                    chk: Some(c),
                    expire: None,
                })).map_err(|e| {
                    println!("{}", e);
                });
            }),
            (Some(res), None) => res.into_iter().for_each(|r| {
                let _ = self.request(Method::POST, SensuEndpoint::Silenced, Some(SensuPayload {
                    res: Some(r),
                    chk: None,
                    expire: None,
                })).map_err(|e| {
                    println!("{}", e);
                });
            }),
            (None, Some(chk)) => chk.into_iter().for_each(|c| {
                let _ = self.request(Method::POST, SensuEndpoint::Silenced, Some(SensuPayload {
                    res: None,
                    chk: Some(c),
                    expire: None,
                })).map_err(|e| {
                    println!("{}", e);
                });
            }),
            (_, _) => unreachable!(),
        };
        Ok(())
    }

    pub fn list(&mut self, s: ListOpts) -> Result<(), Box<dyn Error>> {
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
    Client(&'a str),
    /// Endpoint for getting check results
    Results,
}

impl<'a> Into<Uri> for SensuEndpoint<'a> {
    fn into(self) -> Uri {
        match self {
            SensuEndpoint::Silenced => "/silenced".parse::<Uri>().expect("Should not get here"),
            SensuEndpoint::Clear => "/silenced/clear".parse::<Uri>().expect("Should not get here"),
            SensuEndpoint::Client(c) => format!("/clients/{}", c).parse::<Uri>()
                .expect("Should not get here"),
            SensuEndpoint::Clients => "/clients".parse::<Uri>().expect("Should not get here"),
            SensuEndpoint::Results => "/results".parse::<Uri>().expect("Should not get here"),
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
#[derive(Clone)]
pub enum SensuResource {
    Client(String),
    Subscription(String),
}

impl Display for SensuResource {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            SensuResource::Client(ref s) => write!(f, "client:{}", s),
            SensuResource::Subscription(ref s) => write!(f, "{}", s),
        }
    }
}
