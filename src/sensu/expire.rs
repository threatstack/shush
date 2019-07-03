use std::fmt::{self,Display};

/// Enum for all types of duration of silences - only used in silences
#[derive(Debug,PartialEq,Clone)]
pub enum Expire {
    /// No time expiration with optional expire on resolve
    NoExpiration(bool),
    /// Expires in `usize` seconds with optional expire on resolve
    Expire(usize, bool),
}

impl Display for Expire {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Expire::NoExpiration(true) => write!(f, "not expire until resolution"),
            Expire::NoExpiration(false) => write!(f, "never expire"),
            Expire::Expire(sz, true) => write!(f, "expire in {} seconds or on resolution", sz),
            Expire::Expire(sz, false) => write!(f, "expire in {} seconds", sz),
        }
    }
}
