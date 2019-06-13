use std::fmt::{self,Display};
use std::vec;

use sensu::SensuResource;

/// Enum representing Shush target resource type (AWS node, Sensu client, or subscription)
#[derive(PartialEq,Debug)]
pub enum ShushResourceType {
    /// AWS node
    Node,
    /// Sensu client ID
    Client,
    /// Sensu subscription
    Sub,
    /// No type provided
    Wildcard,
}

/// List of resources and the resource type
#[derive(PartialEq,Debug)]
pub struct ShushResources {
    pub res_type: ShushResourceType,
    pub resources: Vec<String>,
}

impl ShushResources {
    pub fn is_node(&self) -> bool {
        match self.res_type {
            ShushResourceType::Node => true,
            ShushResourceType::Client => false,
            ShushResourceType::Sub => false,
            ShushResourceType::Wildcard => false,
        }
    }

    pub fn is_client(&self) -> bool {
        match self.res_type {
            ShushResourceType::Node => false,
            ShushResourceType::Client => true,
            ShushResourceType::Sub => false,
            ShushResourceType::Wildcard => false,
        }
    }

    pub fn is_subscription(&self) -> bool {
        match self.res_type {
            ShushResourceType::Node => false,
            ShushResourceType::Client => false,
            ShushResourceType::Sub => true,
            ShushResourceType::Wildcard => false,
        }
    }
}

impl Display for ShushResources {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.res_type {
            ShushResourceType::Node => write!(f, "Instance IDs: ")?,
            ShushResourceType::Client => write!(f, "Sensu clients: ")?,
            ShushResourceType::Sub => write!(f, "Subscriptions: ")?,
            ShushResourceType::Wildcard => (),
        };
        write!(f, "{}", if self.resources.len() > 0 {
            self.resources.join(", ")
        } else {
            return Err(fmt::Error);
        })
    }
}
