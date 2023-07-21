use std::{
    error::Error,
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
    // The value of the message was not able to be recovered for some reason.
    Lost,
    // The value failed to be sent to the reciving actor because the recieving
    // actor is offline or has closed it's mailbox intentionally.
    Closed(M),
    // The value failed to be sent to the reciving actor because the reciving
    // actors mailbox is full
    Full(M),
}

impl<M: Message + Debug> Error for SendError<M> {}

impl<M: Message> SendError<M> {
    pub fn into_inner(self) -> M {
        match self {
            SendError::Lost => unreachable!(),
            SendError::Closed(msg) => msg,
            SendError::Full(msg) => msg,
        }
    }
}

impl<M: Message> From<mpsc::error::TrySendError<M>> for SendError<M> {
    fn from(value: mpsc::error::TrySendError<M>) -> Self {
        match value {
            mpsc::error::TrySendError::Full(m) => Self::Full(m),
            mpsc::error::TrySendError::Closed(m) => Self::Closed(m),
        }
    }
}

impl<M: Message> fmt::Display for SendError<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SendError::Lost => write!(f, "Message to actor failed because mailbox was/is full, and message was lost for some reason")?,
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
pub enum AskError<M: Message> {
    /// The value that we tried to send to the actor failed to make it because the
    /// actors mailbox is closed. We can return the original message.
    Closed(M),
    /// The message was successfully sent to the actor however the sender was dropped
    /// for an unknown reason (maybe the actor paniced). This is no way to recover
    /// the message sent to the actor.
    Dropped,
}

impl<M: Message + Debug> Error for AskError<M> {}

impl<M: Message> fmt::Display for AskError<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AskError::Closed(_) => write!(f, "Message failed to send to actor because mailbox is closed")?,
            AskError::Dropped => write!(f, "Message successfully sent to actor however it failed while we were waiting for a response")?,
        };
        Ok(())
    }
}

impl<M: Message + Debug> fmt::Debug for AskError<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed(arg0) => f.debug_tuple("Closed").field(arg0).finish(),
            Self::Dropped => write!(f, "Dropped"),
        }
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
            if let Some(envelope) = err.0.as_any().downcast_mut::<Envelope<M>>() {
                Err(SendError::Closed(envelope.unwrap()))
            } else {
                Err(SendError::Lost)
            }
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
    pub async fn try_async_ask<M>(&self, message: M) -> Option<A::Output>
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
    pub async fn async_ask<M>(&self, message: M) -> Result<A::Output, AskError<M>>
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

    pub async fn schedule(self, duration: Duration) -> Self {
        tokio::time::sleep(duration).await;
        self
    }

    /// Similar to just [IntoFuture](#into_future), but instead of asking the actor
    /// to stop, we subscribe to the completion of the actor some time in the future.
    /// Calling this method a second time would replace the current subscriber which
    /// will result in this call failing for the first caller.
    ///
    /// Only one caller can subscribe to the completion of an actor currently.
    pub async fn wait_for_completion(self) -> Result<A, IntoFutureError> {
        let (tx, rx) = oneshot::channel();
        if (self
            .internal_send_async(IntoFutureShutdown::new(false, tx))
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
    }
}

/// Errors that can occur if the supervios actor dies while awaiting for the child
/// actors to finish they panic.
#[derive(Debug, PartialEq, Eq)]
pub enum IntoFutureError {
    MailboxClosed,
    Paniced,
}

impl std::error::Error for IntoFutureError {}

impl std::fmt::Display for IntoFutureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IntoFutureError::MailboxClosed => {
                writeln!(
                    f,
                    "Actor failed to resolve because it's mailbox is already closed"
                )
            }
            IntoFutureError::Paniced => {
                writeln!(f, "Actor failed to resolve because of it Paniced")
            }
        }
    }
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
                    .internal_send_async(IntoFutureShutdown::new(true, tx))
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
