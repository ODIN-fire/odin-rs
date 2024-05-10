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
variant. 

<a id="message-size"></a>
Since this message set enum is the type of the actor mailbox (channel) this size matters - a Rust `enum` is sized according to
its largest variant. If the ratio of max to min size of variants is too large then the channel can waste a lot of memory. If this is
a problem we can always wrap (part of) large messages within heap-allocated containers (`Box`, `Arc`, `Vec` etc.) which collapses
the size of the wrapped data to a pointer.

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

<p class="standout">how to connect actors from different domains that do not know about each other?</p>

In other words - how do we make actors in open actor systems reusable in different contexts. This is not a
problem if actors are just used in a single application or a single domain (such as a generic web server) - here the set
of actor and message types is closed and known a priori. It becomes a vital problem for a framework such as `odin_actor`
that is meant to be extended by 3rd parties and for various kinds of applications.

This section describes the levels at which we can separate sender and receiver code in `odin_actor`,

The basis for all this is how we can specify the receiver of a particular message within the sender

### (1) `ActorHandle<M>` {.bottom-sep}

`ActorHandle<M>` fields can be used to send messages of any variant of the message set that is defined by the 
`define_actor_msg_set` macro: 
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

`ActorHandle<M>` is a `struct` that is `Clone` + `Send`, hence it can be sent in messages and stored in fields. 
Cloning `ActorHandle` is inexpensive.


### (2) `MsgReceiver<T>` and `MsgReceiverList<T>` {.bottom-sep}

`MsgReceiver<T>` can be used to send messages of a single type `T` to the receiver (if `T` is in the receiving actors
message set - see above). This is the next level of separation since now the sender only has to know that the receiver
understands `T` - it does not need to know what other messages the receiver processes.

`ActorHandle<M>` has a blanket impl for `MsgReceiver<T>` for all variants of its message set `M`. 

`MsgReceiver<T>` is a trait, which means it can only be stored within the sender using a type variable 
```rust
struct MySender<R> where R: MsgReceiver<SomeMsg> {
   receiver: R, ...
}
```

To support heterogenous lists of `MsgReceiver<T>` implementors we provide a `MsgReceiverList<T>` trait together with
a `msg_receiver_list!(..)` macro that can be used like so:
```rust
   //--- receiver actor module(s)
   define_actor_msg_set! { Receiver1Msg =  Msg1 | ... }
   define_actor_msg_set! { Receiver2Msg =  ... | Msg1 | ... }
   ...
   struct Receiver1 { ... }
   struct Receiver2 { ... }

   //--- sender actor module
   struct MySender<L> where L: MsgReceiverList<Msg1> {
      receivers: L, ...
   }
   impl<L> MySender<L> where L: MsgReceiverList<Msg1> {
      ... self.receivers.send_msg( Msg1{...}, true).await ...
   }

   //--- actor system construction (main)
   let receiver1_handle = spawn_actor!( actor_system, "recv1", Receiver1 {..});
   let receiver2_handle = spawn_actor!( actor_system, "recv2", Receiver2 {..});

   spawn_actor!( actor_system, "sender", 
                 Sender::new( msg_receiver_list!( receiver1_handle, receiver2_handle : MsgReceiver<Msg1>) ))
```

`MsgReceiverList<T>` has the usual send functions but adds a `ignore_err: bool` argument to each of them, defining
if the send operation for the list should ignore error results for its elements. If set to false, the first element
send operation that fails shortcuts the list send operation. 

`MsgReceiver<T>` and `MsgReceiverList<T>` represent static receiver types - with them we cannot dynamically add 
new receivers at runtime.


### (3) `DynMsgReceiver<T>` and `DynMsgReceiverList<T>` {.bottom-sep}

