# odin_server

The `odin_server` system crate provides the infrastructure to create servers. The primary server type is the 
`SpaServer` which implements a [*Single Page Application*](https://en.wikipedia.org/wiki/Single-page_application) 
web server with composable `SpaService` ([*micro service*](https://en.wikipedia.org/wiki/Microservices)) stacks.
The crate it based on the [Axum](https://docs.rs/axum/latest/axum/) framework and hence seamlessly integrates with
the [Tokio](https://docs.rs/tokio/latest/tokio/index.html) async runtime that is also used for our actor system
implementation in the [odin_actor](../odin_actor/odin_actor.md) crate.

The general use case for our servers is to support soft-realtime updates of sensor and tracking data down to 1Hz
latency. To achieve this we push data updates over websockets to all connected clients and hence assume a limited
number of simultaneous users (< 1000). 

The primary constructs of `odin_server` are

- the `SpaServer` actor
- the `SpaService` trait

There is one `SpaServer` actor and an open number of `SpaService` trait implementations, hence the latter is the main
abstraction of `odin_server`. `SpaService` instances often act as display layers for a multitude of dynamic data types
such as tracked objects, weather info and satellite observations.  

A `SpaService` has two main purposes:

- provide the components that are served via http. 
  The main resource component of a `SpaService` is usually a Javascript module that contains the client-side code to communicate with the server and to display data received from it. Those *assets* make full use of the [odin_build](../odin_build/odin_build.md) crate, i.e.
  they can be inlined (for stand-alone servers) or looked up in a number of file system locations
- trigger the initial data download via websocket when a new client (browser) connects.

Dynamic data such as tracked objects normally comes from a separate *DataActor* that is associated with a `SpaService`. Although this is a role (not a type) it has a common message interface to `SpaServer`:

- announce availability of data
- provide a current data snapshot to be sent to new clients (connected browsers)
- provide data updates to be sent to all clients when the internal state changes

```
                     ┌────────────────────────┐                      
                     │      SpaServer         │                assets
                     │                        │      ┌─────────────┐ 
                     │  ┌──────────────────┐  │   ┌──┤  js-module  │ 
                     │  │  SpaServiceList  │  │   │  └─────────────┘ 
                     │  │                  │  │   │  ┌─────────────┐ 
┌───────────┐        │  │ ┏━━━━━━━━━━━━┓◄──┼──┼───┼──┤     ...     │ 
│ DataActor ├─┐ ◄────┼──┼─┃ SpaService ┃─┐ │  │   │  └─────────────┘ 
└─┬─────────┘ │      │  │ ┗━┯━━━━━━━━━━┛ │ │  │   │                  
  └─────┬─────┘      │  │   └────────────┘ │  │   │           proxies
        │            │  └──────────────────┘  │   └──── name | url   
        │            │                        │         ...  | ...   
        │            │      connections       │                      
        │  init      │  ┌──────────────────┐  │                      
        └───────────►│  │ip-addr  websocket│  │                      
           update    │  │  ...      ...    │  │                      
                     │  └──────────────────┘  │                      
                     │                        │                      
                     └────┬─────────────▲─────┘                      
                          │             │                            
                 - - - - -│- - - - - - -│- - - - - -                 
                   http://│             │wss://                      
                          ▼   clients   ▼                                                
```


The `SpaServer` actor encapsulates two pieces of information:

- the static `SpaServiceList` that contains an ordered sequence of `SpaService` trait objects for the web application.
  This list is provided as a `SpaServer` contstructor parameter (e.g. created in `main()`) but uses its own type since `SpaService` instances can depend on other SpaServices.
- the dynamic list of client connections (client IP address and associated websocket)

`SpaServer` has an internal and external message interface. The internal interface is used to update the connection list (which
is not shared with the `SpaServices`). The external interface includes two generic message types sent by *DataActors*:

- `SendWsMsg(ip_addr,data)` to send data snapshots to a new connection (address provided in the message)
- `BroadcastMsg(data)` to broadcast data updates to all current connections

We use [JSON](https://www.json.org/json-en.html) for all websocket communications.

`SpaServer`, `SpaService` and *DataActor* implementations do not need to know each other, they can reside in different crates and
even domains (system or application). This is mostly achieved through `SpaService` trait objects and 
[`odin_action` data actions](../odin_action/odin_action.md) which are set in the only code that needs to know the concrete types - the
actor system instantiation site (e.g. `main()`).

Each web application actor system is implemented as a single executable. In general, development of new web applications therefore
involves two steps:

1. creating *DataActor* and associated `SpaService` implementations for new data sources
2. writing code that instantiates the required actors and connects them through *data actions* 
   (see [odin_action](../odin_action/odin_action.md))


## 1. Creating `SpaService` Implementations

As a `SpaService` has the two main functions of (1) initializing the server and then (2) initializing clients through their
websockets. We look at these steps in sequence.

### 1.1 Initializing the Server

`SpaService` objects are `SpaServer` constructor arguments. They have to be created first but instead of passing them directly
into the `SpaServer` constructor we use a `SpaServiceList` accumulator to do so. The rationale is that `SpaServices` can depend
on each other, e.g. a track service depending on the framework provided websocket and virtual globe rendering services. 
`SpaServiceList` is used to make sure only one service of each type name is included. It is initialized like so:

```rust
SpaServiceList::new()
    .add( build_service!( GoesrService::new(...)) )
    ...
```

The `odin_server::build_service!(expr)` macro is just syntactic sugar that wraps the provided expr into a closure to defer the actual creation of the service until we know its typename has not been seen yet. The `SpaServiceList::add()` funtion then calls the `SpaService::add_dependencies(..)` implementation, which can recursively repeat the process:

```rust
    fn add_dependencies (&self, svc_list: SpaServiceList) -> SpaServiceList {
        svc_list
            .add( build_service!( UiService::new()))
            .add( build_service!( WsService::new()))
    }
```

While `SpaServiceList` is used to accumulate the required `SpaService` instances it is not used to store them in the `SpaServer`.
Instead, we extract these instances and wrap them as trait objects in an internal `SpaSvc` type that allows us to add some
service specific state.  Once a `SpaServer` actor receives a `_Start_` system message it begins to assemble the served document by traversing the stored `SpaService` trait objects.

There are two component types each `SpaService` can add:

- document fragments (HTML elements such as scripts)
- routes (HTML GET/POST handlers that respond to service specific asset requests)

Again, the `SpaServer` does not add such components directly to the generated HTML document and Axum handlers but accumulates them in a 
`SpaComponents` struct that can filter out redundant components (e.g. external script references). The `SpaComponets` type is
essentially our single page document model that includes:

- header items (CSS links, external script links, `odin_server` Javascript modules)
- body fragments (HTML elements)
- routes (the HTML URIs we serve)
- proxies (a map of symbolic external server names to their respective base URIs)
- assets (a map from symbolic asset filenames to `SpaService` crate specific `load_asset()` lookup functions)

`SpaComponents` includes methods to add each of those components from within `SpaService::add_components(..)` implementations like so:

```rust
    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()>  {
        spa.add_assets( self_crate!(), load_asset);
        spa.add_module( asset_uri!("odin_sentinel_config.js"));
        spa.add_module( asset_uri!("odin_sentinel.js"));

        spa.add_route( |router, spa_server_state| {
            router.route( &format!("/{}/sentinel-image/*unmatched", spa_server_state.name.as_str()), get(Self::image_handler))
        });

        Ok(())
    }
```

`SpaService` implementor crates use the [odin_build](../odin_build/odin_build.md) crate to generate respective `load_asset(..)` functions
from their `lib.rs` modules like so:

```rust
use odin_build::define_load_asset;
...
define_load_asset!{}
...
```

Although our own `SpaService` specific Javascript modules are looked up/served through this `load_asset()` function we have to
add them explicitly through calling `add_asset(..)` since our document model supports post-initialization hooks that are automatically
called at the end of the BODY element and we have to ensure that all (possibly asynchronous) Javascript modules are initialized
at this point.

`SpaService` implementations only have to add the components they need. Once all services have added their components the `SpaServer`
calls the `SpaComponents::to_html(..)` function to generate the served document and generates required Axum 
[routers](https://docs.rs/axum/latest/axum/index.html#routing) with their respective 
[handler](https://docs.rs/axum/latest/axum/index.html#handlers) functions. 

```                                                                                         
                                               ┌──────────────┐                           
                                               │ OtherService │                   configs 
                                               └──────┬───────┘         ┌──────────────┐  
                  ┌──────────────┐                    │       ┌─────────┤my_service.ron│  
                  │SpaServiceList│                    │       │   init  └──────────────┘  
                  └──┬──▲────────┘                    │       │                           
                     │  │   ┌─────────────────────────┼───────▼──┐                 assets 
 _Start_             │  │   │ MyService : SpaService  │          │       ┌─────────────┐  
                     │  │   │                         ▼          │    ┌──┤my_service.js│  
    start()_server() │  └───┼─ add_dependencies(svc_list)        │    │  └─────────────┘  
                     ▼      │                                    │    │  ┌─────────────┐  
           build_router()  ◄┼─ add_components(spa_components)◄───┼────┼──┤     ...     │  
                     │      │                         │          │    │  └─────────────┘  
                     │      │  ...                    │          │    │                   
                     │      └─────────────────────────│──────────┘    │           proxies 
                     ▼                                ▼               └──── name | url    
               doc_handler()  ◄────────────────── document                  ...  | ...    
                                                                                          
               asset_handler()                                                            
                                                                                          
               proxy_handler()                                                            
                     │                                                                    
                     ▼                                                                    
                 http://                                                                  
```

At this point `SpaServer::start_server()` spawns the Axum `TcpListener` task and we are ready to serve client requests.


### 1.2 Initializing and Updating Clients

Most application domain `SpaService` implementations involve dynamic data that needs to be pushed to connected browsers. That
data typically does not get generated by the `SpaService` itself but by some dedicated *DataActor* that is only concerned about
maintaining that data, not about distributing or rendering it. To make this available in a web server context we use
interaction between the respective `SpaService`, its *DataActor* and the `SpaServer`.

There are two types of interaction

- initialization of new clients
- update of all connected clients

Since we need to push data both work by sending JSON messages over the websocket associated with a client.

New connections are deteced by a request for the websocket URI that is handled by the framework provided `WsService` (which
is a dependency for all dynamic data services). Once the protocol upgrade (http -> ws) is accepted the `WsService` handler sends
an internal `AddConnection` message to the `SpaServer` which in response stores the new remote address and websocket in its 
connection list and then calls the `init_connection(..)` method of all its `SpaServices`. 

The `SpaService::init_connection(..)` implementations then send a message to their *DataActor* that contains a 
[`odin_action::DynDataRefAction`](odin_action/odin_action.md) object which captures both the handle of the `SpaServer` actor
and the ip address of the new connection. When the *DataActor* processes that message it executes the `DynDataRefAction`
passing in a reference to its internal data. The action body itself generates a JSON message from the data reference and 
sends it as a `SendWsMsg` message to the `SpaServer` actor, which then uses the remote ip address of the message to look up
the corresponding websocket in its connection list and then sends the JSON message payload over it.

```
                                   ┌────────────────────────────────────────────────┐
                                   │                  SpaServer                     │
                                   │                                                │
                                   │  ┌─────────────────────────────────────┐       │
                                   │  │ MyService : SpaService              │       │
┌─────────────────┐                │  │                                     │       │
│    DataActor    │                │  │  ...                                │       │
│                 │ DataAvailable  │  │                                     │       │
│ [init_action]  ─┼────────────────┼──┼► data_available(hself,has_conn,..)  │       │
│                 │                │  │                                     │       │
│ exec( action) ◄─┼────────────────┼──┼─ init_connection(hself,has_data,..)◄─────────────┐
│        │        │                │  │                                     ├────┐  │    │AddConnection
│        │        │                │  └─────────────────────────┬───────────┘    │  │    │
│        │        │   SendWsMsg    │                            │     WsService  ────────┘ 
│        └────────┼────────────────┼──► send_ws_msg()           └────────────────┘  │
│                 │                │                              ┌───────────┐     │
│ [update_action]─┼────────────────┼──► broadcast_ws_msg() ◄──────┤connections│     │
│                 │ BroadcastWsMsg │         │                    └───────────┘     │
└─────────────────┘                │         │                                      │
                                   └─────────┼──────────────────────────────────────┘
                                             │                                    
                                             │                                    
                                             ▼                                    
                                           wss://                                 
```

A typical `SpaService::init_connection(..)` implementation looks like this:

```rust
async fn init_connection (&mut self, hself: &ActorHandle<SpaServerMsg>, is_data_available: bool, conn: &mut SpaConnection) 
  -> OdinServerResult<()> {
        ...
        if is_data_available {
            let action = dyn_dataref_action!( hself.clone(): ActorHandle<SpaServerMsg>, remote_addr: SocketAddr => |data: &MyData| {
                let data = ws_msg!( JS_MOD_PATH, data).to_json()?;
                let remote_addr = remote_addr.clone();
                Ok( hself.try_send_msg( SendWsMsg{remote_addr,data})? )
            });
            self.h_data_actor.send_msg( ExecSnapshotAction(action)).await?;
        }
        Ok(())
    }
```

Since the `SpaService` needs to send a message to its *DataActor* this implies that a handle to the actor is stored in the
`SpaService`, usually from a `PreActorHandle` of the *DataActor* passed into the `SpaService` constructor.

Many *DataActors* have to obtain input from remote servers according to specific schedules hence there is a chance the first
clients are going to connect before the *DataActor* is ready. To avoid the overhead of creating, sending and executing superfluous
data actions and websocket messages we keep track of the `data_available` state of *DataActors* within the `SpaServer`. This works
by using a `init_action` field in the *DataActor* that has its actions executed once the data is initialized. The actor system
instantiation site (e.g. `main()`) then sets this action to send a `DataAvailable` message to the `SpaServer`, which passes it on
to matching `SpaServices` by calling their `data_available(..)` functions. Those functions can use the *DataActor* name and/or
the data type to determine if this is a relevant data source. If it is, and if the server already has connections, the `data_available()`
implementation sends a `DataRefAction` containing message to the *DataActor* just like in the `init_connection()` case above. The
`SpaServer` then stores the `data_available` status for that service, to be passed into subsequent `init_connection(..)` calls.

While the data availability tracking adds some overhead to both *DataActors* and `SpaService` implementations it is an effective
way to deal with the intrinsic race condition between connection requests and external data acquisition. In many cases the
implementations of `data_available()` and `init_connection()` share common code which should be factored out into separate functions.
There is a prominent exception to this symmetry rule. If the `SpaService` uses several *DataActors* and clients have to get a list of 
entities (e.g. satellites) they can expect data for then this list will be sent only - and un-conditionally - by `init_connection()` 
(e.g. see `odin_goesr::goesr_service::GoesrHotspotService` or `odin_orbital::hotspot_service::OrbitalHotspotService`).

This leaves us with data updates, which are always initiated by the *DataActor*. When its internal data model changes the *DataActor*
executes a `DataAction` that is stored in one of its fields which is set from the actor system instantiation site (`main()`) to an
action that creates a JSON message from the updated data and sends it as a `BroadcastWsMsg` message to the `SpaServer`. The
server then distributes the JSON message over the websockets of all of its current connections.


## 2. Instantiating the Web Application Actor System

What ties all this together is the site where we create the `SpaServices`, *DataActors* and the `SpaServer` - usually the `main()`
function of the application binary.

The following code is an example from the [`odin_sentinel`](../odin_sentinel/odin_sentinel.md) crate. The `SentinelActor` takes
the *DataActor* role, The `SentinelService` is the associated `SpaService`.

We use a `PreActorHandle` for the `SentinelActor` (*DataActor*) since we need to pass it into the `SentinelService` 
(`SpaService`) constructor, which is required to create the `SpaServer`, which is then in turn used to initialize the init/update
action fields when instantiating the `SentinelActor` (see [actor communication](../odin_actor/actor_communication.md) in 
`odin_actor`).

```rust
use std::any::type_name;
use odin_build;
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_sentinel::{SentinelStore,SentinelUpdate,LiveSentinelConnector,SentinelActor,load_config, web::SentinelService};

run_actor_system!( actor_system => {

    let hsentinel = PreActorHandle::new( &actor_system, "updater", 8);

    let hserver = spawn_actor!( actor_system, "server", SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "sentinels",
        SpaServiceList::new()
            .add( build_service!( hsentinel.to_actor_handle() => SentinelService::new( hsentinel)))
    ))?;

    let _hsentinel = spawn_pre_actor!( actor_system, hsentinel, SentinelActor::new(
        LiveSentinelConnector::new( load_config( "sentinel.ron")?), 
        dataref_action!( hserver.clone(): ActorHandle<SpaServerMsg> => |_store: &SentinelStore| {
            Ok( hserver.try_send_msg( DataAvailable{sender_id:"updater",data_type: type_name::<SentinelStore>()} )? )
        }),
        data_action!( hserver: ActorHandle<SpaServerMsg> => |update:SentinelUpdate| {
            let data = ws_msg!("odin_sentinel/odin_sentinel.js",update).to_json()?;
            Ok( hserver.try_send_msg( BroadcastWsMsg{data})? )
        }),
    ))?;
    
    Ok(())
});
```

## 3. Client Interaction

Please refer to the [Server-Client Interaction](client.md) section for details of how to write
client (browser side) code that interacts with the `SpaServer` and `SpaService` instances. 

These clients are represented by Javascript modules (served as service specific [**assets**](../odin_build/odin_build.md)) that do
direct DOM manipulation and use JSON message sent over websockets to communicate with the server.