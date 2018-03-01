//! ## Shush
//! Silence is golden.
//!
//! ### Purpose
//! Sensu is an alerting solution that provides per host metric checks. The Sensu primitives are checks,
//! subscriptions, and clients. A check corresponds to a comparison of a metric against the specified
//! theshold. A client is a single host identified by an ID. A subscription is one or more nodes
//! grouped under the same subscription name. Sensu can be used for
//! alerting on critical and warning thresholds and enables integration for any Ruby-compatible
//! API with many existing plugins that have already been contributed.
//! As a result this can be a critical part of monitoring infrastucture. However, these thresholds
//! are static, and in maintenance cases, there can often be false positives and corner cases.
//! This tool enables the user to create, remove, or list active silences for
//! combinations of clients/subscriptions and checks in Sensu. This is useful for noise reduction,
//! scheduled maintenance, and temporary or permanent silencing when adjusting thresholds. Shush
//! is a simple way to silence on any combination of subscriptions and checks or clients and checks.
//!
//! ### External dependencies
//! A Sensu server with a version of the REST API of 0.29 or greater is a hard requirement.
//!
//! For more information on the Sensu REST API, click
//! [here](https://sensuapp.org/docs/0.29/api/silenced-api.html).
//!
//! ### Setup and background
//! Shush accesses three Sensu API endpoints. For Shush to be operational the following
//! Sensu endpoints must be reachable:
//!
//!   * `GET /clients`
//!   * `POST /silenced`
//!   * `POST /silenced/clear`
//!
//! This tool gives the user with the option to provide instance IDs (AWS-specific - click [here](#aws-specific-configuration)
//! for a setup guide) or to provide Sensu client IDs (applicable for all applications using Sensu).
//!
//! When providing either instance IDs or Sensu client IDs as opposed to subscriptions, validation
//! against the Sensu server is performed to verify that the ID is registered and active.
//! Additionally, shush performs mapping from instance ID to sensu client ID when using
//! the parameters `-n` (long form, `--aws-nodes`).
//!
//! ### Notes on usage
//! Shush has three actions: silence, clear silence, and list. The default is silence, `-l`
//! enables listing mode, and `-r` enables clearing mode.
//! 
//! ### Parameter details
//! Shush operates in silence mode when neither `-l` nor `-r` is provided.
//! It will silence any nodes associated
//! with the instance IDs passed `-n`, client IDs passed to `-i`,
//! subscriptions passed to `-s`, or checks
//! passed to `-c`. All arguments can take a single value or a comma separated list of
//! values. Only one of instance IDs, client IDs and subscriptions can be specified in one
//! invocation of Shush.
//!
//! `-l` combined with either of the flags `-c` or `-s` (`-n` and `-i` are not allowed)
//! will list the requested information matched against the argument passed to the
//! corresponding flag. This is expected to be a regex and will be compiled as such or ignored.
//!
//! `-r` added to the same parameters used in silence mode will simply
//! clear the same checks created by silence mode.
//!
//! ### Rust Version
//! Shush was developed on Rust 1.16 and is not guaranteed to work on anything
//! earlier.
//!
//! # Examples
//! ## List all active silences
//! ```
//! shush -l
//! ```
//! 
//! ## List all active silences with a subscription matching the regex `something.*`
//! ```
//! shush -l -s "something.*"
//! ```
//!
//! ## Silence all checks on clients with instance IDs `INST_ID_1` and `INST_ID_2`
//! ```
//! shush -n INST_ID_1,INST_ID2
//! ```
//!
//! ## Silence check `SOME_CHECK` for 1 hour and 30 minutes
//! ```
//! shush -c SOME_CHECK -e 1h30m
//! ```
//!
//! ## Silence check `SOME_CHECK` indefinitely
//! ```
//! shush -c SOME_CHECK -e none
//! ```
//!
//! ## Silence check `SOME_CHECK` until alert resolves
//! ```
//! shush -c SOME_CHECK -o
//! ```
//!
//! ## Silence check `SOME_CHECK` on client with instance ID `INST_ID_1`
//! ```
//! shush -n INST_ID_1 -c SOME_CHECK
//! ```
//!
//! ## Silence check `SOME_CHECK` on client with Sensu client name `CLIENT_1`
//! ```
//! shush -i CLIENT_1 -c SOME_CHECK
//! ```
//!
//! ## Silence check `SOME_CHECK` on client with Sensu subscription `SUB_1`
//! ```
//! shush -s SUB_1 -c SOME_CHECK
//! ```
//!
//! ## Clear check silence for `SOME_CHECK` on client with instance ID `INST_ID_1`
//! ```
//! shush -r -n INST_ID_1 -c SOME_CHECK
//! ```
//!
//! ## AWS-Specific Configuration
//! To configure AWS support for shush, you will need to make modifications on the Sensu side as
//! well. Sensu checks operate by sending a JSON payload back to the Sensu server with some
//! predefined and some arbitrary data. To enable shush selection by AWS instance ID, add an
//! `instance_id` key with a value equivalent to executing the following command from your AWS node:
//!
//! ```
//! curl http://169.254.169.254/1.0/meta-data/instance-id
//! ```
//!
//! This must be added to the Sensu _client_ object. See [here](https://sensuapp.org/docs/0.29/reference/clients.html)
//! for more details. Once this has been done on the server side, shush will do the rest.

#![deny(missing_docs)]

extern crate futures;
extern crate regex;
extern crate getopts;
extern crate tokio_core;
extern crate hyper;
extern crate hyper_tls;
extern crate native_tls;

