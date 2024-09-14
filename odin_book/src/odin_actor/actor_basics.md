# The Actor Programming Model

Actors represent a concurrency programmming model that avoids shared memory between concurrent (and possibly parallel)
executions by means of message passing. Since its introduction in [^Hewitt73] it has been the subject of extensive
formalization but the essence is that

<p class="standout">actors are concurrently executing objects that only communicate through messages, without sharing state</p>

In addition, existing actor implementations also ensure that messages received by an actor are processed sequentially,
which basically allows to treat an actor implementation as sequential code. This significantly reduces the concurrency
related complexity of systems that use actors as their primary building blocks.

The Actor programming model as used by `odin_actor` revolves around five components:

<img class="mono right" src="./img/actors-mono.svg" width="35%"/>

 - an **ActorSystem** that instantiates and manages actors
 - the **Actors** themselves as the units of concurrent execution
 - **ActorHandles** as the public-facing actor component
 - **actor mailboxes** that represent the (internal) message queues of actors
 - **actor state** as the mutable, internal memory of an actor
 - a **receive(msg)** function that defines how the actor processes received messages  

**ActorSystems** instantiate and monitor actors. In concrete implementations they include some scheduler that picks
runnable actors (with pending messages) and maps them to kernel threads. They can also be used to manage global
resources (such as job schedulers) and perform actor synchronization (i.e. implement `ActorSystem` specific actor
state models) 

**Actors** are the concurrently executing entities. An actor aggregates a usually invisible **`mailbox`** (message queue),
an **actor state** that holds the (mutable) actor-specific data and a **`receive(msg)`** message handler function that can 
in response to received messages

- mutate the actor state
- create other actors
- send messages to other actors

It should be noted that **Actors** are an abstract concept - concrete ActorSystem implementations have considerable
leeway to implement them. They can use message queues and actor state outside of physical actor objects. Actors
can even be implemented as "behavior" function objects that pass in the state as a message handler argument, and use the
message handler return value to set the next behavior and/or state.  

**ActorHandles** represent the visible reference of an actor towards other actors and the actor system. The role of
an **ActorHandle** is to allow sending messages to the associated actor without exposing its internal state or directly 
affecting its lifetime.

The original actor programming model is abstract. It does not concern itself with implementation specifics such as
type safety, e.g. to statically check that we can only send messages to actors that can handle them. However, those
programming language specific aspects can have a profound impact on genericity and safety of actor system frameworks 
(e.g. to ensure that we do not leak actor state through references passed in messages). 

Concrete implementations should also specify 

- mailbox/send semantics (unbounded -> non-blocking send, bounded -> blocking when receiver queue is full)
- message processing order (e.g. *sequential-per-sender*)

Especially the first topic is relevant to address potential *back-pressure* in actor systems (slow receivers blocking
fast senders). 

[^Hewitt73]	: Carl Hewitt; Peter Bishop; Richard Steiger (1973). "A Universal Modular Actor Formalism for Artificial Intelligence". IJCAI.
