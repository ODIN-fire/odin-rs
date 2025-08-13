# odin_adsb

The `odin_adsb` crate is an application domain crate to import and display aircraft information received from an [ADS-B](https://en.wikipedia.org/wiki/Automatic_Dependent_Surveillance%E2%80%93Broadcast) server. This is an example of a low latency streaming data source with hundreds of 
objects updated in a 1Hz interval. There are currently two supported data sources

- [dump1090](https://github.com/flightaware/dump1090)
- [jet1090](https://crates.io/crates/jet1090)

Both support a variety of physical receiver devices (e.g. RTL-SDR dongles) and provide a socket through which decoded ADS-B text messages are served.
In order to use this crate you need to have access to a dump1090 or jet1090 server.

The primary purpose of `odin_adsb` is to demonstrate how to implement low-latency, near-realtime tracking in ODIN.


## Main Constructs

`odin_adsb` is a variation of the common ODIN data import crate structure such as used by [`odin_sentinal`](../odin_sentinel/odin_sentinel.md). It consists of

- an `AdsbActor` to store received aircraft information and distribute it within ODIN applications
- a configured `AdsbConnector` implementation that is used by the actor to retrieve ADS-B messages from the external receiver
  (`odin_adsb` currently has two impls: `SbsConnector` and `JetConnector`)
- an `AdsbService` to make ADS-B aircraft data available as a layer within the [`odin_server`](../odin_server/odin_server.md)`::SpaServer` 

What differs is the way in which external data is retrieved. Whereas most other crates are in control of when and how to retrieve data (which
usually includes initial retrieval of history) this is strictly reactive in `odin_adsb` - the source is a data stream without history. Consequently there is no difference between initial and consecutive retrieval and hence no `DataAvailable` message sent to the 
server/micro service. This means the actor is more simple and only has an *update* action to define what should be done with changed aircraft records.

```
                                          odin │ external             
                                               │              aircraft
                   ┌─────────────────┐         │                 │    
   AdsbConfig ────►│ AdsbActor       │         │                 │    
                   │┌────────────────┴───┐     │    ┌────────────▼───┐
                   ││ connector-task     │     │    │    ADS-B       │
                   ││┌─────────────┐◄────┼──socket──┤receiver/decoder│
                   │└│AircraftStore│─┬───┘     │    └────────────────┘
                   │ └──────┬──────┘ │         │      - dump1090      
                   │ ▲      ▼        │         │      - jet1090       
                   │ │update_action  │         │                      
                   └─┼──────┬────────┘                                
       exec_snapshot │      │                                         
     ┌───────────────┼──────┼──┐                                      
     │SpaServerActor │      │  │                                      
     │     ┌─────────┴──┐   │  │                                      
     │     │ AdsbSevice │   │  │                                      
     │     └─────────▲──┘   │  │                                      
     └───────────────┼──────┼──┘                                      
                     │      │wss msg    server                        
─────────────────────┼──────┼─────────────────                        
                     │      │           client                        
                ┌────┴──────▼──┐                                      
                │ odin_adsb.js │                                      
                └──────────────┘                                      
```

The critical part is to have a low latency connector that can keep up with the external data source, to minimize synchronization between
the connector and the actor and to batch changes in the actor before they are sent out via its update slot to avoid back pressure in the
rest of the application actor system.

To that end the connector instance runs as blocking task within its own kernel thread. Communication with the actor is done through
a shared [DashMap](https://crates.io/crates/dashmap) instance (a concurrent *shard hashmap* to reduce lock contention). All entry mutations
are done through the connector but the actor periodically removes stale entries which have not been updated for a configured amount of time.
The workload differs from usual concurrent hashmap use in that here writes (from the connector) are frequent (millisecond range) and reads
(from the actor) only happen at configured intervals (in second range). The actor reads have to happen in bounded time and have to avoid
global locks that would block the connector. This is a caveat for the implementation of update actions (which happen from the actor read
scope), which is why the `AircraftStore` provides efficient functions to turn changed entries into JSON messages that can be sent to 
a `SpaServer` - the standard update action. 

The AdsbActor does not send received aircraft updates right away to the server - it starts a timer on a configured
`update_interval`. The corresponding `_Timer_` message handler triggers the `update_action` (usually set where the
actor system is initialized) with the current state of the `AircraftStore`. The typical action in a `SpaServer` application
is to call `AircraftStore::get_json_update_msg(writer)` to collect all aircraft changed since the last update into
a single [JSON](https://www.json.org/json-en.html) message. This shields against potential message bursts and allows
to use `update_interval` to scale the system. This is important since aircraft send ADS-B messages triggered by
their *Mode S transponders* (i.e. as a irregular time series).

ADS-B messages are received by the `AdsbConnector` through a socket that streams text messages (JSON for jet1090,
CSV for dump1090). Since data update lifespan is short and updates are small they are sent as JSON messages to the client
through the websocket. 

## Configuration

`odin_adsb` at this point as a single config file (see [`odin_build`](../odin_build/odin_build.md)) that is used
to instantiate the `AdsbActor` and shared with its `AdsbConnector`:

```ron
AdsbConfig(
    source: "KNUQ",                        // the receiver station name
    timezone: "America/Los_Angeles",       // time zone of receiver station
    url: "localhost:30003",                // the socket from which to read the ADS-B data
    update_interval: Duration(secs:60,nanos:0), // how often to send out aircraft updates
    max_trace: 64,                         // number of trace points (last reported positions) to keep in ringbuffer
    drop_after: Duration(secs:25,nanos:0), // duration after which un-changed aircraft entries are dropped 
)
```

`max_trace` is used to initialize a circular buffer that is used to store the N last positions received for this
aircraft. `drop_after` is the duration after which aircraft that have not been updated will be dropped (there is
no explicit drop message in ADS-B as it is used for in-flight situational awareness).

## JS Module

The `odin_adsb.js` module also follows the standard layout set forth in [`odin_server`](../odin_server/client.md) but
with the added challenge that we need efficient update of data displayed both in Cesium and in the user interface components.
Moreover, (dynamically updated) flight paths need to be visualized in a 3D context and require [*GLTF*](https://www.khronos.org/gltf/)
models that are displayed with attitude parameters (Euler angles) to show the aircraft orientation.

## Example

This is a minimal applicaton to show `AdsbActor`, `SbsConnector` and `AdsbService` in a `SpaServer` application:

```rust
use chrono::{DateTime,Utc};
use odin_actor::prelude::*;
use odin_common::json_writer::JsonWriter;
use odin_server::prelude::*;
use odin_adsb::{load_config, Aircraft, AircraftStore, actor::AdsbActor, sbs::SbsConnector, adsb_service::AdsbService};
use anyhow::Result;

run_actor_system!( actor_system => {
    let pre_server = PreActorHandle::new( &actor_system, "server", 64);

    let hadsb = spawn_actor!( actor_system, "adsb",
        AdsbActor::<SbsConnector,_>::new(
            load_config("adsb.ron")?, 
            dataref_mut_action!(  
                let mut w: JsonWriter = JsonWriter::with_capacity(4096), // use a cached writer to assemble the ws_msg
                let mut hserver: ActorHandle<SpaServerMsg> = pre_server.to_actor_handle() => 
                |store: &AircraftStore| {
                    let ws_msg = store.get_json_update_msg(w);
                    Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
                }
            )
        )
    )?;

    let hserver = spawn_pre_actor!( actor_system, pre_server, SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "adsb",
        SpaServiceList::new()
            .add( build_service!( => AdsbService::new( vec![hadsb]) ))
    ))?;

    Ok(())
});
```

## Building and running dump1090

[dump1090](https://github.com/flightaware/dump1090) has to be cloned from the github repository and requires a working
`CC` compiler. Assuming the standard RTL based USB receiver you need to install the native `librtlsdr` package and then build
like so:

```sh
git clone https://github.com/flightaware/dump1090
cd dump1090
make  make RTLSDR=yes BLADERF=no HACKRF=no LIMESDR=no
```

Execute `dump1090 --help` to see the various command line options.

## Building and running jet1090

Since [jet1090](https://crates.io/crates/jet1090) is a Rust application with available sources it can be built and installed
by a simple 

```sh
cargo install jet1090
```

Running it is slightly more complex as it just prints the received ADS-B messages to the console, which means we have
to capture `stdout` and re-publish through a server socket. There are multiple ways to do this with standard Unix commands
(netcat or socat): 

(1) single client, multiple reconnects

```sh
mkfifo /tmp/adsb_fifo
nc -l -k -p 30003 < /tmp/adsb_fifo&
jet1090 -v rtlsdr:// > /tmp/adsb_fifo
```

(2) multiple clients, multiple reconnects

```sh
mkfifo /tmp/adsb_fifo
socat TCP-LISTEN:30003,reuseaddr,fork,cool-write 'PIPE:/tmp/adsb_fifo!!PIPE:/tmp/adsb_fifo'&
jet1090 -v rtlsdr:// > /tmp/adsb_fifo
```

Note this blocks jet1090 until the first connection is made. The `cool-write` option is essential to avoid
socat shutting down and causing a broken pipe when a client disconnects
