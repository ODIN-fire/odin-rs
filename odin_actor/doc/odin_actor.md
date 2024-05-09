# odin_actor

The `odin_actor` crate provides an implementation for a typed [actor model](https://en.wikipedia.org/wiki/Actor_model)
that serves as the basis for ODIN applications.

From a Rust perspective this is a library that implements actors as async tasks that process input received through 
actor-specific channels and encapsulate actor specific state (not visible to the outside). It is an architectural
abstraction layer on top of async runtimes (such as [tokio](https://tokio.rs/)).

The crate mostly provides a set of macros that implement a DSL for construction and operation of actor systems, namely

- [`define_actor_msg_set`] to define an enum for all messages understood by an actor
- [`impl_actor`] to define the actor as a 3-tuple of actor state, actor message set and a `receive` function that provides
  the (possibly state dependent) behavior for each input message (such as sending messages to other actors) 

Applications can be as simple as this:

```rust
// define messages understood by our actor
pub struct Greet(&'static str);
define_actor_msg_set! { pub GreeterMsg = Greet }

// define our actor state
pub struct Greeter { name: &'static str }

// define the actor tuple (incl. behavior)
impl_actor! { match msg for Actor<Greeter,GreeterMsg> as
    Greet => term! { println!("{} sends greetings to {}", self.name, msg.0); }
}

// instantiate and run the actor system
#[tokio::main]
async fn main() ->Result<()> {
    let mut actor_system = ActorSystem::new("greeter_app");

    let actor_handle = spawn_actor!( actor_system, "greeter", Greeter{name: "me"})?;
    actor_handle.send_msg( Greet("world")).await?;

    actor_system.process_requests().await?;

    Ok(())
}
```