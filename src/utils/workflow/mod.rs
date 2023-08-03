use std::{future::Future, pin::Pin};

use crate::{Actor, ActorRef, Ask, Message};

use self::then::Then;

mod then;

// Private base trait to allow running a step of a workflow without it being a
// breaking change
pub trait WorkflowBase<In: Message> {
    type Output: Message;
    type Future: Future<Output = Self::Output> + Send;

    fn run(self, inner: In) -> Self::Future;
}

/// Composable workflow steps
///
/// A `Workflow` is a series of steps some data may need to go through before
/// completing at an end state. Workflows can have branching paths. They are
/// useful in describing the path of data through the system as it needs to
/// get data from different actors as well as adhoc functions.
///
/// Chain together anonymous functions and actor references to create a workflow
/// that is repeatable.
pub trait Workflow<In: Message>: WorkflowBase<In> {
    /// Execute async function
    fn then<Out, F>(self, func: F) -> Then<Self, F>
    where
        Self: Sized + WorkflowBase<In> + Send + 'static,
        Out: Message,
        F: Workflow<Self::Output, Output = Out> + Send + 'static,
    {
        Then { a: self, b: func }
    }
}

impl<In, Out, Fut, F> WorkflowBase<In> for F
where
    In: Message,
    Out: Message,
    Fut: Future<Output = Out> + Send,
    F: FnOnce(In) -> Fut,
{
    type Output = Out;
    type Future = Fut;

    fn run(self, inner: In) -> Self::Future {
        (self)(inner)
    }
}

impl<In, A> WorkflowBase<In> for ActorRef<A>
where
    In: Message,
    A: Actor + Ask<In>,
{
    type Output = A::Result;
    type Future = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn run(self, input: In) -> Self::Future {
        Box::pin(async move {
            if let Ok(response) = self.ask(input).await {
                return response;
            }
            todo!("Handle errors bro")
        })
    }
}

impl<In: Message, T: WorkflowBase<In>> Workflow<In> for T {}

/// Chain together steps in the overall workflow that a message must go through
/// to be treated as a "success"
pub struct Chained<A, B> {
    a: A,
    b: B,
}

impl<In, A, B> WorkflowBase<In> for Chained<A, B>
where
    In: Message,
    A: Workflow<In> + Send + 'static,
    B: Workflow<A::Output> + Send + 'static,
{
    type Output = B::Output;
    type Future = Pin<Box<dyn Future<Output = B::Output> + Send>>;

    fn run(self, input: In) -> Self::Future {
        Box::pin(async move {
            let output = self.a.run(input).await;
            self.b.run(output).await
        })
    }
}

#[cfg(test)]
mod tests {

    use crate::{Actor, Ask, AskResult};

    use super::{Workflow, WorkflowBase};

    async fn add_one(input: i32) -> i32 {
        input + 1
    }

    #[tokio::test]
    async fn chain_functions_together_into_workflow() {
        let workflow = add_one.then(add_one).then(add_one).then(add_one);
        let o1 = workflow.run(0).await;
        assert_eq!(o1, 4);
    }

    struct AddOnce {}
    impl Actor for AddOnce {}

    #[derive(Debug, PartialEq, Eq)]
    enum Number {
        I32(i32),
        I64(i64),
        I128(i128),
        U32(u32),
        U64(u64),
    }
    // IntoAskMessage!(Number);

    impl Ask<Number> for AddOnce {
        type Result = Number;

        fn handle(&mut self, message: Number, _: &mut crate::Ctx<Self>) -> AskResult<Self::Result> {
            AskResult::Reply(match message {
                Number::I32(i32) => Number::I64(i32 as i64),
                Number::I64(i64) => Number::I128(i64 as i128),
                Number::I128(i128) => Number::U32(i128 as u32),
                Number::U32(u32) => Number::U64(u32 as u64),
                Number::U64(u64) => Number::I32(u64 as i32),
            })
        }
    }

    #[tokio::test]
    async fn chain_actor_messages_together_into_workflow() {
        let addr = AddOnce {}.start();
        let ad2 = addr.clone();
        let output = addr
            .clone()
            .then(move |input| async move {
                if let Ok(out) = ad2.ask(input).await {
                    return out;
                }
                todo!("error")
            })
            .then(addr.clone())
            .then(addr.clone())
            .run(Number::I32(0))
            .await;
        assert_eq!(output, Number::U64(0));
    }
}
