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

    pub trait SharedStoreValueConstraints = Clone + Send + Sync + Debug + 'static + for<'a> Deserialize<'a> + Serialize;
```

This resemblance is intentional - our general use case is a in-memory database of relatively few (<1000>) items, for which a
`std::collections::HashMap` is a valid choice. Apart from normal item getters/setters the primary operation is to iterate over store items.
Out of the box `odin_share` therefoer includes a `SharedStore` impl for `std::collections::HashMap`.

Persistency is supported by providing a `PersistentHashMapStore` struct that encapsulates a `HashMap` which is initialized from and
stored to a JSON file.

The abstraction should also support larger data sets that require disk storage, caches and query mechanisms. Since our data model
is simple we constrain queries to [glob pattern searches](https://en.wikipedia.org/wiki/Glob_(programming)) which are supported by the specialized `glob_.._iter()` iterators.

`SharedStoreValueConstraints` reflects the need to serialize/deserialize store content, send items as messages and to use store trait objects
from async code.

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

While there is no reason `SharedStoreActors` cannot be used by any other actor the most common use is as a storage backend for a
[`SpaServer` actor](../odin_server/odin_server.md). To simplify spawning the `SharedStoreActor` (and explicitly setting up respective
init and change actions) we therefore provide a `odin_share::spawn_server_share_actor(..)` method that can be use like so:

```rust
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_share::prelude::*;

run_actor_system!( actor_system => {
    let pre_server = PreActorHandle::new( &actor_system, "server", 64);

    let hstore = spawn_server_share_actor(&mut actor_system, "share", pre_server.to_actor_handle(), &"examples/shared_items.json", false)?;

    let hserver = spawn_pre_actor!( actor_system, pre_server, SpaServer::new(
        ...
        SpaServiceList::new()
            ...
            .add( build_service!( let hstore = hstore.clone() => ShareService::new( hstore)) )
    ))?;
    Ok(())
});
```

## Abstract Sharing Model

TODO - explain SharedItem (pathname keys), owner, role, subscription


## Client-side `SharedStore` sharing via `ShareService`

While the previous section was about how to use `SharedStore` between components (actors) *within* an ODIN server, we
also use this mechanism to share interactively entered data between *users* of such an ODIN server. Technically this means
we need to provide a [`odin_server::SpaService`](../odin_server/odin_server.md) implementation that updates store values
through incoming websocket message handlers and distributes the store changes to other users through outgoing websocket
messages, which are then distributed on the client side to respective `SpaService` Javascript modules. This is the
purpose of `ShareService` and its associated `odin_share.js` Javascript module asset, which in turn depends on and
extends the general `main.js` module provided by [`odin_server`](../odin_server/client.md).

In addition to the sharing data values `SharedStore` also supports synchronizing operations between users. It is important 
to note this is not unidirectional like screen sharing in video conferencing but allows to perform certain actions such as
view selection remotely. Although such actions are confined to the browser sandbox this is of course security relevant and hence
only takes place if the user explicitly allows it and specifies from whom such shared commands are accepted. This is based
on the *role* concept - if not already taken a user can identify/register for a role and then - separately - choose to 
publish under that role. Other users will automatically see new roles (with their publishing status) and then - through
opt-in - subscribe to certain roles, at which point they will receive published commands from that role. A user can
register for multiple roles.

It is up to the JS modules of respective `SpaServices` which sync commands they support, both incoming through 

```javascript
import * as main from "../odin_server/main.js";
...
main.addSyncHandler( handleSyncMessage);
...
function handleSyncMessage (msg) {
    if (msg.updateCamera) {...} // example: reaction to remote view changes (if subscribed)
    ...
}
```

or outgoing through 
```javascript
   ... 
   main.publishCmd( { updateCamera: {lon, lat, alt} });  // example: execute view change remotely (if publishing)
   ...
```

The general API for synchronized operations is provided by [`main.js](../odin_server/client.md) and consists of the
following functions:

- `requestRole (role)` - try to obtain a role (will fail if already registered on the server)
- `releaseRole (role)` - the converse (will fail if this is not an own role)
- `publishRole (role, isPublishing)` - start/stop publishing under specified role 
- `subscribeToExtRole (role, isSubscribed)` - start/end subscription to an external role
- `publishCmd (cmd)` - send command to other users if there is a publishing own role and at least one remote subscriber

Only `publishCmd` is used by general JS modules, all other functions are just for speciality modules such as `odin_share.js`
that implement remote sharing (normally through a dedicated UI).