//! Generates Shush data structures for `sensu` module from command line flags
use std::collections::{HashSet,HashMap};
use std::error::Error;
use std::fmt::{self,Display};
use std::fmt::Write;
use std::vec;

use getopts;
use hyper::Method;
use hyper::StatusCode;
use regex::Regex;
use serde_json::Value;
use teatime::{ApiClient,JsonApiClient};
use teatime::sensu::SensuClient;

#[cfg(not(test))]
use std::process;

use config::ShushConfig;
use err::SensuError;
use json;
use resources::ShushResources;
use sensu::*;

#[cfg(test)]
mod process {
    pub fn exit(exit_code: u32) -> ! {
        panic!(format!("Panicked with exit code {}", exit_code))
    }
}

/// Struct resprenting parameters passed to Shush
#[derive(PartialEq,Debug)]
pub enum ShushOpts {
    /// Silence action
    Silence {
        /// Resource to silence (AWS node, client or subscription)
        resource: Option<ShushResources>,
        /// Check to silence
        checks: Option<Vec<String>>,
        /// Expiration
        expire: Expire
    },
    /// Clear action
    Clear {
        /// Resource for which to clear silence (AWS node, client or subscription)
        resource: Option<ShushResources>,
        /// Check for which to clear silence
        checks: Option<Vec<String>>,
    },
    /// List action
    List {
        /// Regex string for subscription or client list filtering
        sub: Option<String>,
        /// Regex string for check list filtering
        chk: Option<String>,
    }
}

