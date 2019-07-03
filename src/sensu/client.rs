use std::borrow::Borrow;
use std::collections::{HashMap,HashSet};
use std::convert::TryInto;
use std::error::Error;
use std::fmt::Display;
use std::process;

use serde_json::{self,Value,Map};
use hyper::{Body,Client,Method,Request,StatusCode,Uri};
use hyper::client::HttpConnector;
use hyper::header::{self,HeaderValue};
use hyper::rt::{Future,Stream};
use tokio::runtime::Runtime;

use super::*;
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
            -> Result<Option<Value>, SensuError> where U: TryInto<Uri>, U::Error: Display {
        let mut full_uri = uri.try_into().map_err(|e| SensuError::new_string(e))?;
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
            if chunk.len() < 1 {
                return Ok(None);
            }
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

    fn get_node_to_client_map(&mut self) -> Result<HashMap<String, String>, Box<dyn Error>> {
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

    fn map_to_sensu_resources(&mut self, res: ShushResources)
            -> Result<Vec<SensuResource>, Box<dyn Error>>{
        let (resource_type, resources) = (res.res_type, res.resources);
        let mut map = self.get_node_to_client_map()?;
        let mapped_resources = match resource_type {
            ShushResourceType::Node => resources.iter().filter_map(|v| {
                if let Some(val) = map.remove(v) {
                    if self.validate_client(val.as_str()) {
                        Some(SensuResource::Client(val))
                    } else {
                        None
                    }
                } else {
                    println!(r#"WARNING: instance ID "{}" not associated with Sensu client ID"#, v);
                    println!("If you recently provisioned an instance, please wait for it to \
                             register with Sensu");
                    println!();
                    None
                }
            }).collect(),
            ShushResourceType::Sub => {
                let subs = resources.into_iter().map(SensuResource::Subscription).collect();
                self.validate_subscriptions(subs)
            },
            ShushResourceType::Client => resources.into_iter()
                .filter_map(|c| {
                    if self.validate_client(c.as_str()) {
                        Some(SensuResource::Client(c))
                    } else {
                        None
                    }
                }).collect(),
        };
        Ok(mapped_resources)
    }

    fn validate_client(&mut self, client_name: &str) -> bool {
        let resp = self.request(Method::GET, SensuEndpoint::Client(client_name), None);
        if let Err(SensuError::NotFound) = resp {
            return false;
        } else {
            return true;
        }
    }

    fn validate_subscriptions(&mut self, subscriptions: Vec<SensuResource>) -> Vec<SensuResource> {
        let print_error = || {
            println!("Failed to pull data from API for subscriptions");
            println!("Proceeding without subscription validation");
        };

        let resp_res = self.request(Method::GET, SensuEndpoint::Clients, None);
        let resp = match resp_res {
            Err(SensuError::NotFound) => {
                print_error();
                return subscriptions;
            },
            Err(SensuError::Message(s)) => {
                println!("{}", s);
                return subscriptions;
            },
            Ok(resp) => resp,
        };

        let mut subs: HashSet<String> = HashSet::new();
        if let Some(Value::Array(vec)) = resp {
            let iter: Vec<String> = vec.into_iter().filter_map(|obj| {
                obj.as_object().and_then(|o| o.get("subscriptions"))
                    .and_then(|subs| subs.as_array()).map(|arr| {
                        let v: Vec<String> = arr.into_iter()
                            .filter_map(|s| s.as_str().map(|st| st.to_string())).collect();
                        v
                    })
            }).flatten().collect();
            for name in iter {
                subs.insert(name);
            }
        } else {
            print_error();
            return subscriptions;
        };

        subscriptions.into_iter().filter_map(|sub| {
            let string: &String = sub.borrow();
            if subs.contains(string) {
                Some(sub)
            } else {
                None
            }
        }).collect()
    }

    pub fn silence(&mut self, s: SilenceOpts) -> Result<(), Box<dyn Error>> {
        let resources: Option<Vec<String>> = match s.resources {
            Some(res) => Some(self.map_to_sensu_resources(res)?.into_iter()
                .map(|r| format!("{}", r)).collect()),
            None => None,
        };
        let checks = s.checks;
        let expire = s.expire;
        match (resources, checks) {
            (Some(res), Some(chk)) => iproduct!(res, chk).for_each(|(r, c)| {
                println!("Silencing check {} on resource {} and will {}", c, r, expire);
                let _ = self.request(Method::POST, SensuEndpoint::Silenced, Some(SensuPayload {
                    res: Some(r),
                    chk: Some(c),
                    expire: Some(expire.clone()),
                })).map_err(|e| {
                    println!("{}", e);
                    process::exit(1);
                });
            }),
            (Some(res), None) => res.into_iter().for_each(|r| {
                println!("Silencing all checks on resource {} and will {}", r, expire);
                let _ = self.request(Method::POST, SensuEndpoint::Silenced, Some(SensuPayload {
                    res: Some(r),
                    chk: None,
                    expire: Some(expire.clone()),
                })).map_err(|e| {
                    println!("{}", e);
                    process::exit(1);
                });
            }),
            (None, Some(chk)) => chk.into_iter().for_each(|c| {
                println!("Silencing checks {} on all resources and will {}", c, expire);
                let _ = self.request(Method::POST, SensuEndpoint::Silenced, Some(SensuPayload {
                    res: None,
                    chk: Some(c),
                    expire: Some(expire.clone()),
                })).map_err(|e| {
                    println!("{}", e);
                    process::exit(1);
                });
            }),
            (_, _) => {
                println!("No targets specified - Exiting...");
                process::exit(1);
            },
        };
        Ok(())
    }

    pub fn clear(&mut self, s: ClearOpts) -> Result<(), Box<dyn Error>> {
        let resources: Option<Vec<String>> = match s.resources {
            Some(res) => Some(self.map_to_sensu_resources(res)?.into_iter()
                .map(|r| format!("{}", r)).collect()),
            None => None,
        };
        let checks = s.checks;
        match (resources, checks) {
            (Some(res), Some(chk)) => iproduct!(res, chk).for_each(|(r, c)| {
                println!("Clearing silences on checks {} on resources {}", c, r);
                let _ = self.request(Method::POST, SensuEndpoint::Clear, Some(SensuPayload {
                    res: Some(r),
                    chk: Some(c),
                    expire: None,
                })).map_err(|e| {
                    println!("{}", e);
                    process::exit(1);
                });
            }),
            (Some(res), None) => res.into_iter().for_each(|r| {
                println!("Clearing silences on all checks on resources {}", r);
                let _ = self.request(Method::POST, SensuEndpoint::Clear, Some(SensuPayload {
                    res: Some(r),
                    chk: None,
                    expire: None,
                })).map_err(|e| {
                    println!("{}", e);
                    process::exit(1);
                });
            }),
            (None, Some(chk)) => chk.into_iter().for_each(|c| {
                println!("Clearing silences on checks {} on all resources", c);
                let _ = self.request(Method::POST, SensuEndpoint::Clear, Some(SensuPayload {
                    res: None,
                    chk: Some(c),
                    expire: None,
                })).map_err(|e| {
                    println!("{}", e);
                    process::exit(1);
                });
            }),
            (_, _) => {
                println!("No targets specified - Exiting...");
                process::exit(1);
            },
        };
        Ok(())
    }

    pub fn list(&mut self, s: ListOpts) -> Result<(), Box<dyn Error>> {
        let resp = self.request(Method::GET, SensuEndpoint::Silenced, None)?;
        if let Some(Value::Array(v)) = resp {
            for obj in v {
                if let Value::Object(o) = obj {
                    let user = o.get("creator").and_then(|c| c.as_str()).unwrap_or("unknown");
                    let subscription = o.get("subscription").and_then(|c| c.as_str())
                        .unwrap_or("all");
                    let check = o.get("check").and_then(|c| c.as_str()).unwrap_or("all");
                    let expire = o.get("expire").and_then(|c| c.as_u64());
                    let eor = o.get("expire_on_resolve").and_then(|c| c.as_bool()).unwrap_or(false);

                    println!("subscription:\t\t{}", subscription);
                    println!("Check:\t\t\t{}", check);
                    match expire {
                        Some(num) => println!("Expiration:\t\t{}", num),
                        None => println!("Expiration:\t\tnever"),
                    };
                    println!("Expire on resolve:\t{}", eor);
                    println!("User:\t\t\t{}", user);
                    println!();
                }
            }
        }
        Ok(())
    }
}
