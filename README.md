# Wasmcompute Actors

This is a simple library that wraps tokio and gives access to it through an actor
model interface. Actors are linked together through channels created from tokio.
Actors are ran as `tokio::task` and are given a channel of messages 

This library was designed to try and not expose the need to program generic futures
through `Pin<Box<_>>`.

More documentation to come later.

## Utilities

Working with this actor library can be a very harsh experience. For instance, the
library only gives the user access to synchronous functions and any async functions
are required to be executed on tokio tasks. It can be hard to follow a chain of
messages that are sent through the system from one actor to another and back. To
make the flow of data easier to follow, utility concepts are built on top of the
core actor library.

### Workflows

Describe the flow of data that needs to be followed as data makes it's way through
the system.
