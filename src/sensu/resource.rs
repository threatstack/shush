use std::borrow::Borrow;
use std::fmt::{self,Display};
use std::hash::{Hash,Hasher};

/// Sensu resource for conversion to payload
#[derive(Clone,Debug,Eq,PartialEq)]
pub enum SensuResource {
    Client(String),
    Subscription(String),
}

impl Display for SensuResource {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            SensuResource::Client(ref s) => write!(f, "client:{}", s),
            SensuResource::Subscription(ref s) => write!(f, "{}", s),
        }
    }
}

impl Borrow<String> for SensuResource {
    fn borrow(&self) -> &String {
        match *self {
            SensuResource::Client(ref s) => s,
            SensuResource::Subscription(ref s) => s,
        }
    }
}

impl Hash for SensuResource {
    fn hash<H>(&self, hasher: &mut H) where H: Hasher {
        match *self {
            SensuResource::Client(ref s) => s.hash(hasher),
            SensuResource::Subscription(ref s) => s.hash(hasher),
        }
    }
}