`DynMsgReceiver<T>` is a type that allows us to send and store `MsgReceiver<T>` implementors as trait objects at
runtime. It is boxing a normally transparent `DynMsgReceiverTrait<T>` for which `ActorHandle<M>` has blanket impls. 
It is less efficient than the static `MsgReceiver<T>` since it incurs extra runtime cost for each send
operation (pin-boxing the futures returned by its send operations).

`DynMsgReceiverList<T>` is a container for `DynMsgReceiver<T>` objects. It is used like this:

```rust
   //--- receiver actor module(s)
   define_actor_msg_set! { Receiver1Msg =  Msg1 | ... }
   
   struct Receiver1<S> where S: MsgReceiver<AddMsg1Receiver> { sender: S... }

   impl_actor! { match msg for Actor<Receiver1<S>,Receiver1Msg> where S: MsgReceiver<AddMsg1Receiver> as
      ... self.sender.send_msg( AddMsg1Receiver(self.hself.into())).await ...
      Msg1 => ...
   }

   define_actor_msg_set! { Receiver2Msg =  ... | Msg1 | ... }
   struct Receiver2<S> where S: MsgReceiver<AddMsg1Receiver> { sender: S... }
   ...

   //--- sender actor module
   #[derive(Debug)]
   struct AddMsg1Receiver(DynMsg1Receiver<Msg1>);

   define_actor_msg_set! { MySenderMsg = AddMsg1Receiver | ...}

   struct MySender  {
      receivers: DynMsgReceiverList<Msg1>, ...
   }

   impl_actor! { match msg for Actor<MySender,MySenderMsg> as 
      AddMsg1Receiver => cont! { self.receivers.push(msg.0) }
      ...
      ... self.receivers.send_msg( Msg1{..}, true).await ...
   }

   //--- actor system construction (main)
   let sender = spawn_actor!( actor_system, "sender", MySender {..});
   spawn_actor!( actor_system, "recv1", Receiver1{sender, ...});
   spawn_actor!( actor_system, "recv2", Receiver2{sender, ...});
```

`MsgReceiverList<T>` and `DynMsgReceiverList<T>` are used to implement static/dynamic publish/subscribe patterns.
They allow us to abstract concrete receiver types our sender can communicate with, provided all these
receivers have the message type we send in their message set.

The limitations are that both sender and receivers have to know the respective message type, and the sender has
to know how to instantiate that message. This is a serious constraint for multi-domain frameworks.


### (4) `DataAction<T>` and the `data_action!{..}` macro   {.bottom-sep}

`DataAction<T>`is an abstraction that overcomes the limitation of being able to send only one message type
and having to hard-code message construction in the sender actor (which might not know the messages understood
by potential receivers).

Data actions are defined and documented in the [`odin_action`] crate - while the **action** construct is not
actor specific it is most useful to make actors from different domains inter-operable. They can be viewed as
async "callbacks" that allow the sender to inject its own data into action executions. All the sender actor
has to know is when to execute an action and what data to provide for its execution.

Actions can be defined explicitly as in:

```rust
   // sender actor definition 
   struct Sender<A> where A: DataAction<SenderData> {
       action: A, ...
   }
   impl<A> Sender<A> where A: DataAction<SenderData> {
       ...
         let data: SenderData = ...; // create the data that should be passed into the action
         self.action.execute( data ).await ...
   }
   ...
   // action definition (at the actor system construction site, e.g. main())
   struct MyDataAction {..}
   impl DataAction<SenderData> for MyAction {
       async fn execute (data: &SenderData)->Result<()> { ... } // ⬅︎ concrete action defined here
   }

   ...  Sender::new( MyDataAction{..}, ...)
```

More often actions are one-of-a-kind objects that are defined and instantiated through the macros that are
provided by [`odin_action`], and their action expressions are sending messages to other actors:

