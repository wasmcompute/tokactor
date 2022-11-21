use std::{future::Future, pin::Pin};

use crate::Message;

use super::{Workflow, WorkflowBase};

pub struct Then<T, W> {
    pub(super) a: T,
    pub(super) b: W,
}

impl<In, A, B> WorkflowBase<In> for Then<A, B>
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
