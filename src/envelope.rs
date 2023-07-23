use std::{any::Any, future::Future, pin::Pin, task::Poll};

use tokio::sync::oneshot;

use crate::{
    actor::{Ask, AsyncAsk, InternalHandler},
    message::AnonymousTaskCancelled,
    Actor, Ctx, Handler, Message, Scheduler,
};

/// Contain a message so that it can be accessed by the framework.
pub(crate) struct ConfidentialEnvelope<M: Message> {
    msg: Option<M>,
}

impl<M: Message> ConfidentialEnvelope<M> {
    pub fn new(msg: M) -> Self {
        Self { msg: Some(msg) }
    }

    pub fn unwrap(&mut self) -> M {
        self.msg.take().unwrap()
    }
}

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
    fn send<'a>(&mut self, actor: &'a mut A, context: &'a mut Ctx<A>) -> Resolve<'a>;

    fn as_any(&mut self) -> &mut dyn Any;

    fn scheduler(&self) -> Scheduler;
}

impl<M: Message, A: InternalHandler<M>> SendMessage<A> for ConfidentialEnvelope<M> {
    fn send<'a>(&mut self, actor: &'a mut A, context: &'a mut Ctx<A>) -> Resolve<'a> {
        actor.private_handler(self.unwrap(), context);
        Resolve::ready()
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    fn scheduler(&self) -> Scheduler {
        Scheduler::NonBlocking
    }
}

/// Delivery of a message is handled by the envelope delivery service. It is
/// able to tell the type of message that is being sent in the envelope so that
/// the user only knows they are dealing with a user message. System messages
/// are handled through our own internal system.
impl<M: Message, A: Handler<M>> SendMessage<A> for Envelope<M> {
    fn send<'a>(&mut self, actor: &'a mut A, context: &'a mut Ctx<A>) -> Resolve<'a> {
        actor.handle(self.unwrap(), context);
        Resolve::ready()
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    fn scheduler(&self) -> Scheduler {
        A::scheduler()
    }
}

/// Delivery of a message that is asking for a response of some kind.
impl<M: Message, A: Ask<M>> SendMessage<A> for Response<M, A::Result> {
    fn send<'a>(&mut self, actor: &'a mut A, context: &'a mut Ctx<A>) -> Resolve<'a> {
        let message = self.msg.unwrap();
        let response = actor.handle(message, context);
        let _ = self.tx.take().unwrap().send(response);
        Resolve::ready()
        // TODO(Alec): Add tracing
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    fn scheduler(&self) -> Scheduler {
        A::scheduler()
    }
}

/// Delivery of a message that is asking for a response of some kind.
impl<M, A> SendMessage<A> for AsyncResponse<M, A::Output>
where
    M: Message,
    A: AsyncAsk<M>,
    A::Output: Send + Sync,
{
    fn send<'a>(&mut self, actor: &'a mut A, context: &'a mut Ctx<A>) -> Resolve<'a> {
        let message = self.msg.unwrap();
        let mut rx = context.notifier.subscribe();
        let _ = rx.borrow_and_update();

        let supervisor = context.address();
        let tx = self.tx.take().unwrap();
        let future = actor.handle(message, context);
        let box_future = Box::pin(async move {
            tokio::select! {
                _ = rx.changed() => {
                    let _ = supervisor.internal_send_async(AnonymousTaskCancelled::Cancel).await;
                }
                response = future => {
                    let _ = tx.send(response);
                }
            };
        });
        Resolve::future(box_future)
        // TODO(Alec): Add tracing
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    fn scheduler(&self) -> Scheduler {
        A::scheduler()
    }
}

pub struct Resolve<'a> {
    fut: Option<Pin<Box<dyn Future<Output = ()> + Send + Sync + 'a>>>,
}

impl<'a> Resolve<'a> {
    fn future(fut: Pin<Box<dyn Future<Output = ()> + Send + Sync + 'a>>) -> Self {
        Self { fut: Some(fut) }
    }

    fn ready() -> Self {
        Self { fut: None }
    }
}

impl<'a> Future for Resolve<'a> {
    type Output = ();

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut this = self.as_mut();
        let future = this.fut.as_mut();
        match future {
            Some(fut) => core::pin::pin!(fut).poll(cx),
            None => Poll::Ready(()),
        }
    }
}
