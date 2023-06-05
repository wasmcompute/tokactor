#![allow(clippy::type_complexity)]

use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use crate::{AskError, Message, SendError};

#[derive(Clone, Debug)]
pub struct ActorSendRef<M: Message> {
    inner: mpsc::Sender<M>,
}

impl<M: Message> ActorSendRef<M> {
    pub(crate) fn new(inner: mpsc::Sender<M>) -> Self {
        Self { inner }
    }

    pub fn try_send(&self, message: M) -> Result<(), SendError<M>> {
        if let Err(err) = self.inner.try_send(message) {
            match err {
                mpsc::error::TrySendError::Full(err) => Err(SendError::Full(err)),
                mpsc::error::TrySendError::Closed(err) => Err(SendError::Closed(err)),
            }
        } else {
            Ok(())
        }
    }

    pub async fn send(&self, message: M) -> Result<(), SendError<M>> {
        if let Err(err) = self.inner.send(message).await {
            Err(SendError::Closed(err.0))
        } else {
            Ok(())
        }
    }

    pub async fn send_timeout(&self, message: M, timeout: Duration) -> Result<(), SendError<M>> {
        if let Err(err) = self.inner.send_timeout(message, timeout).await {
            match err {
                mpsc::error::SendTimeoutError::Timeout(err) => Err(SendError::Full(err)),
                mpsc::error::SendTimeoutError::Closed(err) => Err(SendError::Closed(err)),
            }
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug)]
pub struct ActorAskRef<M: Message, Out: Message> {
    inner: mpsc::Sender<(M, oneshot::Sender<Result<Out, AskError<M>>>)>,
}

impl<M: Message, Out: Message> ActorAskRef<M, Out> {
    pub(crate) fn new(inner: mpsc::Sender<(M, oneshot::Sender<Result<Out, AskError<M>>>)>) -> Self {
        Self { inner }
    }

    pub async fn ask(&self, message: M) -> Result<Out, AskError<M>> {
        let (tx, rx) = oneshot::channel();
        if let Err(err) = self.inner.send((message, tx)).await {
            Err(AskError::Closed(err.0 .0))
        } else {
            match rx.await {
                Ok(response) => {
                    let response = response?;
                    Ok(response)
                }
                Err(_) => Err(AskError::Dropped),
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct ActorAsyncAskRef<M: Message, Out: Message> {
    inner: mpsc::Sender<(M, oneshot::Sender<Result<Out, AskError<M>>>)>,
}

impl<M: Message, Out: Message> ActorAsyncAskRef<M, Out> {
    pub fn new(inner: mpsc::Sender<(M, oneshot::Sender<Result<Out, AskError<M>>>)>) -> Self {
        Self { inner }
    }

    pub async fn ask_async(&self, message: M) -> Result<Out, AskError<M>> {
        let (tx, rx) = oneshot::channel();
        if let Err(err) = self.inner.send((message, tx)).await {
            Err(AskError::Closed(err.0 .0))
        } else {
            match rx.await {
                Ok(response) => {
                    let response = response?;
                    Ok(response)
                }
                Err(_) => Err(AskError::Dropped),
            }
        }
    }
}
