use std::{future::Future, pin::Pin, task::Poll};

use tokio::{io::ReadBuf, net::tcp::OwnedReadHalf};

use crate::io::{DataFrame, IoRead, IoReadFuture};

#[derive(Debug)]
pub struct Read<const LEN: usize> {
    pub buffer: [u8; LEN],
    pub read: usize,
}

impl<const LEN: usize> Default for Read<LEN> {
    fn default() -> Self {
        Self {
            buffer: [0_u8; LEN],
            read: 0,
        }
    }
}

impl<const LEN: usize> Read<LEN> {
    pub fn as_slice(&self) -> &[u8] {
        &self.buffer[..self.read]
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.buffer[..self.read].to_vec()
    }
}

pub struct ReadFut<'a, const LEN: usize> {
    read: &'a mut usize,
    buffer: &'a mut [u8; LEN],
    stream: &'a mut OwnedReadHalf,
}

impl<'a, const LEN: usize> Unpin for ReadFut<'a, LEN> {}

impl<'a, const LEN: usize> Future for ReadFut<'a, LEN> {
    type Output = std::io::Result<Option<()>>;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let mut buffer = ReadBuf::new(this.buffer);
        if let Poll::Ready(result) = this.stream.poll_peek(cx, &mut buffer) {
            Poll::Ready(match result {
                Ok(read) => {
                    if read == 0 {
                        Ok(None)
                    } else {
                        this.stream.try_read(&mut this.buffer[..read]).unwrap();
                        *this.read = read;
                        Ok(Some(()))
                    }
                }
                Err(err) => Err(err),
            })
        } else {
            Poll::Pending
        }
    }
}

impl<const LEN: usize> DataFrame for Read<LEN> {
    fn frame(&self) -> &[u8] {
        &self.buffer[..self.read]
    }
}

impl<const LEN: usize> IoReadFuture for Read<LEN> {
    type Future<'a> = ReadFut<'a, LEN>;
}

impl<const LEN: usize> IoRead<OwnedReadHalf> for Read<LEN> {
    fn read<'a>(&'a mut self, stream: &'a mut OwnedReadHalf) -> Self::Future<'a> {
        ReadFut {
            read: &mut self.read,
            buffer: &mut self.buffer,
            stream,
        }
    }
}
