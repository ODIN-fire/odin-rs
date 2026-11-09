# odin_bushfire

The `odin_bushfire` crate is an application domain crate to download, display and update information about bushfires from [Geoscience Australia](https://ecat.ga.gov.au/geonetwork/srv/eng/catalog.search#/home) servers.

The specific data set supported by `odin_bushfire` is the [Near Real-Time National Bushfire Extents](https://ecat.ga.gov.au/geonetwork/srv/eng/catalog.search#/metadata/150695) set, which is distributed in form of [GeoJSON](https://geojson.org/) files.

`odin_bushfire` follows the common [ODIN application domain crate structure](../crate_anatomy.md), i.e. it consists of

- a `lib.rs` module defining the data model
- an `actor.rs` module providing a [`odin_actor::Actor`](../odin_actor/odin_actor.md) implementation for data import and update
- a `service.rs` module providing a [`odin_server::spa::SpaService`](../odin_server/odin_server.md) implementation for a bushfire micro service
- a `odin_bushfire.js` client (browser) Javascript module to interactively browse and display bushfire objects on top of a virtual globe

```
                                                    odin │ external      
     ODIN/                                               │               
       configs/            ┏━━━━━━━━━━━━━━━━━━━━┓        │               
         odin_bushfire/    ┃ BushfireActor      ┃        │  ┌───────────┐
           bushfire.ron ───►                    ┃           │  external │
                           ┃          ┌────────────────────►│   data    │
     ODIN/                 ┃          │         ┃           │  server   │
       cache/              ┃  ┌──────────────┐  ┃        │  └───────────┘
         odin_bushfire/    ┃  │ BushfireStore│  ┃        │               
           <data>          ┃  └──────────────┘  ┃        │               
                           ┃    update_action   ┃                        
                           ┗━━▲━━━━━━━━━━━━━━━━━┛                                                                   
                              │      │                                   
                exec_snapshot │      │                                   
            ┌─────────────────┼──────┼──┐                                
            │ SpaServerActor  │      │  │                                
            │      ┏━━━━━━━━━━━━━━━┓ │  │                                
            │      ┃BushfireService┃ │  │                                
            │      ┗━━━━━━━━━━▲━━━━┛ │  │                                
            └─────────────────┼──────┼──┘                                
                              │   wss│               server              
         ──────────────────── │ ──── │ ────────────────────              
                              │      │               client              
┌────────────────────────┐   ┌┴──────▼──────────┐                        
│ odin_bushfire_config.js┼──►│ odin_bushfire.js │                        
└────────────────────────┘   └──────────────────┘                        
```

While many other crates following this structure have more complex data retrieval protocols `odin_bushfire` just periodically retrieves a single file through http and hence does not have a dedicated connector object/task. Since file sizes are small enough (<1MB), the external server is fast enough and the `BushfireActor` only has one runtime message to react to (`ExecSnapshotAction` in response to a new connection) the retrieval is performed from within the actor upon receiving a _Timer_ message from a local repeat timer that uses a configured interval (`BushfireConfig.check_interval`).

Similar to [`odin_fires`](../odin_fires/odin_fires.md) the `odin_bushfire` crate does unpack the downloaded, continent-wide geojson file and breaks it up into `Bushfire` objects, storing respective polygons in (potentially large) separate geojson files.

The `BushfireStore` used by the `BushfireActor` is a tuple struct for a `HashMap<String,VecDeque<Bushfire>>`, i.e. it uses the unique bushfire id to store the most recently retrieved `Bushfire` records in a ring buffer, only sending updates for changed entries to clients. Clients in turn only download the geojson polygon files on demand when the user requests a specific bushfire perimeter.

As with the data from the US [WFIGS Current Interagency Fire Perimeters](https://data-nifc.opendata.arcgis.com/datasets/nifc::wfigs-current-interagency-fire-perimeters/about) the challenge is handle incomplete/missing feature properties.


## Configuration

```rust,ignore
// sample config for odin_bushfire
BushFireConfig (
    url: "https://services-ap1.arcgis.com/ypkPEy1AmwPKGNNv/ArcGIS/rest/services/Near_Real_Time_Bushfire_Boundaries_view/FeatureServer/3/query?where=area_ha+%3E+0.001&outFields=*&f=pgeojson",
    dem: Some(File("$ODIN_ROOT/data/odin_dem/srtm-1sec-aus-i16.tif")), // or server url
    check_interval: Duration( secs: 1800, nanos: 0),  // check every 30min for updates
    max_history: 10, // number of records per fire we keep (in a ringbuffer)
    max_age: Duration( secs: 604800, nanos: 0), // ignore fires that were reported before (older than 1 week)
)
```

Downloaded data and generated perimeter files are stored in the `odin_bushfire` cache dir, which is cleaned up by
[odin_scrub](../odin_scrub/odin_scrub.md).

## Web Application User Interface

`odin_bushfire` has a single web application user interface window (titled "Bushfire") to browse data sets.

The "fires" panel shows the `Bushfire` entries that match the criteria according to the checkboxes (state, type of bushfire, size of bushfire). Each fire is represented as a single line with basic information such as area, number of available updates and latest update.

<img class="left" src="../img/odin_bushfire-ui.png" width="40%">
    
Selecting a bushfire entry populates the list that shows each available update. Selecting the "show" checkbox of a line displays the perimeter on the virtual globe.

Since the stored fire id is not well suited for display we assign a shorter internal `#<number>` to correlate between the user interface and the virtual globe.

Double clicking on a bushfire item zooms/pans the globe to the most recent location.

Selecting a fire symbol (point or bitmap) on the virtual globe selects the corresponding list item in the "Bushfire" window.

The "fire info" panel shows additional alphanumeric information associated with the fire (e.g. global id and reporting agency)
