//! Generates Shush data structures for `sensu` module from command line flags

use std::process;
use std::vec;

use clap::{App,Arg};
use regex::Regex;

use config::ShushConfig;
use resources::{ShushResources,ShushResourceIterator,ShushResourceType};
use sensu::Expire;

pub struct SilenceOpts {
    pub resources: Option<ShushResources>,
    pub checks: Option<Vec<String>>,
    pub expire: Expire,
}

impl IntoIterator for SilenceOpts {
    type Item = (String, String);
    type IntoIter = itertools::Product<ShushResourceIterator, vec::IntoIter<String>>;

    fn into_iter(self) -> Self::IntoIter {
        let resources = self.resources.map(|r| r.into_iter()).unwrap_or(
            ShushResources { res_type: ShushResourceType::Wildcard, resources: vec![String::new()] }
                .into_iter()
        );
        let checks = self.checks.map(|c| c.into_iter()).unwrap_or(vec![String::new()].into_iter());
        iproduct!(resources.into_iter(), checks.into_iter())
    }
}

pub struct ClearOpts {
    resources: Option<ShushResources>,
    checks: Option<Vec<String>>,
}

pub struct ListOpts {
    sub: Option<String>,
    chk: Option<String>,
}

pub enum ShushOpts {
    Silence(SilenceOpts),
    Clear(ClearOpts),
    List(ListOpts),
}

pub fn get_expiration(expire: String, expire_on_resolve: bool) -> Expire {
    if expire.as_str() == "none" {
        return Expire::NoExpiration(expire_on_resolve);
    }
    let regex = Regex::new("(?P<num>[0-9]+)(?P<units>[dhms])").unwrap_or_else(|e| {
        println!("Failed to compile regex: {}", e);
        process::exit(1);
    });
    let num_secs = regex.captures_iter(expire.as_str()).fold(0, |acc, cap| {
        let num = cap.name("num").map(|val| val.as_str().parse::<usize>().unwrap_or(0));
        let units = cap.name("units").map(|val| val.as_str());
        acc + match (num, units) {
            (Some(n), Some("d")) => n * 60 * 60 * 24,
            (Some(n), Some("h")) => n * 60 * 60,
            (Some(n), Some("m")) => n * 60,
            (Some(n), Some("s")) => n,
            _ => 60 * 60 * 2,
        }
    });
    Expire::Expire(num_secs, expire_on_resolve)
}

pub struct Args<'a>(clap::ArgMatches<'a>);

impl<'a> Args<'a> {
    pub fn new() -> Self {
        Args(App::new("shush").version(env!("CARGO_PKG_VERSION"))
            .author("John Baublitz")
            .about("Sensu silencing tool")
            .arg(Arg::with_name("nodes")
                 .short("n")
                 .long("aws-nodes")
                 .value_name("NODE1,NODE2,...")
                 .help("Comma separated list of instance IDs")
                 .takes_value(true)
                 .conflicts_with_all(&["ids", "subscriptions"]))
            .arg(Arg::with_name("ids")
                 .short("i")
                 .long("client-ids")
                 .value_name("ID1,ID2,...")
                 .help("Comma separated list of client IDs")
                 .takes_value(true)
                 .conflicts_with_all(&["nodes", "subscriptions"]))
            .arg(Arg::with_name("subscriptions")
                 .short("s")
                 .long("subscriptions")
                 .value_name("SUB1,SUB2,...")
                 .help("Comma separated list of subscriptions")
                 .takes_value(true)
                 .conflicts_with_all(&["nodes", "ids"]))
            .arg(Arg::with_name("remove")
                 .short("r")
                 .long("remove")
                 .takes_value(false)
                 .help("Remove specified silences"))
            .arg(Arg::with_name("list")
                 .short("l")
                 .long("list")
                 .takes_value(false)
                 .help("List silences"))
            .arg(Arg::with_name("checks")
                 .short("c")
                 .long("checks")
                 .takes_value(true)
                 .help("Comma separated list of checks")
                 .value_name("CHK1,CHK2,..."))
            .arg(Arg::with_name("expire")
                 .short("e")
                 .long("expire")
                 .help("Time until check should expire or \"none\" for unlimited TTL")
                 .takes_value(true)
                 .value_name("EXPIRATION_TTL"))
            .arg(Arg::with_name("expireonresolve")
                 .short("o")
                 .long("expire-on-resolve")
                 .help("On resolution of alert, clear silence")
                 .takes_value(false))
            .arg(Arg::with_name("configfile")
                 .short("f")
                 .long("config-file")
                 .help("Path to INI config file")
                 .value_name("FILE_PATH")
                 .takes_value(true))
            .get_matches())
    }

    pub fn getconf(&self) -> ShushConfig {
        ShushConfig::new(self.get_match("configfile"))
    }

    pub fn get_matches(&mut self) {
    }

