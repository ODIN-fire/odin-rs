# odin_fems

The `odin_fems` crate is an application domain crate to import and display station data from the
[Fire Environment Mapping System (FEMS)](https://fems.fs2c.usda.gov/), which includes both
current and forecasted

- weather data (temperature, rel humidity, hourly precipitation, wind, peak gusts and solar radiation)
- [National Fire Danger Rating System (NFDRS)](https://research.fs.usda.gov/firelab/projects/firedangerrating) 
  data (1/10/100h dead fuel moisture, Keech Bryam drought index, burning index and ignition/spread/energy release 
  components for V,W,X,Y,Z fuel models )

FEMS provides data from  about 3400 stations across the US, which submit data on an hourly basis. The `odin_fems` crate
does not import all of these but uses a `fems.ron` config file that specifies the stations of interest.

For each monitored station we keep the last observation plus a configured set of forecast hours (both for
both weather and nfdrs data records). Although each stations has its own hourly transmission time (which is
part of the station meta data) there can be delays until new observation records become available. For
this reason we poll each station that *could* have new data in a configured interval (3-5min).

The FEMS server (currently https://fems.fs2c.usda.gov/api/climatology/graphql) uses [GraphQL](https://graphql.org/)
as defined in its [FEMS Climatology AI External User Guide](https://wildfireweb-prod-media-bucket.s3.us-gov-west-1.amazonaws.com/s3fs-public/2025-09/FEMS%20Climatology%20API%20External%20User%20Guide.pdf) of which we currently use the following queries:

- StationMetaData
- WeatherObs
- NfdrObs

Although the Rust ecosystem has a `graphql-rust/graphql-client` crate we do not use it (yet) for two reasons: (a) we
only use a few fixed queries for which we can easily create respective http POST bodies, and (b) some of the FEMS
stations do not have regular 5 digit `station_id` values (e.g. 43913 for Los Gatos) but 8 digit `fems_station_id` values
(e.g. 55522229 for Calero) and for these normal graphql queries fail - we need full controll over defining our own
that pass in station_id and other values as variables.

## Components

The `odin_fems` crate follows the common [ODIN application domain crate structure](../crate_anatomy.md), i.e. it consists of

- a `lib.rs` module defining the data model
- an `actor.rs` module providing a [`odin_actor::Actor`](../odin_actor/odin_actor.md) implementation for data import and update
- a `service.rs` module providing a [`odin_server::spa::SpaService`](../odin_server/odin_server.md) implementation for a bushfire micro service
- a `odin_fems.js` client (browser) Javascript module to interactively browse and display FEMS station data on top of a virtual globe

```
                                                    odin │ external      
     ODIN/                                               │               
       configs/            ┏━━━━━━━━━━━━━━━━━━━━┓        │               
         odin_fems/        ┃ FemsActor          ┃        │  ┌───────────┐
           fems.ron  ──────►                    ┃           │  external │
                           ┃          ┌────────────────────►│   data    │
     ODIN/                 ┃          │         ┃           │  server   │
       cache/              ┃  ┌──────────────┐  ┃        │  └───────────┘
         odin_bushfire/    ┃  │   FemsStore  │  ┃        │               
           <data>          ┃  └──────────────┘  ┃        │               
                           ┃    update_action   ┃                        
                           ┗━━▲━━━━━━━━━━━━━━━━━┛                                                                   
                              │      │                                   
                exec_snapshot │      │                                   
            ┌─────────────────┼──────┼──┐                                
            │ SpaServerActor  │      │  │                                
            │      ┏━━━━━━━━━━━━━━━┓ │  │                                
            │      ┃  FemsService  ┃ │  │                                
            │      ┗━━━━━━━━━━▲━━━━┛ │  │                                
            └─────────────────┼──────┼──┘                                
                              │   wss│               server              
         ──────────────────── │ ──── │ ────────────────────              
                              │      │               client              
┌────────────────────────┐   ┌┴──────▼──────────┐                        
│ odin_fems_config.js    ┼──►│ odin_fems.js     │                        
└────────────────────────┘   └──────────────────┘                        
```
The `FemsActor` is not (yet) using a separate `connector` task to retrieve the external data because we only retrieve data through
3 fixed http queries (i.e. the protocol is fairly simple), the response is fast and the `FemsActor` does not have to minimize
message latency as it does not have to handle a high volume of messages.

To save queries on the external server we batch the initial data retrieval and just send three queries for the whole set of 
configured stations. As a consequence the initial download size for about 20 stations and 8 forecast hours is ~2MB. Subsequent
queries for updated weather and nfdrs observations/forecasts are only in the 20kB range per station. The updates are also
well distributed over the 60min reporting interval.

The actor might get a connector in the future if its message interface is extended to support creating dead fuel moisture maps
as input for fire behavior simulators.

For each of the queries we use separate raw and high level data structures (e.g. `RawWeatherObs` and `FemsWeatherObs`). The first
one is mostly a declarative specification of the data to query and only uses basic types such as `String` and `f32`. The high level
internal model uses `TryFrom<raw-type>` impls to add units of measure (using the `uom` crate) and do canonicalization. 

The JSON data sent to clients over  the websocket varies slightly from the high level struct format to minimize redundancy 
(e.g. for NfdrObs records for different fuel models). This is acceptable as the number of fuel models is fixed 
(V: grass, W: grass-shrub, X: brush, Y: timber, Z: slash).

To have more control over the JSON serialization we use the `odin_common::json_writer` infrastructure.

Since we query all forecast hours together with the current observation we just replace weather and nfdrs records for updated
stations, i.e. there is no need for ring buffers or other reorganizing container types. The `FemsStore` therefore is just
a tuple struct for a simple `HashMap` that uses numeric station ids as keys and `FemsStation` aggregation objects as values.


## Configuration

```rust,ignore
// sample odin_fems configuraion file in RON
FemsConfig (
    region: "SantaClara",
    url: "https://fems.fs2c.usda.gov/api/climatology/graphql",
    station_ids: [
        43913, // Los Gatos
        43809, // Ben Lomond
        ...
    ],
    tx_delay: Duration( secs: 60, nanos: 0), // wait 1 min after scheduled tx time
    check_interval: Duration( secs: 240, nanos: 0), // check every 4 min
    forecast_hours: 8,
    max_file_age: Duration( secs: 21600, nanos: 0) // delete files after 6h
)
```

## Example

The `odin_fems/src/bin/show_fems.rs` source shows a minimal application that uses a `FemsActor` connected
to a `SpaServer` actor to serve FEMS data to connected browsers:

```rust,ignore
use std::sync::Arc;

use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_fems::{ load_config, FemsStore, FemsStation, actor::{FemsActor,FemsActorMsg}, service::FemsService };

run_actor_system!( actor_system => {
    let pre_fems = PreActorHandle::new( &actor_system, "fems", 8);

    let hserver = spawn_actor!( actor_system, "server",
        SpaServer::new(
            odin_server::load_config("spa_server.ron")?,
            "fems",
            SpaServiceList::new()
                .add( build_service!( let hfems: ActorHandle<FemsActorMsg> = pre_fems.to_actor_handle() => FemsService::new( hfems) ))
        )
    )?;

    let fems_id = pre_fems.get_id();
    let _hfems = spawn_pre_actor!( actor_system, pre_fems,
        FemsActor::new(
            load_config("fems.ron")?,
            dataref_action!(
                let sender_id: Arc<String> = fems_id,
                let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |store: &FemsStore| {
                    Ok( hserver.try_send_msg( DataAvailable::new::<FemsStore>(sender_id) )? )
                }
            ),
            dataref_action!(
                let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |station: &FemsStation| {
                    let ws_msg = station.get_json_update_msg();
                    Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
                }
            )
        )
    )?;

    Ok(())
});
```

## Web Application User Interface

`odin_fems` uses a single web application user interface window (titled "FEMS stations") to browse data sets.

<img class="left" src="../img/odin_fems-ui.png" width="40%">

The UI window has 4 collapsible panels, of which only the top `stations` panel is expanded by default. This panel
shows a single list with configured stations. For each station the basic current weather data is displayed
together with the lastest received observation time. Selecting a station populates the bottom 3 panels with
observation data for this station. Double clicking on a station also zooms/pans to the respective station
on the virtual globe. Selecting a station symbol on the virtual globe also selects this station in the list.

The `weather` panel shows the recorded weather data for the selected station. The first item is the current
observation, the following lines represent forecast hours.

NFDRS data is split between the two bottom panels. The `nfdrs` panel includes the fuel model independent
data whereas the `fuel models` panel hold a `TabbedContainer` with tabs for each of the fuel models
(V: grass, W: grass-shrub, X: brush, Y: timber, Z: slash).