impl ShushOpts {
    fn iid_mapper(client: &mut SensuClient, iids: Vec<String>) -> Result<Vec<Value>, Box<Error>> {
        let uri = SensuEndpoint::Clients.into();
        let mut clients = client.request_json::<Value>(Method::Get, uri, None)?;

        // Generate map from array of JSON objects - from [{"name": CLIENT_ID, "instance_id": IID},..] to
        // {IID1: CLIENT_ID1, IID2: CLIENT_ID2,...}
        let mut map = HashMap::new();
        if let Value::Array(ref mut v) = clients {
            for mut item in v.drain(..) {
                let iid = if let Some(Value::String(string)) = item
                        .as_object_mut().and_then(|obj| obj.remove("instance_id")) {
                    Some(string)
                } else {
                    None
                };
                let client = if let Some(Value::String(string)) = item
                        .as_object_mut().and_then(|obj| obj.remove("name")) {
                    Some(string)
                } else {
                    None
                };
                if let (Some(i), Some(c)) = (iid, client) {
                    map.insert(i, c);
                }
            }
            Ok(iids.iter().fold(Vec::new(), |mut acc, v| {
                if let Some(val) = map.remove(v) {
                    acc.push(Value::from(val));
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

    fn validate_client(client: &mut SensuClient, item: &String) -> bool {
        let uri = SensuEndpoint::Client(item).into();
        match client.request::<Value>(Method::Get, uri, None) {
            Ok(resp) => {
                if resp.status() == StatusCode::NotFound {
                    println!("You may have misspelled a resource name");
                    println!("\tResource {} not found - verify that you silenced what you want\n", item);
                    return false;
                }
                match client.response_to_json(resp) {
                    Ok(_) => true,
                    Err(e) => {
                        println!("Failed to validate check {}: {}", item, e);
                        false
                    }
                }
            },
            Err(e) => {
                println!("Failed to make API request to validate resource: {}", e);
                false
            }
        }
    }

    fn get_client_set(client: &mut SensuClient) -> Option<HashSet<String>> {
        let uri = SensuEndpoint::Clients.into();
        let resp = client.request_json::<Value>(Method::Get, uri, None);
        let mut set = HashSet::new();
        if let Ok(Value::Array(vec)) = resp {
            vec.into_iter().for_each(|mut response_item| {
                if let Some(Value::Array(sub_vec)) = response_item.as_object_mut()
                        .and_then(|map| map.remove("subscriptions")) {
                    sub_vec.into_iter().for_each(|sub| {
                        if let Value::String(string) = sub {
                            set.insert(string);
                        }
                    });
                }
            });
        } else if let Err(e) = resp {
            println!("{}", e);
            return None;
        }
        Some(set)
    }

    fn validate_resources(&mut self, client: &mut SensuClient) -> Result<(), Box<Error>> {
        let resources_to_validate = match self {
            ShushOpts::Silence { checks: _, ref mut resource, expire: _ } => resource,
            ShushOpts::Clear { checks: _, ref mut resource, } => resource,
            _ => return Ok(()),
        };
        let is_client = match resources_to_validate.as_ref().map(|r| r.is_client()) {
            Some(b) => b,
            None => return Ok(()),
        };
        let set = if !is_client {
            Self::get_client_set(client)
        } else {
            None
        };

        resources_to_validate.as_mut().map(|r| {
            r.retain(|item| {
                if is_client {
                    Self::validate_client(client, item)
                } else {
                    if let Some(ref s) = set {
                        if s.contains(item) {
                            true
                        } else {
                            println!("You may have misspelled a subscription name");
                            println!("\tSubscription {} not found - verify that you silenced what you want\n", item);
                            false
                        }
                    } else {
                        false
                    }
                }
            });
        });
        Ok(())
    }

    fn validate_checks(&mut self, client: &mut SensuClient) -> Result<(), Box<Error>> {
        let checks_to_validate = match self {
            ShushOpts::Silence { ref mut checks, resource: _, expire: _ } => checks,
            ShushOpts::Clear { ref mut checks, resource: _, } => checks,
            _ => return Ok(()),
        };
        if checks_to_validate.is_none() {
            return Ok(());
        }
        let uri = SensuEndpoint::Results.into();
        let checks_api_resp = client.request_json::<Value>(Method::Get, uri, None)?;
        let (check_iter, check_len) = match checks_api_resp {
            Value::Array(v) => {
                let len = v.len();
                (v.into_iter().filter_map(|json_obj| {
                    if let Some(Value::String(string)) = json::remove_fold(json_obj, "check.name") {
                        Some(string)
                    } else {
                        None
                    }
                }), len)
            },
            _ => { return Err(Box::new(SensuError::new("Invalid JSON schema returned for check list"))); },
        };
        let mut set = HashSet::with_capacity(check_len);
        set.extend(check_iter);

        match checks_to_validate.as_mut() {
            Some(v) => {
                v.retain(|item| {
                    if let true = set.contains(item) {
                        true
                    } else {
                        println!("You may have misspelled a check name");
                        println!("\tCheck {} not found - verify that you silenced what you want\n", item);
                        false
                    }
                });
            },
            None => (),
        };
        Ok(())
    }

    /// Takes a mutable `SensuClient` reference and performs mapping from instance ID to Sensu
    /// client ID
    pub fn map_and_validate(mut self, client: &mut SensuClient) -> Result<Self, Box<Error>> {
        self.validate_resources(client)?;
        self.validate_checks(client)?;
        let opts = match self {
            ShushOpts::Silence {
                resource: Some(ShushResources::Node(v)),
                checks,
                expire,
            } => {
                ShushOpts::Silence {
                    resource: match Self::iid_mapper(client, v) {
                        Ok(ref mut cli_v) => Some(ShushResources::Client(
                            cli_v.drain(..).filter_map(|item| {
                                if let Value::String(string) = item {
                                    Some(string)
                                } else {
                                    None
                                }
                            }).collect()
                        )),
                        Err(e) => {
                            println!("Error mapping instance IDs to client names: {}", e);
                            None
                        }
                    },
                    checks,
                    expire,
                }
            },
            ShushOpts::Clear {
                resource: Some(ShushResources::Node(v)),
                checks,
            } => {
                ShushOpts::Clear {
                    resource: match Self::iid_mapper(client, v) {
                        Ok(ref mut cli_v) => Some(ShushResources::Client(
                            cli_v.drain(..).filter_map(|item| {
                                if let Value::String(string) = item {
                                    Some(string)
                                } else {
                                    None
                                }
                            }).collect()
                        )),
                        Err(e) => {
                            println!("Error mapping instance IDs to client names: {}", e);
                            None
                        }
                    },
                    checks,
                }
            },
            opts => opts,
        };
        Ok(opts)
    }
}

impl Display for ShushOpts {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ShushOpts::Silence { ref resource, ref checks, ref expire } => {
                let resource_result = resource.as_ref().map_or(Ok(Some("All resources".to_string())), |val| {
                    let mut string = String::new();
                    match write!(&mut string, "{}", val) {
                        Ok(()) => (),
                        Err(fmt::Error) => return Err(fmt::Error),
                    };
                    Ok(Some(string))
                });
                let check_result = checks.as_ref().map_or(Ok(Some("All checks".to_string())), |val| {
                    if val.len() > 0 {
                        Ok(Some(format!("Checks: {}", val.join(", "))))
                    } else {
                        Err(fmt::Error)
                    }
                });
                match (resource_result, check_result) {
                    (Ok(Some(string1)), Ok(Some(string2))) => {
                        write!(f, "Silencing...\n\tResources: {}\n\t{}\n\t{}\n", string1, string2, expire)
                    },
                    (Ok(None), Ok(Some(string2))) => {
                        write!(f, "Silencing...\n\tResources: All resources\n\t{}\n\t{}\n", string2, expire)
                    },
                    (Ok(Some(string1)), Ok(None)) => {
                        write!(f, "Silencing...\n\tResources: {}\n\tChecks: All checks\n\t{}\n", string1, expire)
                    },
                    (_, _) => {
                        write!(f, "Silencing...\n\tFAILED! Either all resources or all checks were invalid. Exiting...")
                    },
                }
            },
            ShushOpts::Clear { ref resource, ref checks } => {
                let resource_result = resource.as_ref().map_or(Ok(Some("All resources".to_string())), |val| {
                    let mut string = String::new();
                    match write!(&mut string, "{}", val) {
                        Ok(()) => (),
                        Err(fmt::Error) => return Err(fmt::Error),
                    };
                    Ok(Some(string))
                });
                let check_result = checks.as_ref().map_or(Ok(Some("All checks".to_string())), |val| {
                    if val.len() > 0 {
                        Ok(Some(format!("Checks: {}", val.join(", "))))
                    } else {
                        Err(fmt::Error)
                    }
                });
                match (resource_result, check_result) {
                    (Ok(Some(string1)), Ok(Some(string2))) => {
                        write!(f, "Clearing...\n\tResources: {}\n\t{}\n", string1, string2)
                    },
                    (Ok(None), Ok(Some(string2))) => {
                        write!(f, "Clearing...\n\tResources: All resources\n\t{}\n", string2)
                    },
                    (Ok(Some(string1)), Ok(None)) => {
                        write!(f, "Clearing...\n\tResources: {}\n\tChecks: All checks\n", string1)
                    },
                    (_, _) => {
                        write!(f, "Clearing...\n\tFAILED! Either all resources or all checks were invalid. Exiting...")
                    },
                }
            },
            ShushOpts::List { sub: _, chk: _ } => {
                Ok(())
            }
        }
    }
}

impl IntoIterator for ShushOpts {
    type Item = SensuPayload;
    type IntoIter = vec::IntoIter<SensuPayload>;

    fn into_iter(self) -> Self::IntoIter {
        let match_iters = |r_iter: Option<ShushResources>,
                           c_iter: Option<Vec<String>>,
                           expire: Option<Expire>| {
            let fold_closure = |mut acc: Vec<SensuPayload>, res, chk, expire| {
                acc.push(SensuPayload { res, chk, expire });
                acc
            };

            let vec = match (
                r_iter.map(|x| { x.into_iter() }),
                c_iter.map(|x| { x.into_iter() })
            ) {
                (Some(i1), Some(i2)) =>
                    iproduct!(i1, i2).fold(Vec::new(), |acc, (res, chk)| {
                        fold_closure(acc, Some(res), Some(chk), expire.clone())
                    }),
                (Some(i1), _) => i1.fold(Vec::new(), |acc, res| {
                    fold_closure(acc, Some(res), None, expire.clone())
                }),
                (_, Some(i2)) => i2.fold(Vec::new(), |acc, chk| {
                    fold_closure(acc, None, Some(chk), expire.clone())
                }),
                (_, _) => Vec::new(),
            };
            vec.into_iter()
        };

        match self {
            ShushOpts::Silence { resource, checks, expire } => {
                match_iters(resource, checks, Some(expire))
            },
            ShushOpts::Clear { resource, checks } => {
                match_iters(resource, checks, None)
            },
            _ => {
                unimplemented!()
            },
        }
    }
}

fn parse_clock_time(e: String) -> Expire {
    let mut time_fields: Vec<&str> = e.splitn(3, ':').collect();
    time_fields.reverse();
    let seconds = time_fields.iter().enumerate().fold(0, |acc, (index, val)| {
        let base: usize = 60;
        acc + base.pow(index as u32) * val.parse::<usize>().unwrap_or_else(|e| {
            println!("Invalid time format: {}", e);
            process::exit(1);
        })
    });
    Expire::Expire(seconds)
}

fn parse_hms_time(e: String) -> Expire {
    let re = Regex::new("(?P<num>[0-9]+)(?P<units>[dhms])").unwrap_or_else(|e| {
        println!("Invalid time format: {}", e);
        process::exit(1);
    });
    let num_secs = re.captures_iter(e.as_str()).fold(0, |acc, cap| {
        let num = cap.name("num").map(|val| val.as_str().parse::<usize>().unwrap_or(0));
        let units = cap.name("units").map(|val| val.as_str());
        acc + match (num, units) {
            (Some(n), Some("d")) => n * 86400,
            (Some(n), Some("h")) => n * 3600,
            (Some(n), Some("m")) => n * 60,
            (Some(n), Some("s")) => n,
            _ => 0,
        }
    });
    Expire::Expire(num_secs)
}

// Match and parse expiration value
fn match_expire(e_val: Option<String>, o_val: bool, usage_message: String)
                    -> Expire {
    match (e_val, o_val) {
        (Some(e), false) => {
            if e == "none" {
                Expire::NoExpiration
            } else if e.contains(':') {
                parse_clock_time(e)
            } else if e.contains('d') || e.contains('h') || e.contains('m') || e.contains('s') {
                parse_hms_time(e)
            } else {
                let exp = e.parse::<usize>().unwrap_or(7200);
                if exp < 1 {
                    println!("Expiration value must be greater than 0");
                    println!("{}", usage_message);
                    process::exit(1);
                }
                Expire::Expire(exp)
            }
        }
        (None, false) => Expire::Expire(7200),
        (None, true) => Expire::ExpireOnResolve,
        (_, _) => {
            println!("-e and -o cannot be used together");
            println!("{}", usage_message);
            process::exit(1);
        },
    }
}

// Match and parse resource
#[inline]
fn match_resource(n: Option<String>, i: Option<String>, s: Option<String>)
                  -> Option<ShushResources> {
    match (n, i, s) {
        (Some(st), None, None) => Some(
            ShushResources::Node(st.split(",").map(|val| val.to_string()).collect())
        ),
        (None, Some(st), None) => Some(
            ShushResources::Client(st.split(",").map(|val| val.to_string()).collect())
        ),
        (None, None, Some(st)) => Some(
            ShushResources::Sub(st.split(",").map(|val| val.to_string()).collect())
        ),
        (_, _, _) => None,
    }
}

// Match and parse argument into `Vec`
#[inline]
fn match_comma_sep_args(res: Option<String>) -> Option<Vec<String>> {
    match res {
        Some(ref st) => Some(st.split(",").map(|x| { x.to_string() }).collect()),
        _ => None,
    }
}

fn opt_validation(n_opt: Option<String>, i_opt: Option<String>,
                  s_opt: Option<String>, c_opt: Option<String>,
                  expire: Expire, list: bool, remove: bool)
                  -> ShushOpts {
    // Helper closure for counting if more than one resource is present
    let check_resource_count = |n_opt: &Option<String>,
                                i_opt: &Option<String>,
                                s_opt: &Option<String>| {
        let opts = [n_opt, i_opt, s_opt];
        let v: Vec<_> = opts.iter().filter(|val| val.is_some()).collect();
        v.len() > 1
    };

    // Do actual work around checking validity of arguments passed and doing the
    // corresponding actions
    if list == true && remove == true {
        println!("Cannot use -l and -r together");
        process::exit(1);
    } else if list == true {
        if let Some(_) = n_opt {
            println!("-n cannot be used with -l");
            process::exit(1);
        } else {
            ShushOpts::List {
                sub: s_opt,
                chk: c_opt,
            }
        }
    } else if remove == true {
        if let (None, None, None, None) =
                (n_opt.as_ref(), i_opt.as_ref(), s_opt.as_ref(), c_opt.as_ref()) {
            println!("Must specify at least -n, -i, -s, or -c");
            process::exit(1);
        } else if check_resource_count(&n_opt, &i_opt, &s_opt) {
            println!("Must specify only one of the following: -n, -s, or -i");
            process::exit(1);
        } else {
            ShushOpts::Clear {
                resource: match_resource(n_opt, i_opt, s_opt),
                checks: match_comma_sep_args(c_opt),
            }
        }
    } else {
        if let (None, None, None, None) =
                (n_opt.as_ref(), i_opt.as_ref(), s_opt.as_ref(), c_opt.as_ref()) {
            println!("Must specify at least -n, -i, -s or -c");
            process::exit(1);
        } else if check_resource_count(&n_opt, &i_opt, &s_opt) {
            println!("Must specify only one of the following: -n, -s, or -i");
            process::exit(1);
        } else {
            ShushOpts::Silence {
                resource: match_resource(n_opt, i_opt, s_opt),
                checks: match_comma_sep_args(c_opt),
                expire,
            }
        }
    }
}

/// Do work for parsing and acting on CLI args
pub fn getopts(args: Vec<String>) -> (ShushOpts, ShushConfig) {
    let mut opts = getopts::Options::new();
    opts.optflag("h", "help", "Help text")
        .optflag("l", "list", "List resources specified")
        .optopt("f", "config-file", "Point to INI config file", "FILE_PATH")
        .optflag("r", "remove", "Remove specified silences")
        .optopt("e", "expire", "Seconds until expiration or \"none\" for unlimited TTL", "EXPIRE")
        .optflag("o", "expire-on-resolve", "On resolution of alert, clear silence")
        .optopt("c", "checks", "Checks to silence", "CHECKS")
        .optopt("n", "aws-nodes", "Comma separated list of instance IDs", "INST_IDS")
        .optopt("i", "client-id", "Comma separated list of client IDs", "CLIENT_IDS")
        .optopt("s", "subscriptions", "Comma separated list of subscriptions", "SUBSCRIPTIONS")
        .optflag("v", "version", "shush version");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(e) => {
            println!("{}", e);
            process::exit(1);
        },
    };
    // Detect flags that print output and exit
    if matches.opt_present("h") {
        println!("{}", opts.usage(args[0].as_str()));
        process::exit(0);
    } else if matches.opt_present("v") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        process::exit(0);
    }

    // Check presence of other action flags and get expiration value or use default
    let expire = match_expire(matches.opt_default("e", "7200"), matches.opt_present("o"),
                              opts.usage(args[0].as_str()));
    let list = matches.opt_present("l");
    let remove = matches.opt_present("r");

    let n_opt = matches.opt_str("n");
    let i_opt = matches.opt_str("i");
    let s_opt = matches.opt_str("s");
    let c_opt = matches.opt_str("c");

    // Parse config
    let config = ShushConfig::new(matches.opt_str("f"));
    (opt_validation(n_opt, i_opt, s_opt, c_opt, expire, list, remove), config)
}

#[cfg(test)]
mod test {
    use super::*;
    use std::env;

    #[test]
    #[should_panic]
    fn getopts_exit_no_action() {
        getopts(vec!["-f", format!("{}/{}", env!("CARGO_MANIFEST_DIR"), "cfg/shush.conf").as_ref()].iter().map(|x| { x.to_string() }).collect());
    }

    #[test]
    #[should_panic]
    fn getopts_exit_s_n() {
        getopts(vec!["shush", "-f", format!("{}/{}", env!("CARGO_MANIFEST_DIR"), "cfg/shush.conf").as_ref(), "-s", "sub", "-n", "i-something"].iter().map(|x| { x.to_string() }).collect());
    }

    #[test]
    #[should_panic]
    fn getopts_exit_l_r() {
        getopts(vec!["shush", "-f", format!("{}/{}", env!("CARGO_MANIFEST_DIR"), "cfg/shush.conf").as_ref(), "-l", "-r"].iter().map(|x| { x.to_string() }).collect());
    }

    #[test]
    fn getopts_n_c_split_and_eor() {
        let (shush_opts, _) = getopts(vec!["shush", "-f", format!("{}/{}", env!("CARGO_MANIFEST_DIR"), "cfg/shush.conf").as_ref(), "-o", "-c", "a,b,c", "-n", "a,b,c"].iter().map(|x| { x.to_string() }).collect());
        assert_eq!(shush_opts, ShushOpts::Silence {
            resource: Some(ShushResources::Node(vec!["a", "b", "c"].iter().map(|x| { x.to_string() }).collect())),
            checks: Some(vec!["a", "b", "c"].iter().map(|x| { x.to_string() }).collect()),
            expire: Expire::ExpireOnResolve,
        })
    }

    #[test]
    fn getopts_s_c_split_and_no_expiration() {
        let (shush_opts, _) = getopts(vec!["shush", "-f", format!("{}/{}", env!("CARGO_MANIFEST_DIR"), "cfg/shush.conf").as_ref(), "-e", "none", "-c", "a,b,c", "-s", "a,b,c"].iter().map(|x| { x.to_string() }).collect());
        assert_eq!(shush_opts, ShushOpts::Silence {
            resource: Some(ShushResources::Sub(vec!["a", "b", "c"].iter().map(|x| { x.to_string() }).collect())),
            checks: Some(vec!["a", "b", "c"].iter().map(|x| { x.to_string() }).collect()),
            expire: Expire::NoExpiration,
        })
    }

    #[test]
    fn getopts_s_c_split() {
        let (shush_opts, _) = getopts(vec!["shush", "-f", format!("{}/{}", env!("CARGO_MANIFEST_DIR"), "cfg/shush.conf").as_ref(), "-c", "a,b,c", "-s", "a,b,c"].iter().map(|x| { x.to_string() }).collect());
        assert_eq!(shush_opts, ShushOpts::Silence{
            resource: Some(ShushResources::Sub(vec!["a", "b", "c"].iter().map(|x| { x.to_string() }).collect())),
            checks: Some(vec!["a", "b", "c"].iter().map(|x| { x.to_string() }).collect()),
            expire: Expire::Expire(7200),
        })
    }

    #[test]
    fn getopts_s_c_split_remove() {
        let (shush_opts, _) = getopts(vec!["shush", "-f", format!("{}/{}", env!("CARGO_MANIFEST_DIR"), "cfg/shush.conf").as_ref(), "-r", "-c", "a,b,c", "-s", "a,b,c"].iter().map(|x| { x.to_string() }).collect());
        assert_eq!(shush_opts, ShushOpts::Clear {
            resource: Some(ShushResources::Sub(vec!["a", "b", "c"].iter().map(|x| { x.to_string() }).collect())),
            checks: Some(vec!["a", "b", "c"].iter().map(|x| { x.to_string() }).collect())
        })
    }

    #[test]
    fn getopts_expire_flag() {
        let (shush_opts, _) = getopts(
            vec!["shush", "-f", format!("{}/{}", env!("CARGO_MANIFEST_DIR"), "cfg/shush.conf").as_ref(), "-n", "a,b,c", "-e", "200"].iter().map(|x| { x.to_string() }).collect()
        );
        assert_eq!(shush_opts, ShushOpts::Silence {
            resource: Some(ShushResources::Node(vec!["a", "b", "c"].iter().map(|x| { x.to_string() }).collect())),
            checks: None,
            expire: Expire::Expire(200),
        })
    }

    #[test]
    fn getopts_f_expand() {
        env::set_var("ENV", "dev");
        let (_, config) = getopts(
            vec!["shush", "-f", format!("{}/{}", env!("CARGO_MANIFEST_DIR"), "cfg/shush.conf").as_ref(), "-n", "a"].iter().map(|x| { x.to_string() }).collect()
        );
        assert_eq!(Some("http://your.dev.here".to_string()), config.get("api"));
    }

    #[test]
    fn getopts_expire_flag_invalid() {
        let (shush_opts, _) = getopts(vec!["shush", "-f", format!("{}/{}", env!("CARGO_MANIFEST_DIR"), "cfg/shush.conf").as_ref(), "-n", "a,b,c", "-e", "200b"].iter().map(|x| { x.to_string() }).collect());
        assert_eq!(shush_opts, ShushOpts::Silence{
            resource: Some(ShushResources::Node(vec!["a", "b", "c"].iter().map(|x| { x.to_string() }).collect())),
            checks: None,
            expire: Expire::Expire(7200),
        })
    }

    #[test]
    fn getopts_expire_fmt() {
        let (shush_opts, _) = getopts(vec!["shush", "-f", format!("{}/{}", env!("CARGO_MANIFEST_DIR"), "cfg/shush.conf").as_ref(), "-n", "a,b,c", "-e", "10:10:1"].iter().map(|x| { x.to_string() }).collect());
        assert_eq!(shush_opts, ShushOpts::Silence {
            resource: Some(ShushResources::Node(vec!["a", "b", "c"].iter().map(|x| { x.to_string() }).collect())),
            checks: None,
            expire: Expire::Expire(36601),
        })
    }

    #[test]
    fn getopts_expire_fmt_hms() {
        let (shush_opts, _) = getopts(vec!["shush", "-f", format!("{}/{}", env!("CARGO_MANIFEST_DIR"), "cfg/shush.conf").as_ref(), "-n", "a,b,c", "-e", "1d10m1s"].iter().map(|x| { x.to_string() }).collect());
        assert_eq!(shush_opts, ShushOpts::Silence {
            resource: Some(ShushResources::Node(vec!["a", "b", "c"].iter().map(|x| { x.to_string() }).collect())),
            checks: None,
            expire: Expire::Expire(87001),
        })
    }

    #[test]
    #[should_panic]
    fn getopts_expire_fmt_invalid() {
       getopts(vec!["shush", "-n", "a,b,c", "-e", "10:10oops:1"].iter().map(|x| { x.to_string() }).collect());
    }
}
