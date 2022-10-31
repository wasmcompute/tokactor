use std::{
    fmt::Debug,
    future::{Future, IntoFuture},
    pin::Pin,
};

use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

use crate::{
    envelope::{Envelope, SendMessage},
    message::IntoFutureShutdown,
    Actor, Handler, Message,
};

/// Errors when sending a message failed
#[derive(Debug)]
pub enum SendError<M: Message> {
    Closed(M),
    Full(M),
}

/// Address for one off actors that support asyncrous tasks. Instead of holding a
/// MPSC channel, we hold the actual handle to the tokio task join handle.
pub struct AnonymousRef {
    handle: JoinHandle<()>,
}

impl AnonymousRef {
    pub(crate) fn new(handle: JoinHandle<()>) -> Self {
        Self { handle }
    }

    /// Halt the task by aborting it's execution.
    pub fn halt(self) {
        self.handle.abort()
    }
}

/// Hold a local address to a running actor that can use to send messages to the
/// running actor. The message will be processed at some point in the future.
pub struct ActorRef<A>
where
    A: Actor,
{
    address: mpsc::Sender<Box<dyn SendMessage<A>>>,
}

impl<A: Actor> std::fmt::Display for ActorRef<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ActorRef<{}>", A::KIND)
    }
}

impl<A: Actor> Debug for ActorRef<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActorRef")
            .field("ActorRef", &self.address)
            .finish()
    }
}

impl<A: Actor> Clone for ActorRef<A> {
    fn clone(&self) -> Self {
        Self {
            address: self.address.clone(),
        }
    }
}

impl<A> ActorRef<A>
where
    A: Actor,
{
    pub(crate) fn new(address: mpsc::Sender<Box<dyn SendMessage<A>>>) -> Self {
        Self { address }
    }

    /// Attempt to instantly send a message to an actor. If it fails to send because
    /// the address is closed or full, then silently throw away the message.
    ///
    /// This is mainly used to forward or send requests deeper into the system.
    /// Messages will be siliently dropped if they fail to be pushed to the
    /// recieving queue.
    pub fn try_send<M>(&self, message: M)
    where
        M: Message,
        A: Actor + Handler<M>,
    {
        let _ = self.address.try_send(Box::new(Envelope::new(message)));
    }

    /// Attempt to instantly send a message to an actor. Failure to send the message
    /// responseds the message back to the caller.
    pub fn send<M>(&self, message: M) -> Result<(), SendError<M>>
    where
        M: Message,
        A: Actor + Handler<M>,
    {
        if let Err(err) = self.address.try_send(Box::new(Envelope::new(message))) {
            match err {
                mpsc::error::TrySendError::Full(mut err) => Err(SendError::Full(
                    err.as_any().downcast_mut::<Envelope<M>>().unwrap().unwrap(),
                )),
                mpsc::error::TrySendError::Closed(mut err) => Err(SendError::Closed(
                    err.as_any().downcast_mut::<Envelope<M>>().unwrap().unwrap(),
                )),
            }
        } else {
            Ok(())
        }
    }

    /// Send a message to an actor and wait for the actors mailbox to be made
    /// avalaible. This call can fail if the recieving actor is destoried.
    pub async fn send_async<M>(&self, message: M) -> Result<(), SendError<M>>
    where
        M: Message,
        A: Actor + Handler<M>,
    {
        if let Err(mut err) = self.address.send(Box::new(Envelope::new(message))).await {
            Err(SendError::Closed(
                err.0
                    .as_any()
                    .downcast_mut::<Envelope<M>>()
                    .unwrap()
                    .unwrap(),
            ))
        } else {
            Ok(())
        }
    }
}

/// Errors that can occur if the supervios actor dies while awaiting for the child
/// actors to finish they panic.
#[derive(Debug, PartialEq, Eq)]
pub enum IntoFutureError {
    MailboxClosed,
    Paniced,
}

/// Stop an actor from executing and wait for all messages and children to die that
/// is attached to the actor. Once the actor and all of the children have cleaned up,
/// this method will return with the completed actor.
impl<A: Actor> IntoFuture for ActorRef<A> {
    type Output = Result<A, IntoFutureError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + Sync>>;

    fn into_future(self) -> Self::IntoFuture {
        if self.address.is_closed() {
            Box::pin(async { Err(IntoFutureError::MailboxClosed) })
        } else {
            let (tx, rx) = oneshot::channel();
            let sender = self;
            Box::pin(async move {
                if (sender.send_async(IntoFutureShutdown::new(tx)).await).is_err() {
                    Err(IntoFutureError::MailboxClosed)
                } else {
                    match rx.await {
                        Ok(actor) => Ok(actor),
                        Err(_) => Err(IntoFutureError::Paniced),
                    }
                }
            })
        }
    }
}
