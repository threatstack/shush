use std::collections::HashMap;
use std::env;
use std::path::Path;
#[cfg(not(test))]
use std::process;

use ini::Ini;
use nom::rest_s;

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
