//! Error type and conversions for internal shush error handling

use std::fmt;
use std::fmt::{Formatter,Display};
use std::error::Error;

/// Error type for passing error messages to display from the CLI
#[derive(Debug)]
pub struct SensuError {
    msg: String,
}

impl SensuError {
    pub fn new(msg: &'static str) -> Self {
        SensuError { msg: msg.to_string() }
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
