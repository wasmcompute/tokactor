use std::net::SocketAddr;

#[derive(Debug)]
pub struct TcpRequest(pub SocketAddr);

pub mod read {
    use std::{future::Future, pin::Pin};

    use tokio::{io::AsyncReadExt, net::TcpStream};

    use crate::Message;

    pub struct Read<const LEN: usize> {
        pub error: Option<std::io::Error>,
        pub buffer: [u8; LEN],
        pub read: usize,
    }
    impl<const LEN: usize> Default for Read<LEN> {
        fn default() -> Self {
            Self {
                error: Default::default(),
                buffer: [0_u8; LEN],
                read: Default::default(),
            }
        }
    }

    impl<const LEN: usize> Read<LEN> {
        pub fn to_vec(&self) -> Vec<u8> {
            self.buffer[..self.read].to_vec()
        }
    }

    #[derive(Default)]
    pub struct ReadAll {
        pub error: Option<std::io::Error>,
        pub bytes: Vec<u8>,
        pub read: usize,
    }

    pub trait TcpReadable: Message + Default {
        fn read<'a>(
            &'a mut self,
            stream: &'a mut TcpStream,
        ) -> Pin<Box<dyn Future<Output = Result<(), std::io::Error>> + Send + Sync + 'a>>;

        fn is_closed(&self) -> bool;
    }

    impl<const LEN: usize> TcpReadable for Read<LEN> {
        fn read<'a>(
            &'a mut self,
            stream: &'a mut TcpStream,
        ) -> Pin<Box<dyn Future<Output = Result<(), std::io::Error>> + Send + Sync + 'a>> {
            Box::pin(async move {
                self.read = stream.read(&mut self.buffer).await?;
                Ok(())
            })
        }

        fn is_closed(&self) -> bool {
            self.error.is_none() && self.read == 0
        }
    }

    impl TcpReadable for ReadAll {
        fn read<'a>(
            &'a mut self,
            stream: &'a mut TcpStream,
        ) -> Pin<Box<dyn Future<Output = Result<(), std::io::Error>> + Send + Sync + 'a>> {
            Box::pin(async move {
                self.read = stream.read_to_end(&mut self.bytes).await?;
                Ok(())
            })
        }

        fn is_closed(&self) -> bool {
            false
        }
    }
}

#[derive(Debug)]
pub struct Stdin {}
