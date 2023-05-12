use std::{future::Future, marker::PhantomData};

use crate::{Actor, Ask, AsyncAsk, Ctx, Handler, Message};

pub struct AnonymousActor<In: Message, Out: Message, F: Fn(In) -> Out> {
    f: Option<F>,
    _in: PhantomData<In>,
    _out: PhantomData<Out>,
}

impl<In, Out, F> From<F> for AnonymousActor<In, Out, F>
where
    In: Message,
    Out: Message,
    F: Fn(In) -> Out + Send + Sync + 'static,
{
    fn from(f: F) -> Self {
        Self {
            f: Some(f),
            _in: PhantomData,
            _out: PhantomData,
        }
    }
}

impl<In, Out, F> Actor for AnonymousActor<In, Out, F>
where
    In: Message,
    Out: Message,
    F: Fn(In) -> Out + Send + Sync + 'static,
{
    fn name() -> &'static str {
        "AnoymousFnActor"
    }

    fn mailbox_size() -> usize {
        1
    }
}

impl<In, F> Handler<In> for AnonymousActor<In, (), F>
where
    In: Message,
    F: Fn(In) + Send + Sync + 'static,
{
    fn handle(&mut self, message: In, _: &mut Ctx<Self>) {
        let f = self.f.take().unwrap();
        (f)(message);
    }
}

impl<In, Out, F> Ask<In> for AnonymousActor<In, Out, F>
where
    In: Message,
    Out: Message,
    F: Fn(In) -> Out + Send + Sync + 'static,
{
    type Result = Out;

    fn handle(&mut self, message: In, _: &mut Ctx<Self>) -> Self::Result {
        let f = self.f.take().unwrap();
        (f)(message)
    }
}

impl<In, Out, Fut, F> AsyncAsk<In> for AnonymousActor<In, Fut, F>
where
    In: Message,
    Out: Message,
    Fut: Message + Future<Output = Out>,
    F: Fn(In) -> Fut + Send + Sync + 'static,
{
    type Result = Out;

    fn handle(&mut self, message: In, ctx: &mut Ctx<Self>) -> crate::AsyncHandle<Self::Result> {
        let f = self.f.take().unwrap();
        ctx.anonymous_handle(async move { (f)(message).await })
    }
}
