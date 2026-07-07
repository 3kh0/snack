use super::client::PreparedRequest;

#[derive(Debug, Clone)]
pub struct Connection;

pub fn connect_request() -> Result<PreparedRequest, super::Error> {
    Err(super::Error::TransportNotConfigured)
}
