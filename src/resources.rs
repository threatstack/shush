use std::fmt::{self,Display};
use std::vec;

use sensu::SensuResource;

/// Enum representing Shush target resource (AWS node, Sensu client, or subscription)
#[derive(PartialEq,Debug)]
pub enum ShushResources {
    /// AWS node
    Node(Vec<String>),
    /// Sensu client ID
    Client(Vec<String>),
    /// Sensu subscription
    Sub(Vec<String>),
}

impl ShushResources {
    pub fn is_client(&self) -> bool {
        match *self {
            ShushResources::Node(_) => false,
            ShushResources::Client(_) => true,
            ShushResources::Sub(_) => false,
        }
    }

    pub fn retain<F>(&mut self, f: F) where F: FnMut(&String) -> bool {
        match *self {
            ShushResources::Node(_) => unimplemented!(),
            ShushResources::Client(ref mut v) => v.retain(f),
            ShushResources::Sub(ref mut v) => v.retain(f),
        };
    }
}

impl Display for ShushResources {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ShushResources::Node(ref v) => write!(f, "Instance IDs: {}", if v.len() > 0 {
                v.join(", ")
            } else {
                return Err(fmt::Error);
            }),
            ShushResources::Client(ref v) => write!(f, "Sensu clients: {}", if v.len() > 0 {
                v.join(", ")
            } else {
                return Err(fmt::Error);
            }),
            ShushResources::Sub(ref v) => write!(f, "Subscriptions: {}", if v.len() > 0 {
                v.join(", ")
            } else {
                return Err(fmt::Error);
            }),
        }
    }
}

impl IntoIterator for ShushResources {
    type Item = SensuResource;
    type IntoIter = vec::IntoIter<SensuResource>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            ShushResources::Client(vec) => vec.into_iter()
                .map(SensuResource::Client)
                .collect::<Vec<SensuResource>>().into_iter(),
            ShushResources::Sub(vec) => vec.into_iter()
                .map(SensuResource::Sub)
                .collect::<Vec<SensuResource>>().into_iter(),
            ShushResources::Node(_) => {
                unimplemented!()
            },
        }
    }
}
