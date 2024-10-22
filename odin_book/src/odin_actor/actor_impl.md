# Basic Design

This chapter describes how the general actor constructs introduced in [actor_basics](actor_basics.md) are implemented in `odin_actor`.


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


## ActorSystem



two underlying abstractions that can be varied: async runtime and channel implementation
channel impl controlled by features
