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
        (id: "lomaprieta-cam01", height_above_ground: 12.0), // with local camera attributes
        ...
    ],
    default_height_above_ground: 10.0, // [m] for cameras without configured height_above_ground

    base_url: "https://cameras.alertcalifornia.org/public-camera-data",

    update_interval: Duration(secs:180,nanos:0), // update every 3 min
    max_history: 10, // number of camera states we keep per camera
    max_age: Duration(secs:3600,nanos:0), // keep data for 1h
)
```

Cameras are either specified as plain ids or as `(id: .., height_above_ground: .., tilt_offset: ..)` structs with
local camera attributes that are not (correctly) reported by AlertCalifornia. Both fields are optional:

- `height_above_ground` [m] (default: `default_height_above_ground` of the config, which defaults to 10): height of the camera above the ground (e.g. mast or tower height). The
  reported camera altitude is the ground elevation. This is used for viewpoint tracking and viewshed computation
- `tilt_offset` [deg] (default 0): correction that is added to the reported (or OCR'ed) camera tilt for viewpoint tracking

These values can be calibrated with viewpoint tracking (see below) by checking the distance of known landmarks, and if
the ray of a visible point below the horizon does not hit the terrain (too high camera or too small depression angle).

## Viewpoint Tracking

Image windows (`odin_server/ui_windows.js` `ImageWindow`) have a "track viewpoint" checkbox. If checked, the mouse
position over the image is shown with a vertical and a horizontal line and reported (relative to the image center) to
`odin_alertca.js`, which renders:

- a **bearing line** from the camera position: `azimut + atan(x * tan(hfov/2))`, where `x` is the relative horizontal
  position in [-1,1] and `hfov` is the (zoom dependent) horizontal field of view of the camera when the image was taken
- for cameras that report their tilt (PTZ cameras), the **terrain intersection** of a ray with this bearing and the
  pitch `tilt + tilt_offset + atan(y * tan(vfov/2))` (vertical field of view computed from `hfov` and the image aspect
  ratio). The ray starts at the terrain height of the camera position plus `height_above_ground` and is intersected
  with the most detailed terrain by `odin_cesium::terrainIntersection()`. Rays that do not hit the terrain within
  `viewTrack.maxDist` are labeled with "no terrain intersection"
- the **view angle** of the ray (`bearing / pitch` in degrees) as a label at the camera position

Colors and widths of the bearing line and the intersection range can be changed in the "layer parameters" panel of
the window, which also controls color and alpha of displayed viewsheds (initial values are from `odin_alertca_config.js`).

Clicking on the image freezes/unfreezes the tracked position. Since distances are very sensitive to pitch errors at
grazing angles (`distance ~ camera height above terrain / tan(depression angle)`), the terrain intersection is shown as
the range between the intersections for pitch ± `viewTrack.pitchUncertainty` (default 0.25°), labeled with the nominal
distance and the range in km. Results are reliable in the near field and become increasingly uncertain towards the
horizon. Other error sources are the camera height (`height_above_ground`), tilt calibration (`tilt_offset`), lens
distortion, atmospheric refraction and the difference between the Cesium terrain and the DEM used for viewsheds.

Azimuth, tilt and zoom of each image are read from its overlay text (e.g. `elev:2812' X:-54.88 Y:-1.78 Z:1.0`) by means
of OCR (`odin_alertca::overlay`, using the `odin_image::ocr` module with the PP-OCR models in
`$ODIN_ROOT/data/odin_image/ocr`). This is required since the AlertCalifornia camera data does not include PTZ values
for all PTZ cameras, and PTZ values are updated separately from frames (`ptz_updated_ts` vs. `last_frame_ts`), i.e. they
might not match the image if the camera moved. If OCR fails (or the models are missing) we fall back to the camera data.
The horizontal field of view is always taken from the camera data.

## Viewsheds

The "vsh" column of the camera list shows the viewshed of a camera, i.e. the terrain area that is visible from the camera
position (in any direction since most cameras can pan). Viewsheds are static and hence computed offline with the
`compute_vsh` tool, which stores them as `<camera-id>.geojson.gz` files in `$ODIN_ROOT/data/odin_alertca`. The
`AlertCaService` serves them as `alertca-vsh/<camera-id>.geojson` and `odin_alertca.js` loads them when the respective
"vsh" checkbox is checked for the first time. Cameras without a viewshed file just get their checkbox cleared.

```shell
compute_vsh                                       # all cameras of the sf_bay_area.ron AlertCaConfig
compute_vsh --config my_cameras.ron --height 15   # all cameras of another config, overriding their configured heights
compute_vsh --id blackmtscc-cam01 --lon -122.147301 --lat 37.3186 --height 15  # single camera
```

The camera positions of the config mode are taken from the current AlertCalifornia camera data (or the cached
`all_cameras.json` if it cannot be retrieved). Since the reported altitude is the ground elevation, the camera height
above ground is taken from the configured `height_above_ground` of each camera (an explicit `--height` overrides it).
Single cameras (`--id`) also use the configured height if the camera is in the config, otherwise the
`default_height_above_ground` of the config. Other options are `--max-dist` (default 40km), `--res` (DEM
grid resolution, default 30m), `--resample` (DEM resampling algorithm, default `bilinear` - nearest neighbour
resampling of the 10m source DEM creates noisy terrain and hence about twice the number of polygons), `--target-height` (height of visible targets above ground, default 0m), `--min-patch`
and `--simplify` (see below), `--dem` (`odin_dem` server URL or VRT file, default is the 3DEP 1/3" VRT in
`$ODIN_ROOT/data/odin_dem`) and `--output-dir`.

The computation is implemented in `odin_gdal::viewshed`:

1. a UTM DEM that covers the max distance around the camera is obtained through `odin_dem` (and cached)
2. `GDALViewshedGenerate()` computes the visibility raster (including earth curvature and refraction)
3. visible and invisible patches with less than `--min-patch` cells (default 50, i.e. 4.5ha at 30m) are merged into
   their surroundings (`GDALSieveFilter()`) - this removes speckles that would only add polygons
4. visible cells are polygonized (`GDALPolygonize()`), simplified with a `--simplify` tolerance (default 30m) and
   written as a single WGS84 MultiPolygon feature with 5 decimal coordinates

With the defaults the SF Bay Area cameras have 90-2150 polygons per viewshed (up to 0.3MB per gzipped file). Note the
viewshed only considers terrain - vegetation and buildings are not part of the DEM.

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
