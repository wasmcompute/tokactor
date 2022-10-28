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
    Actor, Ctx, Handler, Message,
};

pub struct AnonymousRef {
    handle: JoinHandle<()>,
}

impl AnonymousRef {
    pub fn new(handle: JoinHandle<()>) -> Self {
        Self { handle }
    }

    pub fn halt(self) {
        self.handle.abort()
    }
}

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
    pub fn new(address: mpsc::Sender<Box<dyn SendMessage<A>>>) -> Self {
        Self { address }
    }

    /// Send a message to an actor and don't wait for a reponse. This is mainly
    /// used to forward or send requests deeper into the system. Messages will be
    /// siliently dropped if they fail to be pushed to the recieving queue
    pub fn try_send<M>(&self, message: M)
    where
        M: Message,
        A: Actor + Handler<M>,
    {
        println!("TRY SENDING MESSAGE TO {}", A::KIND);
        if let Err(err) = self.address.try_send(Box::new(Envelope::new(message))) {
            match err {
                mpsc::error::TrySendError::Full(_) => {
                    println!("FAILED TO PUSH MESSAGE TO {}", A::KIND);
                }
                mpsc::error::TrySendError::Closed(_) => {
                    println!("FAILED TO PUSH MESSAGE TO {} IS CLOSED", A::KIND);
                }
            }
        }
    }

    pub fn send<M>(&self, message: M) -> Result<(), mpsc::error::TrySendError<M>>
    where
        M: Message,
        A: Actor + Handler<M>,
    {
        println!("SENDING MESSAGE TO {}", A::KIND);
        if self.address.is_closed() {
            Err(mpsc::error::TrySendError::Closed(message))
        } else if let Err(err) = self.address.try_send(Box::new(Envelope::new(message))) {
            todo!("How to handle boxed traits when no space on reciever")
        } else {
            Ok(())
        }
    }

    pub async fn async_send<M>(&self, message: M) -> Result<(), Box<M>>
    where
        M: Message,
        A: Actor + Handler<M>,
    {
        if let Err(err) = self.address.send(Box::new(Envelope::new(message))).await {
            todo!("How to handle boxed traits???")
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum IntoFutureError {
    MailboxClosed,
    Paniced,
}

/// Stop an actor from executing and wait for all messages and children to die that
/// is attached to the actor. Once the actor and all of the children have cleaned up,
/// this method will return with the completed actor.
impl<A: Actor<Context = Ctx<A>>> IntoFuture for ActorRef<A> {
    type Output = Result<A, IntoFutureError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + Sync>>;

    fn into_future(self) -> Self::IntoFuture {
        if self.address.is_closed() {
            Box::pin(async { Err(IntoFutureError::MailboxClosed) })
        } else {
            let (tx, rx) = oneshot::channel();
            self.send(IntoFutureShutdown::new(tx));
            Box::pin(async move {
                match rx.await {
                    Ok(actor) => Ok(actor),
                    Err(_) => Err(IntoFutureError::Paniced),
                }
            })
        }
    }
}
