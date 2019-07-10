use std::collections::HashMap;
#[cfg(not(test))]
use std::env;
use std::env::VarError;
use std::path::Path;
#[cfg(not(test))]
use std::process;

use ini::Ini;
use nom::IResult;
use nom::bytes::streaming::{tag,take_until};
use nom::combinator::rest;
use nom::error::ErrorKind;

#[cfg(test)]
mod process {
    pub fn exit(exit_code: u32) -> ! {
        panic!(format!("Panicked with exit code {}", exit_code))
    }
}

#[cfg(test)]
mod env {
    use super::VarError;

    pub fn var(env_var: &str) -> Result<String, VarError> {
        match env_var {
            "USER" => Ok("jbaublitz".to_string()),
            "EXAMPLE_HOST" => Ok("something.host.net".to_string()),
            "ARG" => Ok("path".to_string()),
            _ => Err(VarError::NotPresent),
        }
    }
}

fn get_env_var(i: &str) -> IResult<&str, String> {
    let (i, o) = take_until("${")(i)?;
    let (i_backtrack, _) = tag("${")(i)?;
    let (i, var_name) = take_until("}")(i_backtrack)?;
    let (i, _) = tag("}")(i)?;
    let resolved_env_var = match env::var(var_name) {
        Ok(e) => e,
        Err(VarError::NotPresent) => {
            println!("Variable {} is not present", var_name);
            return Err(nom::Err::Failure((i_backtrack, ErrorKind::ParseTo)));
        },
        Err(VarError::NotUnicode(_)) => {
            println!("Variable {} is not unicode", var_name);
            return Err(nom::Err::Failure((i_backtrack, ErrorKind::ParseTo)));
        }
    };
    Ok((i, o.to_string() + &resolved_env_var))
}

fn sub_or_rest(input: &str) -> IResult<&str, String> {
    let (i, o) = if input.contains("${") {
        get_env_var(input)?
    } else {
        let (sub_i, sub_o) = rest(input)?;
        (sub_i, sub_o.to_string())
    };
    Ok((i, o.to_string()))
}

fn substitute_vars(input: &str) -> IResult<&str, String> {
    let (mut i, mut o) = sub_or_rest(input)?;
    while i.len() > 0 {
        let (loop_i, loop_o) = sub_or_rest(i)?;
        i = loop_i;
        o += &loop_o;
    }
    Ok((i, o))
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
        self.0.get(key).and_then(|val| {
            if let Ok((_, out)) = substitute_vars(val.as_str()) {
                Some(out)
            } else {
                println!("Failed to parse config file - exiting...");
                process::exit(1);
            }
        })
    }
}

#[cfg(test)]
mod test {
    use super::substitute_vars;

    #[test]
    fn test_substitude_vars() {
        let (_, out) = substitute_vars("https://${EXAMPLE_HOST}").unwrap();
        assert_eq!(out, "https://something.host.net");
        let (_, out) = substitute_vars("https://${EXAMPLE_HOST}/${ARG}").unwrap();
        assert_eq!(out, "https://something.host.net/path");
        let (_, out) = substitute_vars("https://localhost/${ARG}").unwrap();
        assert_eq!(out, "https://localhost/path");
        let (_, out) = substitute_vars("https://localhost/${ARG}").unwrap();
        assert_eq!(out, "https://localhost/path");
    }

    #[test]
    #[should_panic]
    fn test_substitude_vars_not_present_failure() {
        substitute_vars("https://localhost/${NOT_PRESENT}").unwrap();
    }

    #[test]
    #[should_panic]
    fn test_substitude_vars_bad_syntax_failure() {
        substitute_vars("https://localhost/${ARG").unwrap();
    }
}