```rust
  // actor modules
  define_actor_msg_set! { Receiver1Msg = Msg1 | ... }
  define_actor_msg_set! { Receiver2Msg = ... | Msg2 | ... }
  ...
  struct Sender<A> where A: DataRefAction<SenderData> { 
     data: SenderData,
     action: A, ...
  }
  impl<A> Sender<A> where A: DataRefAction<SenderData> {
    fn new (action: A)->Self { ... }

    ... self.action.execute(&self.data).await ...
  }
  
  // actor system construction site (e.g. main() function)
     receiver1 = spawn_actor!( actor_system, "recv1", Receiver1{..})?;
     receiver2 = spawn_actor!( actor_system, "recv1", Receiver1{..})?;
     ...
     sender = spawn_actor!( actor_system, "sender", Sender::new(
         dataref_action!( receiver1: ActorHandle<Receiver1Msg>, receiver2: ActorHandle<Receiver2Msg> => |data: &SenderData| {
            receiver1.send_msg( Msg1::new( ...data.clone().,,)).await?;
            receiver2.try_send_msg( Msg2::new(...data.translate() ...))
         })
     ))?;
```

The interesting aspect about the `data_action!(..)` macros is that they can capture data from the macro call site without
requiring a closure (Rust does not yet support async closures). The general pattern of the macro call is as follows:
```
data_action!( «captured-receiver-var» :  «capture-type», ... => |«data-var»: «data-var-type»| «execute-expr»)
```

While data actions effectively separate sender and receiver code there is one last constraint: data actions have to be created
upfront, at system construction time. We cannot send them to actors.


### (6) `DynDataAction<T>` and the `dyn_data_action!{..}` macro   {.bottom-sep}

The [`odin_action`] crate also supports dynamic (trait object) actions through its [`dyn_data_action`] and [`dyn_dataref_action`]
macros, which does allow to send actions in messages. This is in turn useful to

- execute such actions when the receiver processes the containing message
- store actions for later execution (e.g. in a subscriber list)

To store action trait objects and execute their entries [`odin_action`] provides the [`DynDataActionList`] and 
[`DynDataRefActionList`] containers:

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
struct AddUpdateAction(DynDataRefAction<SenderData>)
...
define_actor_msg_set { SenderMsg = AddUpdateAction | PublishChanges | ... }

struct Sender {
   data: SenderData,
   update_action: DynDataRefActionList<SenderData> 
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

Actions sent in messages can also be executed when the receiver processes such messages. Since dyn actions can capture
data from the creation site (within the sender code) this can be useful as a less expensive alternative to the `query()`
mechanism described above (only using the normal actor task context).

With power comes responsibility - being able to use loops within action bodies we have to be aware of two potential 
problems:

- back pressure and
- loss-of-information

The back pressure problem arises if we send messages from within iteration cycles, as in:

```rust
... dataref_action( ... |data: &Vec<SomeData>| {...
      for e in data {
         ... receiver.try_send_msg( SomeMessage::new(e)); ...
      }
    }) ...
```

This can result in `OdinActorError::ReceiverFull` results when sending messages. If we use `try_send_msg(..)` without
processing the return value (as in above snippet) this might even be silently ignored. The solution for this is to
either check the return value or use 

```rust
         ... receiver.send_msg( SomeMessage::new(e)).await ...
```

In this case we have to be aware though that the sender might get blocked, i.e. becomes un-responsive if it is also
a potential message receiver. Should this apply we can run the loop from within a spawned task.

There also might be a (semantic) loss-of-information problem if we need to preserve that all messages sent from within
the loop came from the same input data (the `execute()` argument). Unless receivers could easily reconstruct this from
the respective message payload the solution is to collect the payloads into a container and send that container as one
message, which turns the above case into:

```rust
... dataref_action( ... |data: &Vec<SomeData>| {...
      let msg_payload: Vec<SomePayload> = data.iter().map(|e| payload(e)).collect();
      receiver.try_send_msg( SomeMessage::new( msg_payload)) ...
    }) ...
```

This also addresses the message variant size problem mentioned (above)[#message-size].