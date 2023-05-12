use std::{
    fmt::{self, Debug},
    future::{Future, IntoFuture},
    pin::Pin,
    time::Duration,
};

use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

use crate::{
    actor::{Ask, AsyncAsk, InternalHandler},
    envelope::{AsyncResponse, ConfidentialEnvelope, Envelope, Response, SendMessage},
    message::IntoFutureShutdown,
    Actor, Handler, Message,
};

/// Errors when sending a message failed
#[derive(Debug)]
pub enum SendError<M: Message> {
    // The value failed to be sent to the reciving actor because the recieving
    // actor is offline or has closed it's mailbox intentionally.
    Closed(M),
    // The value failed to be sent to the reciving actor because the reciving
    // actors mailbox is full
    Full(M),
}

impl<M: Message> SendError<M> {
    pub fn into_inner(self) -> M {
        match self {
            SendError::Closed(msg) => msg,
            SendError::Full(msg) => msg,
        }
    }
}

impl<M: Message> fmt::Display for SendError<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SendError::Closed(_) => write!(
                f,
                "Message failed to send to actor because mailbox is closed"
            )?,
            SendError::Full(_) => write!(f, "Message to actor failed because mailbox was/is full")?,
        };
        Ok(())
    }
}

/// Errors when attempting to ask an actor about a question failed.
#[derive(Debug)]
pub enum AskError<M: Message> {
    // The value that we tried to send to the actor failed to make it because the
    // actors mailbox is closed. We can return the original message.
    Closed(M),
    // The message was successfully sent to the actor however the sender was dropped
    // for an unknown reason (maybe the actor paniced). This is no way to recover
    // the message sent to the actor.
    Dropped,
}

impl<M: Message> fmt::Display for AskError<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AskError::Closed(_) => write!(f, "Message failed to send to actor because mailbox is closed")?,
            AskError::Dropped => write!(f, "Message successfully sent to actor however it failed while we were waiting for a response")?,
        };
        Ok(())
    }
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

pub struct ScheduledActorRef<A: Actor> {
    inner: ActorRef<A>,
    duration: Duration,
}

/// Hold a local address to a running actor that can use to send messages to the
/// running actor. The message will be processed at some point in the future.
pub struct ActorRef<A: Actor> {
    address: mpsc::Sender<Box<dyn SendMessage<A>>>,
}

impl<A: Actor> std::fmt::Display for ActorRef<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ActorRef<{}>", A::name())
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

    /// Send a message to an actor and wait for the actors mailbox to be made
    /// avalaible. This call can fail if the recieving actor is destoried.
    pub(crate) async fn internal_send_async<M>(&self, message: M) -> Result<(), SendError<M>>
    where
        M: Message,
        A: Actor + InternalHandler<M>,
    {
        if let Err(mut err) = self
            .address
            .send(Box::new(ConfidentialEnvelope::new(message)))
            .await
        {
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
impl<A> ActorRef<A>
where
    A: Actor,
{
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

    /// Attempt to instantly send a message to an actor. Sending the message may
    /// fail during delivery of the message or getting a response. If the message
    /// fails at any point, this method will return None. Otherwise, on successful
    /// operation, it returns the responding value.
    pub async fn try_ask<M>(&self, message: M) -> Option<A::Result>
    where
        M: Message,
        A: Actor + Ask<M>,
    {
        let (tx, rx) = oneshot::channel();
        let envelope = Response::new(Envelope::new(message), tx);
        if self.address.try_send(Box::new(envelope)).is_err() {
            return None;
        }
        rx.await.ok()
    }

    /// Attempt to asyncrously send a message to an actor, waiting if the reciving
    /// actors mailbox is full. Wait for a response back from the actor containing
    /// the value asked for.
    ///
    /// The message could fail to send, in which case we can return the message
    /// back to the caller. Otherwise, if the other actor drops after the message
    /// has been recieved, we return an error without the original message as it
    /// has been lost.
    pub async fn ask<M>(&self, message: M) -> Result<A::Result, AskError<M>>
    where
        M: Message,
        A: Actor + Ask<M>,
    {
        let (tx, rx) = oneshot::channel();
        let envelope = Response::new(Envelope::new(message), tx);
        if let Err(mut err) = self.address.send(Box::new(envelope)).await {
            return Err(AskError::Closed(
                err.0
                    .as_any()
                    .downcast_mut::<Envelope<M>>()
                    .unwrap()
                    .unwrap(),
            ));
        }
        rx.await.map_err(|_| AskError::Dropped)
    }

    /// Attempt to instantly send a message to an actor. Sending the message may
    /// fail during delivery of the message or getting a response. If the message
    /// fails at any point, this method will return None. Otherwise, on successful
    /// operation, it returns the responding value.
    pub async fn try_async_ask<M>(&self, message: M) -> Option<A::Result>
    where
        M: Message,
        A: Actor + AsyncAsk<M>,
    {
        let (tx, rx) = oneshot::channel();
        let envelope = AsyncResponse::new(Envelope::new(message), tx);
        if self.address.try_send(Box::new(envelope)).is_err() {
            return None;
        }
        rx.await.ok()
    }

    /// Attempt to asyncrously send a message to an actor, waiting if the reciving
    /// actors mailbox is full. Wait for a response back from the actor containing
    /// the value asked for.
    ///
    /// The message could fail to send, in which case we can return the message
    /// back to the caller. Otherwise, if the other actor drops after the message
    /// has been recieved, we return an error without the original message as it
    /// has been lost.
    pub async fn async_ask<M>(&self, message: M) -> Result<A::Result, AskError<M>>
    where
        M: Message,
        A: Actor + AsyncAsk<M>,
    {
        let (tx, rx) = oneshot::channel();
        let envelope = AsyncResponse::new(Envelope::new(message), tx);
        if let Err(mut err) = self.address.send(Box::new(envelope)).await {
            return Err(AskError::Closed(
                err.0
                    .as_any()
                    .downcast_mut::<Envelope<M>>()
                    .unwrap()
                    .unwrap(),
            ));
        }
        rx.await.map_err(|_| AskError::Dropped)
    }

    pub fn schedule(&self, duration: Duration) -> ScheduledActorRef<A> {
        ScheduledActorRef {
            inner: self.clone(),
            duration,
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
                if (sender
                    .internal_send_async(IntoFutureShutdown::new(tx))
                    .await)
                    .is_err()
                {
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