// Only need `json!()` macro for testing
#[cfg(test)]
#[macro_use]
extern crate serde_json;
#[cfg(not(test))]
extern crate serde_json;

#[macro_use]
extern crate itertools;
extern crate ini;
#[macro_use]
extern crate nom;
extern crate teatime;

mod opts;
mod json;
mod sensu;
mod err;

use std::{env,process};

use hyper::Method;
use serde_json::{Value,Map};
use teatime::{ApiClient,JsonApiClient};
use teatime::sensu::SensuClient;

use opts::ShushOpts;
use sensu::SensuEndpoint;
use json::JsonRef;

fn filter_vec(vec: &Vec<serde_json::Value>, sub: Option<String>, chk: Option<String>) -> Option<String> {
    let mut acc_string: String;
    let filter_closure = |item, re: Result<&regex::Regex, &regex::Error>, key| -> bool {
        match re {
            Ok(r) => {
                let item_ref = JsonRef(item);
                let sub_json = item_ref.get_fold_as_str_def(key, "");
                r.is_match(sub_json)
            },
            _ => {
                println!("Invalid {} regex - defaulting to .*", key);
                true
            }
        }
    };
    let re_sub = regex::RegexBuilder::new(sub.unwrap_or(".*".to_string()).as_str())
        .size_limit(8192).dfa_size_limit(8192).build();
    let re_chk = regex::RegexBuilder::new(chk.unwrap_or(".*".to_string()).as_str())
        .size_limit(8192).dfa_size_limit(8192).build();
    acc_string = "Active silences:\n".to_string();
    for filtered_item in vec.iter().filter(|item| {
        filter_closure(item, re_sub.as_ref(), "subscription")
    })
    .filter(|item| {
        filter_closure(item, re_chk.as_ref(), "check")
    }) {
        let filtered_item_ref = JsonRef(filtered_item);
        let sub_val = filtered_item_ref.get_fold_as_str_def("subscription", "all");
        let chk_val = filtered_item_ref.get_fold_as_str_def("check", "all");
        let seconds = filtered_item_ref.get_fold_as_i64_def("expire", -1);
        let on_resolve = filtered_item_ref.get_fold_as_bool_def("expire_on_resolve", false);
        let expiration = if seconds == -1 {
            "never".to_string()
        } else if on_resolve == true {
            "on resolve".to_string()
        } else {
            format!("in {} seconds", seconds)
        };

        acc_string = format!("{}\tSubscription: {}\n\tCheck: {}\n\tExpires {}\n\n", acc_string, sub_val, chk_val, expiration);
    }
    Some(acc_string)
}

fn list_formatting(sensu_client: &mut SensuClient, sub: Option<String>, chk: Option<String>) -> Option<String> {
    let uri = SensuEndpoint::Silenced.into();
    match sensu_client.request_json::<Value>(Method::Get, uri, None) {
        Err(e) => {
            println!("Couldn't gather active silences from API: {}", e);
            None
        },
        Ok(ref val) => {
            match JsonRef(val).get_as_vec() {
                Some(vec) => {
                    filter_vec(vec, sub, chk)
                },
                _ => {
                    println!("Invalid response from API: {}", val);
                    None
                },
            }
        },
    }
}

fn request_iter(sopts: ShushOpts, sclient: &mut SensuClient, method: Method,
                endpoint: SensuEndpoint) {
    let is_node = sopts.resource_is_node();
    let mapped = if is_node {
        sopts.iid_mapper(sclient)
    } else {
        sopts
    };
    println!("{}", mapped);
    for pl in mapped {
        let map: Map<String, Value> = pl.into();
        let uri = endpoint.clone().into();
        match sclient.request(method.clone(), uri, Some(Value::from(map))) {
            Err(e) => {
                println!("Error on silence request: {}", e);
                process::exit(1);
            },
            _ => (),
        };
    }
}

/// Main function - handle arg parsing and all executable actions
pub fn main() {
    let args: Vec<String> = env::args().collect();
    let (shush_opts, shush_cfg) = opts::getopts(args);
    let mut sensu_client = match SensuClient::new(shush_cfg.get("api")
                                                  .unwrap_or(String::new())
                                                  .as_ref()) {
        Ok(c) => c,
        Err(e) => {
            println!("Error creating Sensu client: {}", e);
            process::exit(1);
        }
    };


    match shush_opts {
        ShushOpts::Silence { resource: _, checks: _, expire: _ } => {
            request_iter(shush_opts, &mut sensu_client, Method::Post,
                         SensuEndpoint::Silenced)
        }
        ShushOpts::Clear { resource: _, checks: _ } => {
            request_iter(shush_opts, &mut sensu_client, Method::Post,
                         SensuEndpoint::Clear)
        },
        ShushOpts::List { sub, chk } => {
            print!("{}", list_formatting(&mut sensu_client, sub, chk)
                     .unwrap_or("".to_string()));
        },
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_filter_vec() {
        let res = filter_vec(&vec![json!({
            "subscription": "asldAKHll",
            "check": "9374982",
            "expire": 200
        }),
        json!({
            "subscription": "10alskd",
            "check": "a2i1o4u",
            "expire": 200
        }),
        json!({
            "subscription": "******",
            "check": "asdf",
            "expire": 200
        })], Some("^[a-zA-Z]+$".to_string()), Some("^[0-9]+$".to_string()));
        assert_eq!(
            res,
            Some("Active silences:\n\tSubscription: asldAKHll\n\tCheck: 9374982\n\tExpires in 200 seconds\n\n".to_string())
        );
    }
}
