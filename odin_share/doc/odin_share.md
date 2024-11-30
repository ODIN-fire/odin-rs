# odin_share

The `odin_share` system crate provides the means to share string labeled data between [actors](../odin_actor/odin_actor.md) and
[micro services](../odin_server/odin_server.md). It therefore is also the basis for sharing interactively created data between users of
the same ODIN server.

The data model on which `odin_server` is built is a homogenous, statically typed key value store. Keys are path-like strings and the value type is the generic type parameter of the store. The basic abstraction is an object-safe `SharedStore<T>` trait that resembles the
interface of a standard `HashMap`:

```rust
    pub trait SharedStore<T> : Send + Sync where T: SharedStoreValueConstraints {
        fn ref_iter<'a>(&'a self)->Box<dyn Iterator<Item=(&'a String,&'a T)> + 'a>;
        fn clone_iter(&self)->Box<dyn Iterator<Item=(String,T)> + '_>;

        fn insert(&mut self, k: String, v: T)->Option<T>;
        fn remove (&mut self, k: &str)->Option<T>;
        fn get (&self, k: &str)->Option<&T>;

        fn glob_ref_iter<'a>(&'a self, glob_pattern: &str)->Result<Box<dyn Iterator<Item=(&'a String,&'a T)> + 'a>, OdinShareError>;
        fn glob_clone_iter(&self, glob_pattern: &str)->Result<Box<dyn Iterator<Item=(String,T)> + '_>, OdinShareError>;
        ...
    }
```

This resemblance is intentional - our general use case is a in-memory database of relatively few (<1000>) items, for which a
`std::collections::HashMap` is a valid choice. Apart from normal item getters/setters the primary operation is to iterate over store items.
Out of the box `odin_share` therefoer includes a `SharedStore` impl for `std::collections::HashMap`.

Persistency is supported by providing a `PersistentHashMapStore` struct that encapsulates a `HashMap` which is initialized from and
stored to a JSON file.

The abstraction should also support larger data sets that require disk storage, caches and query mechanisms. Since our data model
is simple we constrain queries to [glob pattern searches](https://en.wikipedia.org/wiki/Glob_(programming)) which are supported by the specialized `glob_.._iter()` iterators.


## Server-side `SharedStore` sharing via `SharedStoreActor`

How do we use a `SharedStore` within an ODIN server? While `SharedStore` implementors could be global they are inherently mutable
and hence a global store would require locking patterns such as:

```rust
    use std::sync::{LazyLock,RwLock};
    ...
    static MY_STORE: LazyLock<RwLock<MyStore>> = ...
```

Moreover, since the shared-ness is a critical aspect we also require a notification mechanism that allows callbacks on mutable clients
once a store changes, which makes the use of global objects quite unwieldy (especially in an `async` context). Hence our primary
use of `SharedStore` instances is the [actor](../odin_actor/odin_actor.md):

```rust
pub struct SharedStoreActor<T,S,A> where T: SharedStoreValueConstraints, S: SharedStore<T>, A: DataAction<SharedStoreChange<T>> {
    store: S,
    change_action: A,
    ...
}

define_actor_msg_set! { pub SharedStoreActorMsg<T> where T: SharedStoreValueConstraints = 
    SetSharedStoreValue<T> | RemoveSharedStoreValue | Query<String,Option<T>> | ExecSnapshotAction<T>
}
```

To make the `SharedStoreActor` type re-usable across different application domains we parameterize it not only with the store item type `T`, the store type `S` but also with a generic [odin action](../odin_action/odin_action.md) type `A` of a actor constructor parameter
which defines the "callback" actions to be performed when the store changes. Upon store mutation this `odin_action::DataAction` is
executed with a 

```rust
pub enum SharedStoreChange<T> where T: SharedStoreValueConstraints {
    Set { hstore: ActorHandle<SharedStoreActorMsg<T>>, key: String },
    Remove { hstore: ActorHandle<SharedStoreActorMsg<T>>, key: String },
    ...
}
```

parameter which can be sent to other actors. Recipients of such `SharedStoreChange` messages can then use its `hstore` actor handle
to query the changed store values by sending a `Query<String,Option<T>>` query message to the store actor, or by sending a

```rust
    struct ExecSnapshotAction<T>( pub DynSharedStoreAction<T> )
```

message to the store with an action trait object that will be executed by the store with its own `SharedStore<T>` trait object parameter.
These patterns look like so in a client actor using the store:

```rust
use odin_actor::prelude::*;
use odin_share::prelude::*;

enum StoreItem {...}

struct Client {...}

#[derive(Debug)] struct SomeClientMsg (ActorHandle<SharedStoreActorMsg<StoreItem>>);
define_actor_msg_set! { ClientMsg = SharedStoreChange<StoreItem> | SomeClientMsg }

impl_actor! { match msg for Actor<Client,ClientMsg> as
    SharedStoreChange<StoreItem> => cont! { // store has changed, query value
        match msg {
            SharedStoreChange::Set{ hstore, key } => {
                println!("client received update for key: {:?}, now querying value..", key);
                match timeout_query_ref( &hstore, key, secs(1)).await {
                    Ok(response) => match response {
                        Some( value ) => ...
                    }
                }
            }
            ...
        }
    }
    ...
    SomeClientMsg => cont! { // iterate over store items
        let action = dyn_shared_store_action!( => |store as &dyn SharedStore<StoreItem>| {
            for (k,v) in store.ref_iter() {
                ... // process key-value items
            }
            Ok(())
        });
        msg.0.send_msg( ExecSnapshotAction(action)).await;
    }
}   
```

The actor system construction in turn uses the change action to register clients in the store:

```rust
use odin_actor::prelude::*;
use odin_share::prelude::*;

run_actor_system!( asys => {
    let client = PreActorHandle::new( &asys, "client", 8); 

    let hstore = spawn_actor!( asys, "store", SharedStoreActor::new(
        HashMap::new(),
        data_action!( let client: ActorHandle<ClientMsg> = client.to_actor_handle() => 
            |update: SharedStoreChange<StoreItem>| Ok( client.try_send_msg( update)? )
        )
    ))?;
    ...
    let client = spawn_pre_actor!( asys, client, Client::new(...))?;
    ...
    Ok(())
}
```

See the `enum_store.rs` example for further details.


## Client-side `SharedStore` sharing via `ShareService`

While the previous section was about how to use `SharedStore` *within* an ODIN server application, our primary goal is to
provide a mechanism to share interactively entered data between users of such an ODIN server. Technically this means we
need to provide a [`odin_server::SpaService`](../odin_server/odin_server.md) implementation that updates store values through
incoming websocket message handlers and distributes the store changes to other users through outgoing websocket messages, which
are then distributed on the client side to respective `SpaService` Javascript modules. This is the purpose of `ShareService` and
its associated `share.js` Javascript module asset.

TBD