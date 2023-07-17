# Wasmcompute Actors

A rust actor model library meant to wrap around tokio in an opinionated way to provide
type safe computing.

## About

Although other actor frameworks exist, I wanted to create my own that provides a
high level of safety and control over the lifecycle of actors. This wasmcompute
actors focuses on staying true to the idea of the actor model system by not
allowing programmed actors to compute asynchronous tasks. Instead, it's programming
model requires users to spawn anonymous actors that handle the async task.

I would hope that this library provides a complete wrapper around `tokio` so that
users would interact with it through this library. Actors are linked together
through channels created from `tokio`. Actors are ran as `tokio::task` and are
given a channel to receive messages from other actors and it's supervisor.

This library was designed to try and not expose the need to program generic futures
through `Pin<Box<_>>`.

## Installation

Install wasmcompute actors by adding the following to your Cargo.toml dependencies.

```toml
[dependencies]
am = "0.1"
```

## Worlds

Actor systems are created inside **Worlds**. These worlds allow the user to communicate
what type of external input that they want into the system. Events received by the
world will be handled by a function that the user would provide. Worlds can listen
to more then one type of event.

It's expected that this function will handle routing and calling into the existing
actor system.

```rust
use tokactor::{World}

struct Router {
    db: Db
}
impl Actor for Router {}

fn main() {
    let world = World::new().unwrap();
    
    let db = world.with_state(async || Db::connect().await.unwrap());

    let router = Router { db };

    let tcp_input = world.tcp_component("localhost:8080", router);

    world.on_input(tcp_input);

    world.block_until_completion();
}

```

Inputs into the system should be light weight and contain very little data.
The actors internally in your system should be able to use these input objects
to find the larger data inside.

## Working with Actors

Actors are light weight `tokio::tasks` and are only ever ran on one thread.
Messages are processed one at a time without the possibility to handle messages
in parallel.

As is required for any actor model library, here is a ping pong example:

```rust
use tokactor::{Actor, Ask, Ctx, Message};

/// Actor that keeps count of the number of ping pong message it receives
pub struct PingPong {
    counter: u8,
}

/// This is the types of message [PingPong] supports
#[derive(Debug, Clone)]
pub enum Msg {
    Ping,
    Pong,
}
impl Msg {
    // retrieve the next message in the sequence
    fn next(&self) -> Self {
        match self {
            Self::Ping => Self::Pong,
            Self::Pong => Self::Ping,
        }
    }
    // print out this message
    fn print(&self) {
        match self {
            Self::Ping => print!("ping.."),
            Self::Pong => print!("pong.."),
        }
    }
}

impl Actor for PingPong {}
impl Ask<Msg> for PingPong {
    type Result = Msg;

    // This is our main message handler
    fn handle(&mut self, message: Msg, _: &mut Ctx<Self>) -> Self::Result {
        message.print();
        self.counter += 1;
        message.next()
    }
}

#[tokio::main]
async fn main() {
    let handle = PingPong { counter: 0 }.start();
    let mut message = Msg::Ping;
    for _ in 0..10 {
        message = handle.ask(message).await.unwrap();
    }
    let actor = handle
        .await
        .expect("Ping-pong actor failed to exit properly");
    assert_eq!(actor.counter, 10);
    println!("\nProcessed {} messages", actor.counter);
}
```

## Messaging actors

Because messages are processed sequentially, there is no way to use another actor
to stop the processing of a currently executing. Instead there are only 2 levels
of mailbox queues:

1. Actors spawned by another actor, are able to receive updates from their spawner (supervisor). For now, this is how a supervisor would shut down a child actor.
2. The normal mailbox for a given actor, where messages are processed sequentially.

### Types of messaging

There are 3 different types of messages you can send an actor. They are: `send`,
`ask`, and `async_ask`. Each have their own uses but also each incur a cost so
only use the next format when needed.

1. `send` means the actor implements the `tokactor::Handler` trait. This implementation does not return an answer.
2. `ask` means the actor implements the `tokactor::Ask` trait. This implementation is good to return some pre-computed state. Can return a pre-determined answer.
3. `async_ask` mean the actor implements the `tokactor::AsyncAsk` trait. This implementation requires the actor to return an anonymous asynchronous actor that can return a given answer. Best to use when more processing is needed to find an answer.

Internal messages are put in the same mailbox as normal messages. They have their
own messaging system for all generic actors. It is mainly used to stop and actor
and return it's state through `await`ing an `ActorRef`. This will destroy the
actors address for the rest of the program.

### Stopping an actor

There are 2 ways to request an actor to stop.

1. Have the actor stop at some point during it's life time
2. `.await` a `ActorRef`, which will send a message to the actor to stop executing and return itself

If you would like to still `await` an `ActorRef`, but you don't want stop it's
execution and instead subscribe to it completing, then you can just call `.wait_for_completion()`
on the `ActorRef`. This will allow the actor to execute until it decides to stop,
at which point, the value saved on the actor will be returned.

## Other types of actors

This actor library provides utility actors to handle different needs. Currently
the following features are provided by utility actors:

- Router
- Workflows
- Generic

### Router

Good for create multiple of the same base actors and sending them requests in
a round robin configuration.

```rust
let builder = RouterBuilder::new(5);
let router = Router::<ChoosenActor>::new(builder);
let address = router.start();
for _ in 0..5 {
    for i in 0..5 {
        let actor = address.async_ask(Id(())).await.unwrap();
        assert_eq!(actor.number, i + 1);
    }
}
let _ = address.await;
```

### Workflows

Working with this actor library can be a very harsh experience. For instance, the
library only gives the user access to synchronous functions and any async functions
are required to be executed on tokio tasks. It can be hard to follow a chain of
messages that are sent through the system from one actor to another and back. To
make the flow of data easier to follow, utility concepts are built on top of the
core actor library.

Workflows are ether asynchronous functions or `ActorRef` that handle a given input
and return some type of output.

### Generic Actors

Sometimes when creating an actor, you want it to be very configurable leading
to a generic heavy implementation. Sharing the address of this actor would require
your entire program to implement the actors given generic parameters. By building
an actor though a `CtxBuilder` however, you can create a generic heavy implementation
of an actor and then give access to it through multiple different addresses for
a given message.

```rust
let test = Test {
    _a: 0_u8,
    _b: 0_u16,
    _c: 0_u32,
};
let ctx = CtxBuilder::new(test);
let ctx = ctx.sender::<MsgA<u8>>();
let ctx = ctx.asker::<MsgB<u16>>();
let ctx = ctx.ask_asyncer::<MsgC<u32>>();
// each address relates to one message
let (a1, a2, a3) = ctx.run();
a1.send(MsgA(1_u8)).await.unwrap();
a2.ask(MsgB(1_u16)).await.unwrap();
a3.ask_async(MsgC(1_u32)).await.unwrap();
```

## Road Map

There are features that are missing from the library that would be smart to add
in. These are the features I would want to add to the library for it to reach a
`1.0.0` release.

- [x] Long running actors that send messages to themselves until they stop themselves
- [x] Provide ways to not require tokio as a dependency
- [x] Actors that handle accessing tcp
- [ ] Actors that handle accessing udp
- [ ] Actors that handle accessing filesystem
- [ ] Actors that handle accessing terminal
- [ ] Allow for supervisor actors to restart actors that fail with state intact
- [ ] Give more utility functions for creating larger workflows that can be hardcoded (Workflow builder)
- [x] Add tracing
- [ ] Raise the number of tests
- [ ] Post a comment on the PR with benchmark results
