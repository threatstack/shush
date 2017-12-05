//! Error type and conversions for internal shush error handling

use std::fmt;
use std::io;
use std::fmt::{Formatter,Display};
use std::error::Error;

use native_tls;
use serde_json;
use hyper;

macro_rules! sensu_from_error {
    ( $error_name:path ) => {
        impl From<$error_name> for SensuError {
            fn from(e: $error_name) -> Self {
                SensuError{ msg: e.description().to_string() }
            }
        }
    }
}

/// Error type for passing error messages to display from the CLI
#[derive(Debug)]
pub struct SensuError {
    msg: String,
}

impl SensuError {
    pub fn new(msg: &str) -> Self {
        SensuError{ msg: msg.to_string() }
    }
}

impl Error for SensuError {
    fn description(&self) -> &str {
        self.msg.as_str()
    }
}

impl Display for SensuError {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        write!(f, "{}", self.msg)
    }
}

sensu_from_error!(hyper::Error);
sensu_from_error!(io::Error);
sensu_from_error!(serde_json::Error);
sensu_from_error!(native_tls::Error);
sensu_from_error!(hyper::error::UriError);
