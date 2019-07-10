//! Error type and conversions for internal shush error handling

use std::fmt;
use std::fmt::{Formatter,Display};
use std::error::Error;

use hyper;

/// Error type for passing error messages to display from the CLI
#[derive(Debug)]
pub enum SensuError {
    NotFound,
    Message(String),
}

impl SensuError {
    pub fn new(msg: &str) -> Self {
        SensuError::Message(msg.to_string())
    }

    pub fn new_string<F>(any_format: F) -> Self where F: Display {
        SensuError::Message(format!("{}", any_format))
    }

    pub fn not_found() -> Self {
        SensuError::NotFound
    }
}

impl From<hyper::Error> for SensuError {
    fn from(e: hyper::Error) -> Self {
        SensuError::new(e.description())
    }
}

impl Error for SensuError {
    fn description(&self) -> &str {
        if let SensuError::Message(string) = self {
            string.as_ref()
        } else {
            "404 Not found"
        }
    }
}

impl Display for SensuError {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        write!(f, "{}", self.description())
    }
}
