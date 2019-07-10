use std::fmt::{self,Display};
use std::vec;

/// Enum representing Shush target resource type (AWS node, Sensu client, or subscription)
#[derive(PartialEq,Debug)]
pub enum ShushResourceType {
    /// AWS node
    Node,
    /// Sensu client ID
    Client,
    /// Sensu subscription
    Sub,
}

/// List of resources and the resource type
#[derive(PartialEq,Debug)]
pub struct ShushResources {
    pub res_type: ShushResourceType,
    pub resources: Vec<String>,
}

impl IntoIterator for ShushResources {
    type Item = String;
    type IntoIter = ShushResourceIterator;

    fn into_iter(self) -> Self::IntoIter {
        ShushResourceIterator(self.resources.into_iter())
    }
}

impl Display for ShushResources {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.res_type {
            ShushResourceType::Node => write!(f, "Instance IDs: ")?,
            ShushResourceType::Client => write!(f, "Sensu clients: ")?,
            ShushResourceType::Sub => write!(f, "Subscriptions: ")?,
        };
        write!(f, "{}", if self.resources.len() > 0 {
            self.resources.join(", ")
        } else {
            return Err(fmt::Error);
        })
    }
}

pub struct ShushResourceIterator(vec::IntoIter<String>);

impl Iterator for ShushResourceIterator {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}
