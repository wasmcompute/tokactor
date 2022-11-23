use std::any::Any;

use tokio::sync::oneshot;

use crate::{
    actor::{Ask, AsyncAsk},
    message::AnonymousTaskCancelled,
    Actor, Ctx, Handler, Message,
};

/// Contain a message so that it can be accessed by the framework.
pub struct Envelope<M: Message> {
    msg: Option<M>,
}

impl<M: Message> Envelope<M> {
    pub fn new(msg: M) -> Self {
        Self { msg: Some(msg) }
    }

    pub fn unwrap(&mut self) -> M {
        self.msg.take().unwrap()
    }
}

pub struct Response<M: Message, R: Message> {
    msg: Envelope<M>,
    tx: Option<oneshot::Sender<R>>,
}

impl<M: Message, R: Message> Response<M, R> {
    pub fn new(msg: Envelope<M>, tx: oneshot::Sender<R>) -> Self {
        Self { msg, tx: Some(tx) }
    }
}

pub struct AsyncResponse<M: Message, R: Message> {
    msg: Envelope<M>,
    tx: Option<oneshot::Sender<R>>,
}

impl<M: Message, R: Message> AsyncResponse<M, R> {
    pub fn new(msg: Envelope<M>, tx: oneshot::Sender<R>) -> Self {
        Self { msg, tx: Some(tx) }
    }
}

pub trait SendMessage<A: Actor>: Send + Sync {
    fn send(&mut self, actor: &mut A, context: &mut Ctx<A>);

    fn as_any(&mut self) -> &mut dyn Any;
}

/// Delivery of a message is handled by the envelope delivery service. It is
/// able to tell the type of message that is being sent in the envelope so that
/// the user only knows they are dealing with a user message. System messages
/// are handled through our own internal system.
impl<M: Message, A: Handler<M>> SendMessage<A> for Envelope<M> {
    fn send(&mut self, actor: &mut A, context: &mut Ctx<A>) {
        actor.handle(self.msg.take().unwrap(), context);
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// Delivery of a message that is asking for a response of some kind.
impl<M: Message, A: Ask<M>> SendMessage<A> for Response<M, A::Result> {
    fn send(&mut self, actor: &mut A, context: &mut Ctx<A>) {
        let message = self.msg.unwrap();
        let response = actor.handle(message, context);
        let _ = self.tx.take().unwrap().send(response);
        // TODO(Alec): Add tracing
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// Delivery of a message that is asking for a response of some kind.
impl<M, A> SendMessage<A> for AsyncResponse<M, A::Result>
where
    M: Message,
    A: AsyncAsk<M>,
{
    fn send(&mut self, actor: &mut A, context: &mut Ctx<A>) {
        let message = self.msg.unwrap();
        let supervisor = context.address();
        let tx = self.tx.take().unwrap();
        let future = actor.handle(message, context);
        context.anonymous_task(async move {
            let handle = future.inner.await;

            match handle {
                Ok(anonymous) => {
                    if let Some(response) = anonymous.result {
                        let _ = tx.send(response);
                    } else {
                        // The task was cancelled by the supervisor so we are just
                        // going to drop the work that was being executed.
                        println!("Cancelled");
                        let _ = supervisor.send_async(AnonymousTaskCancelled {}).await;
                    }
                }
                Err(_) => {
                    // The task ended by a user cancelling or the function panicing.
                    // Drop reciver to register function as complete.
                    println!("Error");
                    let _ = supervisor.send_async(AnonymousTaskCancelled {}).await;
                }
            };
        });
        // TODO(Alec): Add tracing
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}
