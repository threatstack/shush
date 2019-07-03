use std::convert::TryInto;

use hyper::Uri;

/// Enum representing the endpoints in Sensu as a type that shush accesses
#[derive(Clone)]
pub enum SensuEndpoint<'a> {
    /// Endpoint for listing silences
    Silenced,
    /// Endpoint for clearing silences
    Clear,
    /// Endpoint for getting clients
    Clients,
    /// Endpoint for getting a single client
    Client(&'a str),
    /// Endpoint for getting check results
    Results,
}

impl<'a> TryInto<Uri> for SensuEndpoint<'a> {
    type Error = String;

    fn try_into(self) -> Result<Uri, Self::Error> {
        match self {
            SensuEndpoint::Silenced => "/silenced".parse::<Uri>().map_err(|e| format!("{}", e)),
            SensuEndpoint::Clear => "/silenced/clear".parse::<Uri>().map_err(|e| format!("{}", e)),
            SensuEndpoint::Client(c) => format!("/clients/{}", c).parse::<Uri>()
                .map_err(|e| format!("{}", e)),
            SensuEndpoint::Clients => "/clients".parse::<Uri>().map_err(|e| format!("{}", e)),
            SensuEndpoint::Results => "/results".parse::<Uri>().map_err(|e| format!("{}", e)),
        }
    }
}
