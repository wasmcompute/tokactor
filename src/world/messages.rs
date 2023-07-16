use std::net::SocketAddr;

use crate::io::Writer;

#[derive(Debug)]
pub struct TcpRequest(pub Writer, pub SocketAddr);
