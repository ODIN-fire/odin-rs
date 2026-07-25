# odin_actor

The `odin_actor` crate provides an implementation of a typed [actor model](https://en.wikipedia.org/wiki/Actor_model)
that serves as the common basis for ODIN applications.

*Actors* are objects that execute concurrently and only communicate through asynchronous *Messages*. Actors do not share their internal *State* and are only represented to the outside by *ActorHandles*. The only operation supported by ActorHandles is to send messages to the actor, which are then queued in an (actor internal) *Mailbox* and processed by the actor in the order in which they were received. In reaction to received messages actors can send messages or mutate their internal state:

```diagram
         ╭──────╮
   ─────▶︎│Handle│─────x:X──╮ Message
       ┌─┴──────┴──────────│───┐
       │ Actor   State   ┌─▼─┐ │
       │          ▲      ├─:─┤ MailBox
       │          │      └───┘ │
       │          ▼        │   │
       │   receive(m) ◀︎────╯   │
       │     match m           │
       │       X => process_x  │
       │    ...          ───────────▶︎ send messages to other actors
       └───────────────────────┘ 
```

From a Rust perspective this is a library that implements actors as async tasks that process input received through 
actor-owned channels and encapsulate actor specific state that is not visible to the outside. It is an architectural
abstraction layer on top of async runtimes (such as [tokio](https://tokio.rs/)).

In `odin_actor` we map the message interface of an actor to an `enum` containing variants for all message types 
understood by this actor (variants can be anything that satisfies Rust's `Send` trait). The actor state is a user
defined `struct` containint the data that is owned by this actor. Actor behavior defined as a `trait` impl that
consists of a single `receive` function that matches the variants of the actor message enum to user defined expressions. 

Please refer to the respective chapter in the `odin_book` for more details.

The `odin_actor` crate mostly provides a set of macros that implement a DSL for defining and instantiating these
actor components, namely

- [`define_actor_msg_set`] to define an enum for all messages understood by an actor
- [`impl_actor`] to define the actor as a 3-tuple of actor state, actor message set and a `receive` function that provides
  the (possibly state dependent) behavior for each input message (such as sending messages to other actors) 
- [`spawn_actor`] to instantiate actors and start their message receiver tasks

Here is the "hello world" example of `odin_actor`, consisting of a single Greeter actor:

```rust
use tokio;
use odin_actor::prelude::*;
use anyhow::{anyhow,Result};

// define actor message set ①
#[derive(Debug)] pub struct Greet(&'static str);
define_actor_msg_set! { pub GreeterMsg = Greet }

// define actor state ②
pub struct Greeter { name: &'static str }

// define the actor tuple (incl. behavior) ③
impl_actor! { match msg for Actor<Greeter,GreeterMsg> as
    Greet => term! { println!("{} sends greetings to {}", self.name, msg.0); }
}

// instantiate and run the actor system ④
#[tokio::main]
async fn main() ->Result<()> {
    let mut actor_system = ActorSystem::new("greeter_app");

    let actor_handle = spawn_actor!( actor_system, "greeter", Greeter{name: "me"})?;
    actor_handle.send_msg( Greet("world")).await?;

    actor_system.process_requests().await?;

    Ok(())
}
```

This breaks down into the following four parts:

1. define actor message set
2. define actor state 
3. define the actor tuple (incl. behavior) 
4. instantiate and run the actor system


## Creating and running an `ActorSystem`

The basic steps to bring an `ActorSystem` to life are:

1. create the `ActorSystem` context
2. spawn the `Actor` instances to run in it
3. send `_Start_` messages to all of them
4. await for the actor system to process all requests

Step 1 creates the `ActorSystem` itself and sets its execution context, including trace support (i.e. observability and 
error/warn/info/debug log level support through respective explicit macro calls in the code) and termination handlers (graceful
ctrl-C shutdown)

Step 2 uses the `spawn_actor!{ .. }` and `spawn_pre_actor!{ .. }` macros which receive the actor state objects as arguments
and then (transparently) wrap them into `Actor` instances (representing tasks), returning `ActorHandle` objects that can
be used in initialization of subsequent actors. Use `PreActorHandle` instances to break cyclic `Actor` dependencies

Step 3 is usually performed by calling the `ActorSystem::timeout_start_all(..)` function and awaiting its result, hence this
has to happen from an `async` context. This is usually using a timeout duration to detect cases in which actors fail to initialize
in time.

Step 4 involves an `await` which therfore needs to be executed from an `async` function/block. This blocks until there is no
more active actor left in the `ActorSystem`.

For convenience we provide the `run_actor_system!{ .. }` macro that performs steps 1, 3 and 4. It is called like this:

```rust
..
run_actor_system!( actor_system => {
   // step 2: spawn actors
   Ok(())
});
```

This macro automatically creates an async `main` function and imports both the `tokio` and `anyhow` crates which therefore
need to be dependencies of the application crate (listed in its Cargo.toml).

To execute such an application with tracing support, launch it by setting both the `ODIN_TRACE` and the `RUST_LOG` environment
variables, e.g. like

```sh
> ODIN_TRACE=1 RUST_LOG=info   cargo run ...
```

We require the explicit environment variables to make sure the associated runtime overhead is only added on-demand.


## Terminating an `ActorSystem`

Actor systems terminate once there is no more active actor, i.e. all have terminated their associated message processing tasks. There
are three ways actors can be terminated:

* programmatically by the actor handler return values (`ReceiveAction` enum with `Continue`, `Stop` and `RequestTermination` variants)
* programmatically by sending `_Terminate_` messages to actors or calling and awaiting `request_termination(..)` on an `ActorSystemHandle`
* manually by sending SIGINT to the actor system process (e.g. by ctrl-C keyboard input)

There are respective `cont!{..}`, `stop!{..}` and `term!{..}` macros to associate return values to messages from within the actor
message handlers. Usually the only `Stop` response is from the automatically provided `_Terminate_` handler. `RequestTermination`
differs from `Stop` by sending a `_Terminate_` to the actor, i.e. it makes sure all pending messages are still processed.
