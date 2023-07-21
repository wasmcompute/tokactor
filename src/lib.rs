#![recursion_limit = "512"]

mod actor;
mod address;
mod anonymous;
mod context;
mod envelope;
mod executor;
mod generic;
mod io;
mod message;
mod single;
mod utils;
mod world;

pub use crate::actor::{Actor, Ask, AsyncAsk, Handler, Scheduler};
pub use crate::address::{ActorRef, AnonymousRef, AskError, IntoFutureError, SendError};
pub use crate::context::{ActorContext, Ctx};
pub use crate::message::{DeadActorResult, Message};
pub use crate::world::{builder::WorldBuilder, messages::TcpRequest, World, WorldResult};

pub mod util {
    pub mod read {
        pub use crate::io::read::Read;
    }
    pub mod builder {
        pub use crate::single::{ActorAskRef, ActorAsyncAskRef, ActorSendRef, CtxBuilder};
    }
    pub use crate::utils::router::Router;
    pub mod io {
        pub use crate::io::{
            Component, ComponentReader, DataFrame, DataFrameReceiver, Reader, Writer,
        };
    }
    pub mod fs {
        pub use crate::io::fs::FsActor;
    }
    pub use crate::utils::workflow::Workflow;
}
