//! Generates Shush data structures for `sensu` module from command line flags
use std::vec;
use std::fmt::{self,Display};
use std::collections::HashMap;
use std::path::Path;
use std::env;

use getopts;
use hyper::Method;
use regex::Regex;
use ini::Ini;
use nom::rest_s;
use teatime::ApiClient;
use teatime::sensu::SensuClient;
use serde_json::Value;

#[cfg(not(test))]
use std::process;

use sensu::*;
use err::SensuError;
use json::JsonRef;

#[cfg(test)]
mod process {
    pub fn exit(exit_code: u32) -> ! {
        panic!(format!("Panicked with exit code {}", exit_code))
    }
}

named!(expand_vars<&str, String>, fold_many0!(
       alt!(
           do_parse!(pretext: take_until!("${") >>
                     tag!("${") >>
                     env: take_until!("}") >>
                     tag!("}") >>
                     (pretext.to_string() +
                      env::var(env)
                      .unwrap_or("".to_string()).as_str()
                     ))
           | map!(rest_s, String::from)
       ), String::new(), |acc, string: String| {
           acc + string.as_str()
       }));

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
    /// Return boolean indicating whether resource is an AWS node or not
    pub fn resource_is_node(&self) -> bool {
        match *self {
            ShushOpts::Silence {
                resource: Some(ShushResources::Node(_)), checks: _, expire: _
            } => true,
            ShushOpts::Clear {
                resource: Some(ShushResources::Node(_)), checks: _,
            } => true,
            _ => false,
        }
    }

    fn mapper(client: &mut SensuClient, iids: Vec<String>) -> Result<Vec<Value>, SensuError> {
        let clients = client.api_request(Method::Get, SensuEndpoint::Clients, None)?;

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

    /// Takes a mutable `SensuClient` reference and performs mapping from instance ID to Sensu
    /// client ID
    pub fn iid_mapper(self, client: &mut SensuClient) -> Self {
        match self {
            ShushOpts::Silence {
                resource: Some(ShushResources::Node(v)),
                checks,
                expire,
            } => {
                ShushOpts::Silence {
                    resource: match Self::mapper(client, v) {
                        Ok(cli_v) => Some(ShushResources::Client(
                            cli_v.into_iter()
                                .filter_map(|item| item.as_str().map(|val| val.to_string()))
                                .collect()
                        )),
                        _ => None,
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
                    resource: match Self::mapper(client, v) {
                        Ok(cli_v) => Some(ShushResources::Client(
                            cli_v.into_iter()
                                .filter_map(|item| item.as_str().map(|val| val.to_string()))
                                .collect()
                        )),
                        _ => None,
                    },
                    checks,
                }
            },
            _ => unimplemented!(),
        }
    }
}

impl Display for ShushOpts {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ShushOpts::Silence { ref resource, ref checks, ref expire } => {
                write!(f, "Issuing silences on the following checks on the following resources.\n\t{}\n\t{}\n\tThese silences {}",
                       resource.as_ref().map_or("All resources".to_string(), |val| val.to_string()),
                       checks.as_ref().map_or("All checks".to_string(), |val|
                                              format!("Checks: {}", if val.len() > 0 {
                                                  val.join(", ")
                                              } else {
                                                  "None".to_string()
                                              })),
                       expire.to_string())
            },
            ShushOpts::Clear { ref resource, ref checks } => {
                write!(f, "Clearing silences on the following checks on the following resources.\n\t{}\n\t{}",
                       resource.as_ref().map_or("All resources".to_string(), |val| val.to_string()),
                       checks.as_ref().map_or("All checks".to_string(), |val|
                                              format!("Checks: {}", if val.len() > 0 {
                                                  val.join(", ")
                                              } else {
                                                  "None".to_string()
                                              })))
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

/// Enum representing Shush target resource (AWS node, Sensu client, or subscription)
#[derive(PartialEq,Debug)]
pub enum ShushResources {
    /// AWS node
    Node(Vec<String>),
    /// Sensu client ID
    Client(Vec<String>),
    /// Sensu subscription
    Sub(Vec<String>),
}

impl Display for ShushResources {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ShushResources::Node(ref v) => write!(f, "Instance IDs: {}", if v.len() > 0 {
                v.join(", ")
            } else {
                "None".to_string()
            }),
            ShushResources::Client(ref v) => write!(f, "Sensu clients: {}", if v.len() > 0 {
                v.join(", ")
            } else {
                "None".to_string()
            }),
            ShushResources::Sub(ref v) => write!(f, "Roles: {}", if v.len() > 0 {
                v.join(", ")
            } else {
                "None".to_string()
            }),
        }
    }
}


impl IntoIterator for ShushResources {
    type Item = SensuResource;
    type IntoIter = vec::IntoIter<SensuResource>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            ShushResources::Client(vec) => vec.into_iter()
                .map(SensuResource::Client)
                .collect::<Vec<SensuResource>>().into_iter(),
            ShushResources::Sub(vec) => vec.into_iter()
                .map(SensuResource::Sub)
                .collect::<Vec<SensuResource>>().into_iter(),
            ShushResources::Node(_) => {
                unimplemented!()
            },
        }
    }
}

/// Struct representing shush config file
#[derive(Debug)]
pub struct ShushConfig(HashMap<String, String>);

impl ShushConfig {
    /// Initialize ShushConfig with path
    pub fn new(path: Option<String>) -> Self {
        let parse_config = |path: String| {
            let mut hm = HashMap::new();
            for (_, prop) in Ini::load_from_file(path.as_str()).unwrap_or_else(|e| {
                println!("Failed to parse INI file: {}", e);
                process::exit(1);
            }) {
                for (k, v) in &prop {
                    if k == "api" {
                        hm.insert(k.to_string(), v.to_string());
                    }
                }
            }
            ShushConfig(hm)
        };
        let home_config = format!("{}/.shush/shush.conf", env::var("HOME").unwrap_or_else(|_| {
            println!("$HOME environment variable not found - \
                     defaulting to ~/.shush/shush.conf and this expansion may or may not work");
            "~/.shush/shush.conf".to_string()
        }));

        if path.is_some() && Path::new(path.as_ref().unwrap().as_str()).is_file() {
            parse_config(path.unwrap())
        } else if Path::new("/etc/shush/shush.conf").is_file() {
            parse_config("/etc/shush/shush.conf".to_string())
        } else if Path::new(home_config.as_str()).is_file() {
            parse_config(home_config)
        } else {
            println!("No config found - exiting");
            process::exit(1);
        }
    }

    /// Get config option from ShushConfig object
    pub fn get(&self, key: &str) -> Option<String> {
        self.0.get(key).and_then(|val| expand_vars(val.as_str()).to_result().ok())
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
