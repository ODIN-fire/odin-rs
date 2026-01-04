# odin_himawari

The `odin_himawari` crate is an application domain crate to import and display data from the geostationary 
["Himawari"](https://en.wikipedia.org/wiki/Himawari_(satellites)) satellites that cover most of Asia, Australia
and New Zealand.

This crate currently supports the `L2WLF010_FLDK` hotspot data product downloaded from 
a [JAXA](https://global.jaxa.jp/) [ftp server](ftp.ptree.jaxa.jp:21) which requires a registered user account that 
can be obtained [here](https://www.eorc.jaxa.jp/ptree/registration_top.html). Using this crate therefore requires
a configuration file that contains the user credentials and therefore has to be stored outside
the ODIN source tree (e.g. in `ODIN_ROOT/configs/odin_himawari/himawari.ron` - see [odin_build](../odin_build/odin_build.md)).

The Himawari hotspot data products are computed for each hour in 10min increments (HH:00, HH:10, HH:20, HH:30, HH:40, HH:50)
and a nominal latency of about 40min. More information can be found in the [Himawari User Guide](https://www.eorc.jaxa.jp/ptree/userguide.html) and the [Himawari Wildfire Product Documentation](https://www.eorc.jaxa.jp/ptree/documents/README_H08_L2WLF.txt).

The main functions of the `odin_himawari` crate are:

1. periodic data retrieval (e.g. every 5min)
2. translation of external data formats (mostly [CSV](https://en.wikipedia.org/wiki/Comma-separated_values)) into internal data model
3. async import/notification with import actor
4. web (micro) service for browser based visualization
5. archive replay (TBD)


## Crate Modules

This is a single-source, regular time interval data retriever module with a [Cesium](https://cesium.com/) based web front end.
As such it follows the standard ODIN crate design with the following modules:

- `lib.rs` : data model definitions and cross-module retrieval/parse functions (`HimawariHotspot`, `HimawariHotspotSet`,
  `HimawariHotspotStore`, `HimawariConfig`)
- `live_importer.rs` : FTP based periodic data retrieval and translation (`LiveHimawariHotspotImporter`)
- `actor.rs` : [odin_actor](../odin_actor/odin_actor.md) based import and data distribution `HimawariHotspotActor` (using a 
   parameterized `HimawariHotspotImporter` trait implementor)
- `service.rs` : defines `HimawariHotspotService` - a [odin_server](../odin_server/odin_server.md) based 
   `odin_server::spa::SpaService` implementation that makes `HimawariHotspotActor` data available in the context of a `odin_server::spa::SpaServer` single page web application server
- `errors.rs` : the `odin_himawari` error definitions and corresponding external error mapping

The diagram below shows how these components are connected:

```
                                                        odin │ external      
    ODIN/                                                    │               
      config/             ┏━━━━━━━━━━━━━━━━━━━━┓             │               
        odin_himawari/    ┃HimawariHotspotActor┃    - cwd    │               
          himawari.ron ───►                    ┃    - nlst   │  ┌───────────┐
                          ┃  ┌──────────────────┐   - retr      │   JAXA    │
    ODIN/              ┌─────┼ importer task  ◄─┼──────────────►│    ftp    │
      cache//          │  ┃  └──────────────────┘ CSV           │  server   │
        odin_himawari/ ▼  ┃  ┌──────────────┐  ┃             │  └───────────┘
          H09_...WLF.csv ───►│ HotspotStore │  ┃             │               
                          ┃  └──────────────┘  ┃             │               
                          ┃    update_action   ┃                             
                          ┗━━▲━━━━━━━━━━━━━━━━━┛                             
               exec_snapshot │      │                                        
        ┌────────────────────┼──────┼──┐                                     
        │ SpaServerActor     │      │  │                                     
        │ ┏━━━━━━━━━━━━━━━━━━━━━━┓  │  │                                     
        │ ┃HimawariHotspotService┃  │  │                                     
        │ ┗━━━━━━━━━━━━━━━━━━▲━━━┛  │  │                                     
        └────────────────────┼──────┼──┘                                     
                             │   wss│               server                   
        ─────────────────────┼──────┼─────────────────────                   
                             │      │               client                   
┌───────────────────────┐   ┌┴──────▼────────┐                               
│odin_himawari_config.js┼──►│odin_himawari.js│                               
└───────────────────────┘   └────────────────┘                                                             
```

The `odin_himawari` crate is an example of how to retrieve data through [FTP](https://en.wikipedia.org/wiki/File_Transfer_Protocol).
Conceptually we could compute a download schedule in order to minimize latency but as of this time (01/2026) the delay variability for new files showing up on the FTP server is in the same order of magnitude as the update interval (~10min). It is not stable enough to support a precomputed/configured schedule for single data sets. Since the product user manual defines a filename scheme that includes the product, date and reference time we therefore resort to

- query at a regular time interval < 10min (configured update interval should be ~5min)
- first obtain a directory listing of available files from the FTP server
- only download files we did not retrieve in a previous cycle

Moreover, the FTP server provides hotspot data sets as compact CSV files (which further reduces the required bandwidth) but since each update cycle has to re-establish a FTP connection the configured update interval should not be lower than ~3min. More frequent updates would not be useful anyways since the time from sensor data acquisition to data product availability on the FTP server is typically ~40min.

Himawari uses a similar instrument as the [GOES-R](https://en.wikipedia.org/wiki/GOES-18) satellites and therfore also experiences 
saturation and masking that causes false positives and false negatives. Analogous to the [`odin_goesr`](../odin_goesr/odin_goesr.md) data we therefore use a ringbuffer to maintain the last N datasets so that users can step through them and see changes/gaps for a selected hotspot over time. The hotspot classification of Himawari uses a different set of attributes as GOES-R and differentiates between

- level (flaming,smoldering,cold)
- pixel reliability (high, normal, low)
- pixel quality (normal, saturated, low confidence)


## Configuration

So far `odin_himawari` uses a single configuration file that is kept outside the repository (normally in 
`$ODIN_ROOT/configs/odin_himawari/himawari.ron`) since it contains JAXA server user credentials:

```rust,ignore
// example config for Himawari hotspot import

HimawariConfig(
    sat_id: 41836, // Himawari 9

    // get free account from https://www.eorc.jaxa.jp/ptree/registration_top.html
    user: "<your-JAXA-uid-here>",
    pw: "<your-JAXA-pw-here>",
    uri: "ftp.ptree.jaxa.jp:21",

    dem: Some(File("$ODIN_ROOT/data/srtm-aus/srtm-aus-i16.vrt")), // put your DEM data dir here (e.g. Australia)
    // dem: None, // if you don't have digital elevation data for region of interest

    init_hours: 6, // initially retrieve 6h of data
    update_interval: Duration( secs: 300, nanos: 0), // check for new files every 5min
    cleanup_interval: Duration( secs: 3600, nanos: 0), // remove old files > max_age once per hour
    max_age: Duration(secs:43200,nanos:0),         // keep entries/files for 12hr
)
```

## Example

This is a minimal example application that uses a `HimawariHotspotActor` and a `SpaServer` with a single
`HimawariHotspotService` micro service layer to display and update Himawari hotspot data on top of a virtual
globe in a single page web application:

```rust,ignore
use std::sync::Arc;
use odin_build;
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_himawari::{
    HimawariConfig, HimawariHotspotStore, HimawariHotspotSet, PKG_CACHE_DIR,
    service::HimawariHotspotService, actor::HimawariHotspotActor, live_importer::LiveHimawariHotspotImporter
};

run_actor_system!( actor_system => {

    let pre_server = PreActorHandle::new( &actor_system, "server", 64);

    let config: Arc<HimawariConfig> = Arc::new( odin_himawari::load_config("himawari.ron")?);
    let himawari = spawn_actor!( actor_system, "himawari", HimawariHotspotActor::new(
        config.clone(),
        LiveHimawariHotspotImporter::new( config, Arc::new( PKG_CACHE_DIR.clone())),
        dataref_action!(
            let sender_id: Arc<String> = Arc::new("himawari".to_string()),
            let hserver: ActorHandle<SpaServerMsg> = pre_server.to_actor_handle() => |store: &HimawariHotspotStore| {
                Ok( hserver.try_send_msg( DataAvailable::new::<HimawariHotspotStore>(sender_id) )? )
            }
        ),
        data_action!(
            let hserver: ActorHandle<SpaServerMsg> = pre_server.to_actor_handle() => |hs: HimawariHotspotSet| {
                let w = hs.to_json()?;
                let ws_msg = ws_msg_from_json( HimawariHotspotService::mod_path(), "hotspots", w.as_str());
                Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
            }
        )
    ))?;

    let _hserver = spawn_pre_actor!( actor_system, pre_server,
        SpaServer::new(
            odin_server::load_config("spa_server.ron")?,
            "himawari",
            SpaServiceList::new()
                .add( build_service!( => HimawariHotspotService::new( himawari) ))
        )
    )?;

    Ok(())
});
```

## Web Application User Interface

`odin_himawari` has a single web application user interface window (titled "Himawary Hotspots") to browse data sets.

<img class="left" src="../img/odin_himawari-ui.png" width="35%">

The **`"data sets"`** panel contains a time sorted list of retrieved data set files (most recent one on top). Each line
shows some statistics about the hotspots contained in this set. The `date` column refers to the date/time the sensor data
was acquired, the `recv` column shows when the respective file was downloaded. 

Selecting a data set entry populates the **`"hotspots"`** panel. Each hotspot (pixel) is represented by its own line that shows
the hotspot classification (level, reliability and quality flag), measured fire radiative power (frp) and pixel area and
the hotspot location as geodetic coordinates (the fire product only provides 3 decimal precision for hotspot locations).
Double clicking on a hotspot line zooms and pans the virtual globe view to the respective hotspot location.

Selecting a hotspot entry populates the **`"history"`** panel with all hotspots from all data sets that have the same position.
This allows to assess the history/progression of a single hotspot.

The **`"filter"`** panel (which is collapsed by default) can be used to select hotspot classifications to display (level, reliability
and pixel quality).

The **`"layer parameters"`** panel holds input elements to modify display parameters for hotspots.
