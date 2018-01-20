//! Error type and conversions for internal shush error handling

use std::fmt;
use std::io;
use std::fmt::{Formatter,Display};
use std::error::Error;

use native_tls;
use serde_json;
use hyper;
use teatime::ClientError;

macro_rules! from_error {
    ( $error_name:ident, $( $error_from:path ),* ) => {
        $(
            impl From<$error_from> for $error_name {
                fn from(e: $error_from) -> Self {
                    $error_name { msg: e.description().to_string() }
                }
            }
        )*
    }
}

/// Error type for passing error messages to display from the CLI
#[derive(Debug)]
pub struct SensuError {
    msg: String,
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

from_error!(SensuError,
            hyper::Error,
            io::Error,
            serde_json::Error,
            native_tls::Error,
            hyper::error::UriError,
            ClientError);
