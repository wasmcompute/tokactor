mod actor;
mod address;
mod context;
mod envelope;
mod message;
mod utils;

pub use crate::actor::{Actor, Ask, AsyncAsk, Handler};
pub use crate::address::{ActorRef, AnonymousRef, AskError, IntoFutureError, SendError};
pub use crate::context::{ActorContext, AsyncHandle, Ctx};
pub use crate::message::{DeadActorResult, Message};

pub mod util {
    pub use crate::utils::workflow::Workflow;
}

pub mod macros {
    #[macro_export]
    macro_rules! IntoAskMessage {
        ($id: ident) => {
            impl Message for $id {}
            impl IntoFuture for $id {
                type Output = Self;
                type IntoFuture = Ready<Self>;

                fn into_future(self) -> Self::IntoFuture {
                    std::future::ready(self)
                }
            }
        };
    }

    pub use IntoAskMessage;
}
