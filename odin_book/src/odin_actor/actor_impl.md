# Basic Design

This chapter describes how the general actor constructs introduced in [actor_basics](actor_basics.md) are implemented in `odin_actor`,
which reflects our major design choices:

- map each actor into a dedicated [async task](https://rust-lang.github.io/async-book/) that owns the actor state 
- use an actor specific enum type to define the set of messages that can be sent to/are processed by this actor
  (each message type is wrapped into a tuple struct variant of this enum)
- use bounded [multi-producer/single-consumer (MPSC) channels](https://doc.rust-lang.org/rust-by-example/std_misc/channels.html)
  of this message set enum to implement actor mailboxes 
- wrap the sender part of the channel into a (cloneable) actor handle and move the receiver part and the actor state into
  the task function, which loops to process received messages
- use normal enum matching to dispatch messages from within the actor task
- use the actor handle to send messages to the associated actor

This ensures our basic requirements:

- actor message interfaces can be checked at compile time - we can only send messages to actors who process them, and
  each actor processes all of the message types in its interface
- actor state cannot accidentally leak from within its task (neither during construction nor while sending messages)
- actors can process concurrently (and - depending on async runtime and hardware - in parallel)
- message processing back pressure is propagated (bounded channel write blocks until receiver is ready), the system related
  memory per actor is bounded (no out-of-memory conditions because of "hung" actors)

The remainder of this page looks at each of the actor elements: messages, mailboxes, actors (handles and state) and actor systems.


## Messages and Actor Message Sets

Messages are ordinary structs, they do not require any `odin_actor` specific overhead other than that they for obvious reasons 
have to be `Send` and have to implement `Debug` (`odin_actor` requirement to support debug/logging).

The `odin_actor` crate does define a number of *system messages* for lifetime control and monitoring purposes (`_Start_`,
`_Pause_`, `_Resume_`, `_Timer_`, `_Exec_`, `_Ping_`, `_Terminate_`). Those messages do not have to be handled explicitly by actors (although
they can, should the actor require specific actions). System messages can be sent to any actor.

*Message sets* are the complete message interfaces of their associated actors. They are implemented as `enums` since we 
want to be able to statically (at compile time) check that

- an actor processes all message types in its interface (no "forgotten" messages)
- we can only send messages to actors who have this message type in their interface

Other than for actor definition message set enums are mostly transparent, which means they need `From<msg-type>` impls for all their
variants. Message sets have to include the system messages mentioned above. Since this would be tedious to define explicitly we provide 
the `define_actor_msg_set!(..)` macro that can be used like so:

```rust
use odin_actor::prelude::*;

#[derive(Debug)] struct MsgA(usize);
#[derive(Debug)] struct MsgB(usize);

define_actor_msg_set! { MyActorMsg = MsgA | MsgB }
...
```

This gets expanded to an enum type with `From<T>` impls for each of its variants:

```rust
...
enum MyActorMsg {
    MsgA(MsgA),
    MsgB(MsgB)
}
impl From<MsgA> for MyActorMsg {...}
impl From<MsgB> for MyActorMsg {...}
...
```

The macro also adds variants for the system messages so that we can send them to each actor.

Apart from automatic `From<..>` impls the main operation performed on message set enums is matching their variants inside of
actor `receive()` functions. To avoid boilerplate and to make the code more readable we provide support matching on variant
types from within the `impl_actor! {..}` macro:

```rust
impl_actor! { match msg for Actor<MyActor,MyActorMsg> as
    MsgA => cont! { 
        // process msg: MyActorMsg::MsgA
    }
    ...
    _Start_ => cont! {
        // process msg: MyActorMsg::_Start_
    }
```

However, Rust enum variants are _not_ types, hence the framework automatically has to map type names (from the match arm patterns)
to variant identifiers, which requires name mangling in case of generic types and tuples. This name mangling is performed automatically
and uses similar valid unicode identifier characters (see `odin_macro` implementation) to ensure that compiler error messages are still
readable.

It should be noted that since we use `enums` to define message sets developers should be aware of the variant type sizes - Rust enums
are sized to accommodate the largest of their variants and mailboxes represent arrays of respective message set enums. Use `Arc<MyLargeType>` in case variants can get large.


## Mailboxes

Mailboxes are implemented as Rust `channels`, i.e. `odin_actor` does not provide its own type and uses (transparently) whatever the
configured channel implementation default to (e.g. [`flume::bounded`](https://docs.rs/flume/latest/flume/fn.bounded.html)). This is 
controlled at build time by `odin_actor` features (currently `tokio_kanal` or `tokio_flume`).

The `odin_actor` crates uses bounded channels, i.e. we do not support dynamically sized mailboxes. The rationale is to use mailbox
bounds for back pressure control and to prevent out-of-memory errors at runtime. This also means we have to support three types
of message sends:

- async send (potentially blocking until space becomes available)
- try_send (non-blocking but fails if mailbox is full)
- timeout_send (async with a specified max timeout - in between the above two choices)


## ActorHandle

`ActorHandle` is a system provided struct with a type parameter that represents the actor message set type. This type
is used to define the sender-part of the actor mailbox (mpsc channel - see [Actor](#actor) section below), which in
turn is what makes our actor message interfaces type safe (at compile time).

```rust
pub struct ActorHandle <M> where M: MsgTypeConstraints {
    pub id: Arc<String>,
    hsys: Arc<ActorSystemHandle>,
    tx: MpscSender<M> // internal - this is channel specific
}
```


Since `ActorHandle` is primarily used to send messages to the corresponding actor the main functions in its inherent impl are:

- `async fn send_msg<T> (&self, msg: T)->Result<()> where T: Into<M> {...}`
- `async fn timeout_send_msg<T> (&self, msg: T, to: Duration)->Result<()> where T: Into<M> {...}`
- `pub fn try_send_msg<T> (&self, msg:T)->Result<()> where T: Into<M> {...}`

Note that all are generic in the message type `T: Into<M>`, i.e. any type for which the respective actor message set `M`
has a `From` trait impl (which our `define_actor_msg_set!(..)` macro automatically generates).

`ActorHandles` have one basic requirement - they have to be inexpensive to clone. For that reason we use `Arc<T>` references
to store the id (name) and the `ActorSystemHandle` of the respective actor.

`ActorHandles` are not created explicitly - they are the return values of `spawn_actor!{..}` or `spawn_pre_actor!{..}` macro
calls.

The system also provides a `PreActorHandle<M>` struct that allows explicit construction in case we have cyclic dependencies
between actors. The sole purpose of `PreActorHandle` is to subsequently create `ActorHandles` from it. To that end it creates
and stores both sender and receiver parts of the actor task channel but it does not allow to use them - all its fields are private
and are just used as a temporary cache. The `spawn_pre_actor!{..}` macro is used to spawn actors from respective `PreActorHandles`. 


## Actor State

Just like the for the message types `odin_actor` accepts any `struct` as actor state, without the need for any specific
fields or trait impls.

There usually is an associated inherent impl for such structs which defines the functional interface of the actor. A common
pattern is to use minimal code in the actor impl itself and just call actor state methods from the message match expressions
like so:

```rust
struct MyActor {...}

impl MyActor {
    fn process_msg_a (&mut self, msg: MsgA) {
        ...
    }
    ...
}

impl_actor! { match msg for Actor<MyActor,MyActorMsg> as
    MsgA => cont! { 
        self.process_msg_a( msg)
    }
    ...
}
```


## Actor

The `odin_actor` crate uses a single generic actor type

```rust
pub struct Actor <S,M> where S: Send + 'static, M: MsgTypeConstraints {
    pub state: S,
    pub hself: ActorHandle<M>,
}
```

where the type variable `S` represents the user defined actor state type and the type variable `M` represents the actor 
message set type defined by a corresponding `define_actor_msg_set!(..)` invocation. The `Actor` type itself is mostly transparent, 
usually it is only visible at the location where a concrete actor is defined with the `impl_actor! { ... }` macro.

To avoid boilerplate in the associated message matcher code `odin_actor` provides blanket `Deref` and `DerefMut` impls that
forward to the `state: S` field. For the most part, developers can treat actor and actor state synonymously. 

One consequence of not having constraints on the actor state type and keeping system related data in the framework provided
`Actor<S,M>` struct is that we need to pass actor handles into inherent impl methods like so:

```rust
struct MyActor {...}

impl MyActor {
    async fn send_something (&mut self, hself: &ActorHandle<MyActorMsg>) {
        hself.send_msg(...).await
    }
    ...
}

impl_actor! { match msg for Actor<MyActor,MyActorMsg> as
    ... => cont! { 
        self.send_something( &self.hself).await
    }
    ...
}
```

We define concrete `Actor` types by means of our `impl_actor!{..}` macro, which has the primary purpose of generating
a `ActorReceiver<M>` trait impl for the concrete `Actor` type. This trait defines the function 

```rust
fn receive (&mut self, msg: MsgType)-> impl Future<Output = ReceiveAction> + Send
```

which is our actor message dispatcher (a matcher on the actor message set enum variants).

Once it is spawned at runtime the `Actor` is moved into its own Tokio task. Since the `Actor` owns the actor state `S` this
guarantees actor encapsulation - it is not visible to the outside anymore. The task in turn consists of a loop that awaits
incoming messages from the actor mailbox (task channel reader part) and then dispatches the message through the `receive()`
function of the `ActorReceiver` impl.

Each `receive` match arm has to return a `ReceiveAction` enum that tells the task how to proceed:

- `ReceiveAction::Continue` continues to loop, waiting for the next message to receive
- `ReceiveAction::Stop` breaks the loop and terminates message processing for this actor. This is the default 
   result when dispatching `_Terminate_` system messages
- `ReceiveAction::RequestTermination` sends a termination request to the associated `ActorSystem` but continues
   to loop. The `ActorSystem` in turn sends `_Terminate_` messages to all its actors in response

The system provides the `cont!{..}`, `stop!{..}` and `term!{..}` macros as syntatic sugar to make sure match arm expressions
do return respective `ReceiveAction` values.


## ActorSystem

Spawning actor tasks and transferring ownership of its `Actors` is the responsibility of the system provided `ActorSystem`
struct. Its main function therefore is `spawn_actor(..)` which is normally just called by the `spawn_actor!{..}` macro that
transparently 

- creates a MPSC channel for the actor message set type
- creates an `ActorHandle` that stores the sender part of the channel
- creates an `Actor` from the provided actor state object and the `ActorReceiver` impl generated by the associated
  `impl_actor!{..}` call (which means it has to be in scope at the point of the `spawn_actor{..}` call so that the
  compiler can deduce the message type set)
- spawns a new task with the system provided `run_actor(..)` task function, moving both the `Actor` and the receiver
  part of the MPSC channel into this task

The `ActorSystem` also keeps track of all running actors as a list of `SysMsgReceiver` trait objects. This means 
`ActorSystem` can only interact with `Actors` by sending system messages. For this purpose `ActorSystem` has its own 
task that processes `ActorSystemRequest` messages, of which the `ActorSystemRequest::RequestTermination` (sent by
`run_actor` in response to a `ReceiveAction::RequestTermination` return value from the actor `receive()` function) is
the most common one.

Based on its list of `SysMsgReceivers` the `ActorSystem` also manages heart beats (system liveness monitoring) and 
a build-time configurable user interface to display the system status. Both are transparent to the application.

`ActorSystem` is the primary object for actor based applications, which all follow the same general structure:

```rust
...

#[tokio::main]
async fn main() ->Result<()> {
    // create the actor system
    let mut actor_system = ActorSystem::new("main");

    // spawn actors
    let handle_a = spawn_actor!( actor_system, "A", ActorA{..})?;
    let handle_b = spawn_actor!( actor_system, "B", ActorB{..})?;
    ...

    // run the actor system
    actor_system.start_all().await?;
    actor_system.process_requests().await?;

    Ok(())
}
```


There are two underlying abstractions that can be varied for an `ActorSystem` implementation: async runtime and actor
task channel type. Both are configured by a [Cargo build feature](https://doc.rust-lang.org/cargo/reference/features.html)
and provide the same interface. At this time we support 

- the default `tokio_kanal` ([Tokio](https://tokio.rs/) runtime and [Kanal](https://crates.io/crates/kanal) MPSC channel type)
- `tokio_flume` (using the [Flume](https://docs.rs/flume/latest/flume/) MPSC channel type)

Within the same process only one combination can be used.