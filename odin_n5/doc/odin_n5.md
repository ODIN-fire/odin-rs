# odin_n5

The `odin_n5` crate is an app domain crate to import fire sensor data from commercial [N5ensors.com](https://n5sensors.com/) devices. 
Access to the sensor data requires a subscription from N5sensors.com, i.e. this crate can only be used with a valid app key from the 
vendor that is stored in a ODIN config file.

The data is retrieved via HTTP from the configured N5Sensors server. The API provides end points to query devices for basic information
such as temperature and humidity and derived information such as smoke index and air quality. In addition, N5 sensors also supports
an alert system that can be either explicitly queried (for selected devices) or used through a POST endpoint in the client system
(e.g. an ODIN server). At this point `odin_n5` only uses alert queries but the `SpaService` capability to define service specific
routes is ideal to implement the POST notification channel.

This crate assumes that each application monitors a limited number of devices (<100). We do not yet provide an [alarm server](../intro.md)
but plan to integrate the `N5Actor` into a heterogenous alert system that fuses different sensor types (e.g. 
[`odin_alertca` webcams](../odin_alertca/odin_alertca.md), optical[`odin_sentinel` sensors](../odin_sentinel/odin_sentinel.md)
and various satellite sensors).


## Design

This crate uses a common ['SpaServer`](../odin_server/odin_server.mg) based design. Its major components are the `N5Actor` that
is responsible for data retrieval and client notification and `N5Service` which specifies the assets and websocket handlers
for this service.

```
                                                    odin │ external      
                                                         │               
ODIN/                  ┏━━━━━━━━━━━━━━━━━━━┓             │               
  config/              ┃ N5Actor           ┃             │               
    odin_n5/           ┃                   ┃             │  ┌───────────┐
      N5Config.ron  ───►  ┌─────────────────┐      http GET │ N5sensors │
                       ┃  │connector task ◄─┼──────────────►│  server   │
                       ┃  └─────────────────┘ JSON          │           │
                       ┃  ┌─────────────┐  ┃             │  └───────────┘
                       ┃  │N5DeviceStore│  ┃             │               
                       ┃  └─────────────┘  ┃             │               
                       ┃                   ┃                             
                       ┃   update_action   ┃                             
                       ┗━▲━━━━━━━━━━━━━━━━━┛                             
           exec_snapshot │      │                                        
         ┌───────────────┼──────┼──┐                                     
         │SpaServerActor │      │  │                                     
         │  ┏━━━━━━━━━━━━━━━━┓  │  │                                     
         │  ┃  N5Service     ┃  │  │                                     
         │  ┗━━━━━━━━━━━━▲━━━┛  │  │                                     
         └───────────────┼──────┼──┘                                     
                         │ wss  │               server                   
    ─────────────────────┼──────┼─────────────────────                   
                         │      │               client                   
                    ┌────┴──────▼───┐                                    
                    │  odin_n5.js   │                                    
                    └───────────────┘                                    
```

Since N5 devices do not use images or other data files the communication between the `N5Actor` and the N5sensors server
only involves HTTP GET requests to which the N5Sensors server replies with [JSON](https://www.json.org/json-en.html) data.
We only retrieve the last data points for each device on a configured interval. The amount of data is small enough to
allow direct transmission to clients through their websockets. No cached data is needed.


## Configuration

The `odin_n5` crate only uses a single `N5Config` configuration struct that is primarily used to specify the 
URL for the external server together with the API key that is required to authenticate as a valid subscriber.
This is also the reason why respective configuration files should not be part of the repository but kept outside
within the ODIN server file system (e.g. `ODIN_ROOT/configs/odin_n5/` - see [`odin_build`](../odin_build/odin_build.md))

```rust,ignore
# config example for N5 access

N5Config(
    base_uri: "<N5-server-url>",
    api_key: "<your-secret-api-key>",

    max_history_len: 20, // keep 20 last data points per device
    data_cycles: 1, // we only get the most recent entry for each retrieve interval cycle
    retrieve_interval: Duration(secs: 300, nanos: 0), // retrieve device data every 5 minutes
    aggregate_interval: Duration(secs: 9000, nanos: 0), // average over 15min period (shortest duration)
)
```

## Example

```rust
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_n5::{
    actor::N5Actor, get_json_update_msg, get_n5_devices, live_connector::LiveN5Connector, 
    load_config, Device, N5Config, N5DataUpdate, N5DeviceStore, n5_service::N5Service,
};

run_actor_system!( actor_system => {
    let pre_n5 = PreActorHandle::new( &actor_system, "n5", 8);

    let hn5 = pre_n5.to_actor_handle();
    let hserver = spawn_actor!( actor_system, "server", 
        SpaServer::new(
            odin_server::load_config("spa_server.ron")?,
            "n5",
            SpaServiceList::new()
                .add( build_service!( => N5Service::new( hn5) ))
        )
    )?;

    let n5_id = pre_n5.get_id();
    let hn5 = spawn_pre_actor!( actor_system, pre_n5, 
        N5Actor::new( 
            LiveN5Connector::new( load_config("n5.ron")?),
            dataref_action!( 
                let sender_id: Arc<String> = n5_id, 
                let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |store: &N5DeviceStore| {
                    Ok( hserver.try_send_msg( DataAvailable::new::<N5DeviceStore>(sender_id) )? )
                }
            ),
            data_action!( 
                let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |updates: Vec<N5DataUpdate>| {
                    let ws_msg = get_json_update_msg( &updates);
                    Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
                }
            )
        )
    )?;

    Ok(())
});
```