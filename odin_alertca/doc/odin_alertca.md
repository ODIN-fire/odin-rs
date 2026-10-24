# odin_alertca

The `odin_alertca` app domain crate imports images from configured [AlertCalifornia.org](https://alertcalifornia.org/) cameras.
AlertCalifornia is a ucsd.edu program that makes more than 1000 live camera feeds (mostly from utility providers) available for
wildfire detection.

Many of these cameras can be remotely controlled by authorized operators which means that `odin_alertca` not only
has to periodically retrieve the imagery for configured cameras but also their attitude (azimut, tilt and zoom).

The `odin_alertca` crate does not attempt to monitor the complete camera set from AlertCalifornia. We use a configuration
file to specify the area specific set of cameras that should be processed. This makes data retrieval a straight forward
periodic process that first obtains the camera state from AlertCalifornia.org (distributed as a single 
[JSON](https://www.json.org/json-en.html) file) and then downloads the images for the configured camera entries that have
changed. All external access is using simple HTTP GET requests.

The camera state is distributed by AlertCalifornia as a single [GeoJSON](https://geojson.org/) `FeatureCollection` that
contains a `Point` feature for each camera. The feature `geometry` contains the precise camera position (longitude,
latitude and height), its `properties` include the camera `id` (which is not human readable), a human readable `name`,
the field of view boundaries (`fov_lft`, `fov_rt`, `fov_center`), the current pan/tilt/zoom state for PTZ cameras
(`az_current`, `tilt_current`, `zoom_current` - these are `null` for fixed cameras) and the time of the last image
(`last_frame_ts`). An example can be found in `odin_alertca/resources/all_cameras.json`.

Since this file contains more than 1000 cameras of which we typically only need a small subset (~30), `odin_alertca` uses
[serde_json](https://docs.rs/serde_json/latest/serde_json/) to stream-parse the features one at a time, only turning
features with configured ids into `Camera` objects. This is robust against changes in property order or additional
properties.

Apart from interactively showing camera viewing directions and downloaded imagery the main purpose of `odin_alertca` is
to drive automatic fire detection using machine learning (not yet implemented).


## Design

The main components of this crate are the `AlertCaActor` and the associated `AlertCaService` which are used in a 
common [`SpaServer`](../odin_server/odin_server.md) setting:

```
                                                        odin │ external         
                                                             │                  
    ODIN/                  ┏━━━━━━━━━━━━━━━━━━━┓             │                  
      config/              ┃ AlertCaActor      ┃             │                  
        odin_alertca/      ┃                   ┃             │                  
          AlertCaConfig ───►  ┌─────────────────┐                AlertCalifornia
                           ┃  │connector task ◄─┼───────────────     server     
                           ┃  └─────────────────┘  - all_cameras.json           
                           ┃  ┌───────────┐    ┃   - images                     
                           ┃  │CameraStore│    ┃             │                  
                           ┃  └───────────┘    ┃             │                  
                           ┃                   ┃───────┐     │                  
                           ┃   update_action   ┃       │                        
                           ┗━▲━━━━━━━━━━━━━━━━━┛       │                        
               exec_snapshot │      │                  │                        
             ┌───────────────┼──────┼─────┐   ODIN/    │                        
             │SpaServerActor │      │     │     cache/ ▼                        
             │  ┏━━━━━━━━━━━━━━━━┓  │     │       odin_alertca/                 
             │  ┃ AlertCaService ┃  │  ┌──┼──────── <images>                    
             │  ┗━━━━━━━━━━━━▲━━━┛  │  │  │                                     
             └───────────────┼──────┼──┼──┘                                     
                             │ wss  │  │ http          server                   
        ─────────────────────┼──────┼──┼─────────────────────                   
                             │      │  │               client                   
                        ┌────┴──────▼──▼──┐                                     
                        │ odin_alertca.js │                                     
                        └─────────────────┘                                     
```

The `AlertCaActor` spawns a dedicated connector task upon start which is responsible for retrieving the external
data at a configured interval (e.g. every 3min). This data includes the camera states (`all_cameras.json`) and the
last image for each configured camera. The connector sequentially retrieves `all_cameras.json` and changed image
files to prevent download bursts on the (external) AlertCalifornia server.

Each `Camera` object represents the state of a camera at a given time (position, name, field of view, pan/tilt/zoom and
image). The `CameraStore` keeps the last `max_history` states for each camera. It is created from the first camera data
the connector retrieves after the actor got started - this is the only time the `init_action` is executed. Subsequent
connector reports only contain cameras that have been updated (and for which we could retrieve images), which are passed
to the `update_action` before they are added to the store. Since the camera position is part of its state, cameras can
move. Configured cameras that were not included in the initial data are added once they show up.

Updates are transmitted through the `update_action` as JSON messages to all connected clients which can then retrieve
respective images through HTTP from the `SpaServer`.

The subset of cameras we monitor is read from a `ODIN/config/odin_alertca/*.ron` file. Received images are stored and
then served from the transient `ODIN/cache/odin_alertca/` directory (which is cleaned up in periodic intervals).

The `odin_alertca.js` client module labels cameras with their `name` and flags cameras that have not been updated for
more than the `maxAlertAge` configured in `odin_alertca_config.js`.

Future use of the `AlertCaActor` for automatic fire detection will not require changes but only different `update_action`
initialization which is set where we define the application (usually the `main()` function).


## Configuration

The `odin_alertca` crate uses one config structure that has the primary function of naming the cameras we are interested
in. It also allows to configure the external URLs to use and the interval in which we access them.

```rust,ignore
// example RON config file for AlertCalifornia cameras in the SF Bay Area
AlertCaConfig(
    cameras: [ // camera ids as used in the AlertCalifornia all_cameras.json data
        "limekilncanyon-cam01",
        "pressonhill-cam01",
        ...
    ],

    base_url: "https://cameras.alertcalifornia.org/public-camera-data",

    update_interval: Duration(secs:180,nanos:0), // update every 3 min
    max_history: 10, // number of camera states we keep per camera
    max_age: Duration(secs:3600,nanos:0), // keep data for 1h
)
```

## Example

```rust
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_alertca::{
    actor::AlertCaActor, 
    alertca_service::AlertCaService, 
    live_connector::LiveAlertCaConnector, 
    load_config, CameraStore, Camera, get_json_update_msg
};
use anyhow::Result;

run_actor_system!( actor_system => {
    let pre_aca = PreActorHandle::new( &actor_system, "alertca", 8);

    let haca = pre_aca.to_actor_handle();
    let hserver = spawn_actor!( actor_system, "server", 
        SpaServer::new(
            odin_server::load_config("spa_server.ron")?,
            "alert-ca",
            SpaServiceList::new()
                .add( build_service!( => AlertCaService::new( haca) ))
        )
    )?;

    let aca_id = pre_aca.get_id();
    let haca = spawn_pre_actor!( actor_system, pre_aca,
        AlertCaActor::new( 
            load_config("sf_bay_area.ron")?,
            LiveAlertCaConnector::new,
            dataref_action!( 
                let sender_id: Arc<String> = aca_id, 
                let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |store: &CameraStore| {
                    Ok( hserver.try_send_msg( DataAvailable::new::<CameraStore>(sender_id) )? )
                }
            ),
            dataref_action!( 
                let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |updates: &Vec<Camera>| {
                    let ws_msg = get_json_update_msg( &updates);
                    Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
                }
            )
        )
    )?;

    Ok(())
});
```

## External URLs

The AlertCalifornia URLs in use are

- https://cameras.alertcalifornia.org/public-camera-data/all_cameras-v3.json - for retrieving all camera states
- https://cameras.alertcalifornia.org/public-camera-data/⟨camera-name⟩/latest-frame.jpg - for the last image of the specified camera
