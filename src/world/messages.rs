use std::net::SocketAddr;

use tokio::net::TcpStream;

use crate::Message;

#[derive(Debug)]
pub struct TcpRequest(pub TcpStream, pub SocketAddr);
impl Message for TcpRequest {}

#[derive(Debug)]
pub struct Stdin {}
impl Message for Stdin {}
