# Actor Communication

Actors don't live in isolation - their whole purpose is to build modular, scalable concurrent systems out of sets of 
communicating actors. We therefore need to define

- what can be sent between actors (messages), 
- how we can send messages
- how to program sender actors when, what and to whom messages should be sent

Moreover, in `odin_actor` we need to do this in a type-safe, statically checked way. Since our implementation language
is Rust we want to ensure that

- we can only send thread- and memory-safe messages
- we can only send messages to actors that handle them (are in the receiver's message interface)
- each actor behavior is complete (no forgotten messages in the actor implementation)
- actors combine (we can build systems out of generic actors that only need to know a minimum about each other)

While the first three requirements are supported by Rust in a straight forward way, the forth requirement is complex.
Let's take it one step at a time.

## Messages

This one is easy - `odin_actor` does not have a specific message type or trait. Anything that is `Send` + `Debug` +
`'static` can be a message. The `Send` constraint is obvious as we pass messages between concurrent tasks (actors).
The`Debug` constraint is only for generic tracing/debugging support. The message type needs to be `static` since it is
part of the receiver actors definition. Some message sender methods do also require `Clone` as all sender methods do
consume the message argument. Should cloning be inefficient the message can also be an `Arc<T>`.

Since an actor typically processes more than one message we need to wrap all of its input message types into an `enum`.
This message set becomes part of the generic `Actor<MsgSet,ActorState>` type. Defining the message set is supported by
the `define_actor_msg_set!{..}` macro:

```rust
define_actor_msg_set! { MyActorMsg = Msg1 | Msg2 }
```

which roughly expands to

```rust
// automatically generated code
enum MyActorMsg {
   Msg1(Msg1),                // user messages...
   Msg2(Msg2),                
   _Start_ (_Start_),         // system messages...
   ... 
   _Terminate_ (_Terminate_) 
}
impl From<Msg1> for MyActorMsg { ... }
impl From<Msg2> for MyActorMsg { ... }
...
```

The macro automatically adds variants for each of the system messages

- `_Start_` - sent by the `ActorSystem` to indicate that all actors have been instantiated and should start to process
- `_Timer_` - sent by timers created from within the actor
- `_Exec_` - a generic message that executes the provided closure within the actor task
- `_Pause_` and `_Resume_` - can be used by actor systems that replay content
- `_Terminate_` - sent by the `ActorSystem` to indicate the application is about to shut down

System messages don't need to be explicitly handled. They are sent either by the `ActorSystem` (e.g. `_Start_`) or
implicitly by `Actor` methods such as `start_timer(..)` or `exec(..)`.

The message set name (e.g. `MyActorMsg`) is then used to define the actor like so:

```rust
impl_actor! { match msg for Actor<MyActorMsg,MyActorState> as
  Msg1 => ... // handle message 'msg' of type 'Msg1'
  Msg2 => ...
}
```

Both `define_actor_msg_set!{..}` and `impl_actor!{..}` automatically translate generic message types (e.g. `Query<Q,A>`)
into valid variant names of the actor message set. Although this mapping is readable and intuitive the programmer does not
need to know (other than to understand related compiler error messages).

Using an enum to encode all possible input messages for an actor also explains why message types should not be large. Not
only would this increase `clone()` cost but it also would enlarge the message set enum, which is sized according to its largest
variant. Since this message set enum is the type of the actor mailbox (channel) this size matters.


## How to Send Messages

Actor mailboxes in `odin_actor` are implemented as bounded async channels. This means sending messages can block the
sender if the receiver queue is full. Since it depends on the actor/message types if this is acceptable we need to support
alternative send operations: 

- `send_msg(msg)` - this is an `async` function that can suspend the sender and hence can only be called from an `async` context
- `timeout_send_msg(msg,timeout)` - also `async` but guaranteed to finish in bounded time, possibly returning a timeout error
- `try_send_msg(msg)` - sync call. returning an error of the receiver queue is full
- `retry_send_msg(max_attempts,delay,msg)` - also sync but re-scheduling the message if receiver queue is full

It is important to note that `retry_send_msg(..)` *can* violate the property that messages from the *same sender* are
processed by the receiver in the order in which they were sent. If partial send order is required this has to be
explicitly enforced in the sender.

All send operations return `Result<(),OdinActorError>` values. Senders should handle `ReceiverClosed` and - for async sends -
`ReceiverFull` and/or `Timeout` error values.

Send methods are defined in `ActorHandle`, `MsgReceiver` and `Actor` (the latter one used to send messages to itself).

Normal message send operations are unidirectional - should the sender expect a response that needs to retain request
information it has to do this association explicitly (e.g. by copying relevant request info into the response message, or
by keeping a list of pending requests in the sender). 


## Waiting for a Response - `Query<Q,A>`

The bi-directional `query(..)` operations overcome this restriction in cases where the sender should wait for a response
before going on. The underlying message type is a generic `Query<Question,Answer>` struct which has to be in the
responders input message set, the concrete `Question` and `Answer` types being provided by the user (with normal message
type constraints). 

The requester sends queries like so:

```rust
...
  let question = ... 
  match query( responder_handle, question).await {
     Ok(response) => ... // process response value
     Err(e) => ... // handle failed query
  }
```

The corresponding responder code would be:

```rust
define_actor_msg_set! { ResponderMsg = ... | Query<Question,Answer> | ...}

impl_actor!{ match msg for Actor<ResponderMsg,ResponderState> as
  ...
  Query<Question,Answer> => {
     let answer = ...
     if let Err(e) = msg.respond( answer).await {
       ...// handle response send error
     }
  }
}
```

In many other actor system libraries this is known as the *ask pattern*.

If the requester message processing should not be blocked (i.e. there are other messages the requester still has to
react to while waiting for a response) the query should be performed from a spawned task. Since the task closure can
capture the query context (e.g. the question) this can still be preferrable to explicit request/response mapping for
one-way messages.

Due to this round trip (and potential per request response channel allocation) queries are less efficient than 
normal message send operations. For repetitive queries from within the same requester there is a `QueryBuilder`
that avoids the response channel allocation for consecutive queries of the same type.


## How to Make Senders Generic - Receivers and Actions

This is the big topic for typed actor communication in (open) actor system frameworks: 

<p class="standout">how to program actors from different domains so that they can still talk to each other?</p>

In other words - how do we make actors in open actor systems reusable in different contexts. This is usually not a
problem if actors are just used in a single application or a single domain (such as a generic web server) - here the set
of actor and message types is closed and known a priori. It becomes a vital problem for a framework such as `odin_actor`
that is meant to be extended by 3rd parties and for various kinds of applications.

This section describes the levels at which we can separate sender and receiver code in `odin_actor`,

The basis for all this is how we can specify the receiver of a particular message within the sender

### (1) `ActorHandle<E>` {.bottom-sep}

`ActorHandle<E>` fields can be used to send messages of any variant of the message set that
is defined by the `define_actor_msg_set` macro: 
```rust
define_actor_msg_set!{ MyMsgSet = Msg1 | Msg2 | ..}
...
impl_actor! { match msg for Actor<MyMsgSet,MyActorState> as 
   Msg1 => ...
   Msg2 => ...
   ...
}
```

This is the least amount of separation between sender and receiver since the sender has to know the full message
interface of the receiver (e.g. `MyMsgSet`), not only the message it wants to send (e.g. `Msg2`). In most cases this is
synonym to knowing the concrete type of the receiver actor, which practically limits this mechanism to very general
receivers or to actors from the same domain (i.e. actors that know about their concrete types anyways).

`ActorHandle<M>` is a `struct` that is  `Clone+Send`, hence it can be sent in messages and stored in fields. 
Cloning `ActorHandle` is inexpensive.


### (2) `MsgReceiver<M>` {.bottom-sep}

`MsgReceiver<M>` can be used to send messages of a single type `M` to the receiver. This is the next level of separation
since now the sender only has to know that the receiver understands `M` - it does not need to know what other messages
the receiver processes. It still requires that both sender and receiver know the same message type `M` though.

`MsgReceiver<M>` is a trait, which means it can only be stored within the sender using either

- a type variable `A: MsgReceiver<SomeMsg>` (the usual case), or
- a trait object: `Box<dyn MsgReceiver<SomeMsg>>` (normally used through `DynMsgReceiver` - see below) 

Respective `MsgReceiver<_>` impls for actor message sets are automatically generated by the `define_actor_msg_set!(..)`
macro, i.e. each `ActorHandle` has impls for all of its message enum variants.


### (3) `MsgAction<M>` and the `define_msg_action!{..}` macro  {.bottom-sep}

`ActorMsgAction<M>` is a trait that has a single `async fn execute(msg:M)`. Corresponding impls get defined
with the `define_msg_action!{..}` macro at the actor system construction site (e.g. in `main()`) and
instances of that action get passed into the sender constructor as an argument:

```rust
   //--- receiver actor module(s)
   define_actor_msg_set! { Receiver1Msg =  Msg1 | ... }
   define_actor_msg_set! { Receiver2Msg =  ... | Msg1 | ... }
   ...
   struct Receiver1 { ... }
   struct Receiver2 { ... }

   //--- sender actor module
   struct Sender<A> where A: MsgAction<Msg1> {
      action: A,
   }
   impl<A> Sender<A> where A: MsgAction<Msg1> {
       ... self.action.execute( Msg1{..}).await ...
   }
   ...
   //--- actor system construction (main)
   let receiver1_handle = spawn_actor!( actor_system, "recv1", Receiver1 {..});
   let receiver2_handle = spawn_actor!( actor_system, "recv2", Receiver2 {..});

   define_msg_action! { MyMsgAction = Msg1 for Receiver1Msg, Receiver2Msg }
   spawn_actor!( actor_system, "sender", Sender::new( MyMsgAction( receiver1_handle, receiver2_handle) ))

```

With `MsgAction<M>` we can roll up a number of compatible receiver `ActorHandle<E>` types into one 
`MsgAction<M>` type without the need for trait objects or boxing. The sender code does not
need to know what actors it sends messages to - this only needs to be specified at the actor system
construction site (e.g. application `main()`) where all actors and message sets are known. 

`MsgAction<M>` instances represent static receiver collections - we cannot dynamically add new receivers at runtime.


### (4) `DynMsgReceiver<M>` and `MsgSubscriptions<M>`

`DynMsgReceiver<M>` overcomes the closed receiver list limitation by means of additional type constraints 
and some runtime cost (trait object allocations and pinning).

`DynMsgReceiver<M>` is a `MsgReceiver<M>` that can be sent in messages. It is normally used indirectly by

- `MsgSubscriptions<E>` as a container to store `Box<dyn MsgReceiver<E>>` instances in the sender
- the `msg_subscriber<E> (s: impl DynMsgReceiver<E>...)` function that creates respective trait objects

This does not add static separation between sender and receiver but it allows to separate lifetimes (receiver
does not need to exist at the point of sender construction).

`MsgSubscriptions<M>` is the primary construct to implement dynamic publish/subscribe message patterns
(see [pub_sub.rs example](examples/pub_sub.md))


All the above mechanisms can only be used to send messages that are created by the sender, which therefore
needs to know (a) the message type and (b) how to instantiate objects of this type. Both are significant
constraints that limit the use to actor types within the same domain or to very general messages, which is
too restrictive for a general actor framework supporting multiple application domains. 


### (5) `DataAction<T>` and the `data_action!{..}` macro

`DataAction<T>`is an abstraction that overcomes the limitation of being able to only sending one message type
and having to hard-code message construction in the sender (which simply might not know the messages understood
by receivers).

`DataAction<T>` is a trait with a generic `async fn execute(t: T)` method that can be used to create arbitrary actor
actions that are passed into the sender constructor. Just as in the `MsgAction<M>` described above we leave definition
of the concrete receivers and send operations to the actor system construction site, but this time the sender actor code does not
need to know what and how to construct message instances - it only needs to know what data to feed into the
`execute(..)` call and when to call it. Actions can be defined explicitly as in:

```rust
   // action definition
   struct MyDataAction {..}
   impl DataAction<SenderData> for MyAction {
       async fn execute (data: &SenderData) { ... } // ⬅︎ concrete action defined here
   }
   
   struct Sender<A> where A: DataAction<SenderData> {
       action: A, ...
   }
   impl<A> Sender<A> where A: DataAction<SenderData> {
       ...
         let data: SenderData = ...; // create the data that should be passed into the action
         self.action.execute( data ).await ...
   }
   
   // actor system construction (main)
   ...  spawn_actor!( actor_system, "sender", Sender::new(MyDataAction{..}))
```

`DataAction<T>::execute(data:T)` is not even limited to sending messages to other actors - it could also
call normal functions. However, since we are focused on actors and hence message passing as the primary interaction
there is a dedicated `data_action!{..}` macro supporting definition and creation of concrete `DataActions` that
can capture receiver actor handles from its call environment:

```rust
  // actor modules
  define_actor_msg_set! { Receiver1Msg = Msg1 | ... }
  define_actor_msg_set! { Receiver2Msg = ... | Msg2 | ... }
  ...
  struct Sender<A> where A: DataAction<SenderData> { 
    action: A, ...
  }
  impl<A> Sender<A> where A: DataAction<SenderData> {
    fn new (action: A)->Self { ... }
  }
  
  // actor system construction site (e.g. main() function)
     receiver1 = spawn_actor!( actor_system, "recv1", Receiver1{..})?;
     receiver2 = spawn_actor!( actor_system, "recv1", Receiver1{..})?;
     ...
     sender = spawn_actor!( actor_system, "sender", Sender::new(
         data_action!( receiver1 as MsgReceiver<Msg1>, receiver2 as MsgReceiver<Msg2> => |data: SenderData| {
            receiver1.send_msg( Msg1::new( ...data.clone().,,)).await?;
            receiver2.try_send_msg( Msg2::new(...data...))
         })
     ))?;
```

The interesting part about the `data_action!(..)` macro is that it captures receiver actor handles (more specifically
the `MsgReceiver<M>` constraints used in the action) from the macro call site without requiring a closure. The general
pattern of the macro call is as follows:
```
data_action!( «captured-receiver-var» as MsgReceiver<M>, ... => |«data-var»: «data-var-type»| «execute-expr»)
```
If we need to send several message types to a receiver we can specify multiple `MsgReceiver<M>` type constraints like so
```
data_action!( r1 as MsgReceiver<Msg1> | MsgReceiver<Msg2>, ...)
```

There is a separate `DataRefAction<T>` with an accompanying `dataref_action!(..)` macro that passes the action data
as a reference into its `execute(&self, dataref: &T)`. This is more suitable in case the action data is directly taken
from a sender field, i.e. would not have to be constructed per `execute(..)` call:

```rust
struct Sender<A> where A: DataRefAction<MyData> {
   data: MyData,
   action: A
   ..
}
impl<A> Sender<A> ... {
   ... self.action.execute( &self.data ).await ...
}
```

If action execution is triggered by messages the action body sometimes needs to pass along some request message
information together with its own data. This is especially the case if the requester needs to associate messages
that are sent back to it from the sender action body with its original request. While this pattern can be implemented
by a dedicated data type in `DataAction` that captures both request and sender data we provide convenience traits
and macros that avoid new data types:

- `LabeledDataAction<L,T>` and its `fn execute(&self, label:L, data:T)` method (created by `labeled_data_action!(..)`)
- `LabeledDataRefAction<L,T>` and its `fn execute(&self, label:L, data: &T)` method (created by `labeled_dataref_action!(..)`)

The label is typically from the triggering request message, but it is still using a type both the sender and the
requester/receiver have to know. If this is too restrictive, or if we need to set actions at runtime, we need the full
power of dynamic action objects.


### (7) `DynDataAction<T>`

`DynDataAction<T>` is an enum that uses trait objects and closures for fully dynamic actions that can be added/removed
at runtime (e.g. through messages). Since this does require allocation and pinning for async actions in each `action.execute(..)`
call we provide two different variants: `AsyncDynDataAction<T>` and the slightly less expensive `SyncDynDataAction<T>`.

Those are rarely used explicitly but instantiated through respective `send_msg_dyn_action!(..)` and `try_send_msg_dyn_action!(..)`
macros as in:

```rust
// receiver actor impl module
struct Msg1 { .. }
...
define_actor_msg_set { ReceiverMsg = Msg1 | ... }
...
impl_actor! { match msg for Actor<ReceiverMsg,Receiver> as
   Msg1 => ...
   ...
}

// sender actor impl module
struct AddUpdateAction(DynDataAction<SenderData>)
...
define_actor_msg_set { SenderMsg = AddUpdateAction | PublishChanges | ... }

struct Sender {
   data: SenderData,
   update_action: DynDataActionList<SenderData> 
   ...
}
impl_actor! { match msg for Actor<SenderMsg,Sender> as
   AddUpdateAction => { ... self.update_action.push( msg.0) ... }
   PublishChanges => { ... self.update_action.execute( &self.data).await ... }
   ...
}
...
// actor system construction module
...
receiver = spawn_actor!( actor_system, "receiver", Receiver::new(..))?;
sender = spawn_actor!( actor_system, "sender", Sender::new(..))?;
...
let action = send_msg_dyn_action!( receiver, |data: &SenderData| Msg1::new(data));
sender.send_msg( AddUpdateAction(action)).await?;
```

To store `DynDataAction` instances in the Sender we use the `DynDataActionList<T>` container.

Note that `DynDataAction` is considerably more expensive than `DataAction`, and especially incurs per-execution cost
for async actions. We therefore recommend to primarily use the static `DataAction` where appropriate.