    pub fn getopts(&self) -> ShushOpts {
        let matches = &self.0;
        let shush_opts = if matches.is_present("nodes") {
            if matches.is_present("remove") {
                ShushOpts::Clear(ClearOpts {
                    resources: matches.value_of("nodes").map(|st| ShushResources {
                        resources: st.split(",").map(|s| s.to_string()).collect(),
                        res_type: ShushResourceType::Node,
                    }),
                    checks: matches.value_of("checks").map(|st| st.split(",")
                                                           .map(|s| s.to_string()).collect()),
                })
            } else if matches.is_present("list") {
                ShushOpts::List(ListOpts {
                    sub: matches.value_of("nodes").map(|st| st.to_string()),
                    chk: matches.value_of("checks").map(|st| st.to_string()),
                })
            } else {
                ShushOpts::Silence(SilenceOpts {
                    resources: matches.value_of("nodes").map(|st| ShushResources {
                        resources: st.split(",").map(|s| s.to_string()).collect(),
                        res_type: ShushResourceType::Node,
                    }),
                    checks: matches.value_of("checks").map(|st| st.split(",")
                                                           .map(|s| s.to_string()).collect()),
                    expire: get_expiration(matches.value_of("expire").map(|s| s.to_string())
                                           .unwrap_or("2h".to_string()),
                                           matches.is_present("expireonresolve")),
                })
            }
        } else if matches.is_present("ids") {
            if matches.is_present("remove") {
                ShushOpts::Clear(ClearOpts {
                    resources: matches.value_of("ids").map(|st| ShushResources {
                        resources: st.split(",").map(|s| s.to_string()).collect(),
                        res_type: ShushResourceType::Client,
                    }),
                    checks: matches.value_of("checks").map(|st| st.split(",")
                                                           .map(|s| s.to_string()).collect()),
                })
            } else if matches.is_present("list") {
                ShushOpts::List(ListOpts {
                    sub: matches.value_of("ids").map(|st| st.to_string()),
                    chk: matches.value_of("checks").map(|st| st.to_string()),
                })
            } else {
                ShushOpts::Silence(SilenceOpts {
                    resources: matches.value_of("nodes").map(|st| ShushResources {
                        resources: st.split(",").map(|s| s.to_string()).collect(),
                        res_type: ShushResourceType::Client,
                    }),
                    checks: matches.value_of("checks").map(|st| st.split(",")
                                                           .map(|s| s.to_string()).collect()),
                    expire: get_expiration(matches.value_of("expire").map(|s| s.to_string())
                                           .unwrap_or("2h".to_string()),
                                           matches.is_present("expireonresolve")),
                })
            }
        } else if matches.is_present("subscriptions") {
            if matches.is_present("remove") {
                ShushOpts::Clear(ClearOpts {
                    resources: matches.value_of("nodes").map(|st| ShushResources {
                        resources: st.split(",").map(|s| s.to_string()).collect(),
                        res_type: ShushResourceType::Sub,
                    }),
                    checks: matches.value_of("checks").map(|st| st.split(",")
                                                           .map(|s| s.to_string()).collect()),
                })
            } else if matches.is_present("list") {
                ShushOpts::List(ListOpts {
                    sub: matches.value_of("nodes").map(|st| st.to_string()),
                    chk: matches.value_of("checks").map(|st| st.to_string()),
                })
            } else {
                ShushOpts::Silence(SilenceOpts {
                    resources: matches.value_of("nodes").map(|st| ShushResources {
                        resources: st.split(",").map(|s| s.to_string()).collect(),
                        res_type: ShushResourceType::Sub,
                    }),
                    checks: matches.value_of("checks").map(|st| st.split(",")
                                                           .map(|s| s.to_string()).collect()),
                    expire: get_expiration(matches.value_of("expire").map(|s| s.to_string())
                                           .unwrap_or("2h".to_string()),
                                           matches.is_present("expireonresolve")),
                })
            }
        } else {
            if matches.is_present("remove") {
                ShushOpts::Clear(ClearOpts {
                    resources: None,
                    checks: matches.value_of("checks").map(|st| st.split(",")
                                                           .map(|s| s.to_string()).collect()),
                })
            } else if matches.is_present("list") {
                ShushOpts::List(ListOpts {
                    sub: None,
                    chk: matches.value_of("checks").map(|st| st.to_string()),
                })
            } else {
                ShushOpts::Silence(SilenceOpts {
                    resources: None,
                    checks: matches.value_of("checks").map(|st| st.split(",")
                                                           .map(|s| s.to_string()).collect()),
                    expire: get_expiration(matches.value_of("expire").map(|s| s.to_string())
                                           .unwrap_or("2h".to_string()),
                                           matches.is_present("expireonresolve")),
                })
            }
        };
        shush_opts
    }

    pub fn get_match(&self, option: &str) -> Option<String> {
        self.0.value_of(option).map(|s| s.to_string())
    }
}

#[cfg(test)]
mod test {
}
