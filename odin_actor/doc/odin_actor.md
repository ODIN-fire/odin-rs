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

### ① define actor message set



### ② define actor state 

### ③ define the actor tuple (incl. behavior) 

### ④ instantiate and run the actor system 