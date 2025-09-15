# odin_wind

## Introduction

The `odin_wind` application domain crate is used to compute high resolution wind fields by means of the
[*WindNinja*](https://ninjastorm.firelab.org/windninja/) microgrid wind model developed at the 
[Missoula Firelab](https://research.fs.usda.gov/firelab). Wind being a major factor for fire behavior
this is crucial terrain- and time-of-day dependent information that also can be challenging to visualize.
General weather forecast wind data often does not have enough spatial resolution to faithfully predict
wind in rugged terrain.

The primary purpose of `odin_wind` is to obtain digital elevation data (from [`odin_dem`](../odin_dem/odin_dem.md))
for requested areas, periodically retrieve weather forecasts (with [`odin_hrrr`](../odin_hrrr/odin_hrrr.md)) and
then execute *WindNinja* (as an external process) for each forecast hour. The results are wind fields
(stored as [GeoTIFF](https://en.wikipedia.org/wiki/GeoTIFF) file) which are then (by means of [`odin_gdal`](../odin_gdal/odin_gdal.md))
translated into  [CSV](https://en.wikipedia.org/wiki/Comma-separated_values) and [GeoJSON](https://geojson.org/) text
files which are suitable to be distributed through a [`odin_server`](../odin_server/odin_server.md) based micro service
and visualized as

- animated particle system
- vector grid
- contour plots

on top of a virtual globe in a browser (using the infrastructure of [`odin_cesium`](../odin_cesium/odin_cesium.md)).
In this respect `odin_wind` is a good example how the various parts of `odin-rs` fit together and can utilize
sophisticated 3rd party components such as *WindNinja*.

## WindNinja

The computational heavy lifting in `odin_wind` is done by [*WindNinja*](https://ninjastorm.firelab.org/windninja/) 
which is used by ODIN as an external process. WindNinja sources are **not** part of the `odin-rs` distribution and
have to be downloaded separately. Prerequisites are:

(1) a working C++ compiler (e.g. [gcc](https://gcc.gnu.org/) or [clang](https://clang.llvm.org/)). These are
available for all platforms and can be installed through native package managers if they are not already distributed
with the OS.

(2) the [CMake](https://cmake.org/) build system, which is also available through native package managers

(3) the same [GDAL](https://gdal.org/en/stable/) library that is also used by [`odin_gdal`](../odin_gdal/odin_gdal.md) and
hence probably already installed as a prerequisite of `odin-rs` itself.

With this building of WindNinja breaks down into the following steps:

(4) obtain sources from this [Github repository](https://github.com/pcmehlitz/windninja). Please note we still
require this fork as not all changes have been merged back into the official WindNinja repository yet:

```sh
mkdir odin-windninja
cd odin-windninja
git clone https://github.com/pcmehlitz/windninja
```

(5) create build directory and use cmake within this directory to configure and build WindNinja

```sh
mkdir build
cd build
cmake -DCMAKE_POLICY_VERSION_MINIMUM=3.5 -DCMAKE_BUILD_TYPE=Release -DNINJA_CLI=ON -DNINJA_QTGUI=OFF ../windninja
...
cmake --build .
...
```

(6) test - the above steps should have created a `src/cli/WindNinja_cli` binary that can be executed from the command line:

```sh
src/cli/WindNinja_cli --help
```

At this point you can either adjust your `PATH` environment to include this directory, move `WindNinja_cli` to a 
directory that is already in the `PATH`, or just leave it there and specify the full path to `WindNinja_cli` in
a `$ODIN_ROOT/configs/odin_wind/wind.ron` configuration file (see [`odin_build`](../odin_build/odin_build.md)):

```ron
WindConfig(
    ...
    windninja_cmd: <path-to-WindNinja_cli>
    ...
)
```

Please note that step(5) has to be repeated every time you update the native [GDAL](https://gdal.org/en/stable/) library.


## Main Constructs and Dataflow

The `odin_wind` crate has two main constructs: `WindActor` and `WindService`. The `WindActor` is responsible for
obtaining the input data required by WindNinja, executing WindNinja itself, post-processing its output and then
announcing availability of the output by executing its `update` action which is normally set within `main` to
send an output file availability notification as JSON message to a [`SpaServer`](../odin_server/odin_server.md).

```
         ┌───────────┐       ╔═══════════╗           
         │ WindActor │◄─────►║ WindNinja ║           
         └──┬─────▲──┘       ║ (process) ║
            │     │          ╚═══════════╝                     
      ┌─────▼─────┼─────┐                          
      │SpaServer  │     │                          
      │  ┌───────────┐  │                          
      │  │WindService│  │                          
      │  └───────────┘  │                          
      └──────┼──┼───────┘   tier 2: user server    
─────────────┼──┼──────────────────────────────────
         ┌───┘  └────┐      tier 1: browser clients
    ┌────▼───┐   ┌───▼────┐                        
    │browser1│...│browserN│                        
    └────────┘   └────────┘                        
```

WindNinja's main inputs for each region of interest are:

- digital elevation (DEM) data and 
- [NOAA HRRR](https://rapidrefresh.noaa.gov/hrrr/) weather forecasts (containing at least 10m U,V wind speed, 
  2m temperature and total cloud cover fields)

While the DEM data only has to be retrieved once for each region, we have to support a mode of operation in which
weather reports (HRRR data) are continuously retrieved and each new data set triggers a new WindNinja forecast
computation to ensure that users always have the latest / updated data for each forecast hour. From a data flow
perspective this means that apart from the `AddWindClient` messages received through websockets (from connected
browsers) availability of new weather data (HRRR or station) drives the (repeated) WindNinja computation and thus
respective `HrrrFileAvailable` input messages are the main `WindActor` triggers.

```                                                          
                                ┌───────────────────────────────────┐               
                                │     WindActor                     │               
                           ┌────┴─┐                         ╭─────╮ │               
                           │update│   ┌─────────────────────┴──┐  │ │               
          ┌── forecast ────┤action│◄──┤compute derived products│◄─╯ │               
          │    JSON        └────┬─┘ 6 │       ▲                │    │               
          │                     │     │       │5               │    │               
          │                     │     │run WindNinja ◄──── Forecast ───────────────── WindNinja
          │                     │     │       ▲                │    │              (child process)
          │                     │     │       │4               │    │               
          │                     │     │get latest wx report ◄──┼─ HrrrFileAvailable ── HrrrActor
          │                     │     └────────────────────────┘    │                     
          │                     │             ▲                     │               
        ┌─┼───────────────┐     │             │3                    │               
        │ │   SpaServer   │     │      get DEM file for region ◄────┼──────────────── DemSource
        │ │               │     └─────────────▲─────────────────────┘              (server of file)
        │ │ ┌───────────┐ │                   │2                                     
        │ │ │WindService│─│── AddWindClient ──┘                                     
        │ │ └───────▲───┘ │                                                           
        └─┼─────────┼─────┘                                                             
         7│         │1            ODIN server                                           
──────────┼─────────┼─────────────────────────                                          
          │websocket│             clients (browser)                                     
        ┌─▼───────────┐                                                                 
        │ odin_wind.js│                                                                 
        └─────────────┘                                                                 
```


Since running WindNinja can be computationally intensive it should only be executed for regions that are explicitly
requested by clients, which should also make sure that different clients use the same region coordinates for the same
incidents (e.g. by means of [`odin_share`](../odin_share/odin_share.md) defined regions). This is especially important
since we have to run WindNinja for each new HRRR forecast data set (up to 18/48 per hour - see
[`odin_hrrr`](../odin_hrrr/odin_hrrr.md)). 

The DEM data is acquired through [`odin_dem`](../odin_dem/odin_dem.md). This can either happen through a `serve_dem`
server over the network (in case the tile map data for the DEM is too large) or directly and synchronously through
the SpaServer file system. Using the `odin_dem::DemSource` enum makes this configurable through the `WindConfig.ron`
configuration file.

This step is triggered by an incoming request to simulate a given region that is not yet in the list if active
regions. 

The Weather data is periodically obtained from the [`odin_hrrr::HrrrActor`](../odin_hrrr/odin_hrrr.md). WindNinja
required fields are 10m UGRD,VGRD, 2m TMP and TCDC (cloud cover), which are also specified in `WindConfig.ron.

Once the `WindActor` receives a notification about the available HRRR forecast step it queues a `WnJob` that
is executed by a speparate task spawned by the `WindActor`. This task is responsible for launching a `WindNinja`
process per forecast and uses the result (a *.tif {h,u,v,w} windvector grid in UTM coordinates) to compute three
client display related data products via [`odin_gdal`](../odin_gdal/odin_gdal.md):

- a *.csv {h,u,v,w, spd} windvector grid in [WGS84](https://en.wikipedia.org/wiki/World_Geodetic_System) 
  (client input for particle system animation)
- a *.csv with wind vector field in [ECEF](https://en.wikipedia.org/wiki/Earth-centered,_Earth-fixed_coordinate_system)
  (client input for static wind vector display)
- a *.json with [GeoJSON](https://geojson.org/) windspeed contour polygons in WGS84 coordinates

Once respective files are available the `WindActor` executes its update_action which usually sends respective
notifications to connected clients.

```
                ┏━━━━━━━━━━━━━┓                               
WindConfig ────►┃  WindActor  ◄────────────► odin_dem         
                ┃             ┃                    ┊          
                ┃ ┌─────────┐ ┃    ╔═══════════╗   ┊ (*.tif)  
                ┃ │ wn_task ◄──────► WindNinja ◄┈╌╌╯          
                ┃ └─▲───┬─┬─┘ ┃    ║ (process) ◄╌╌╌╮          
                ┃   │   │ ┊   ┃    ╚═══════════╝   ┊ (*.grib2)
                ┃   │   │ ┊   ┃    ┌───────────┐   ┊          
                ┃   │   │ ┊   ◄────► HrrrActor ├╌╌╌╯          
                ┗━━━┼━━━┼━┼━━━┛    └───────────┘              
      region reqest │   │ ┊                                 
                    │   │ ╰┄┄┄┄┄┄┄┄┄┄┄╮                                  
    ┌───────────────┼───┼─────┐       ┊ ODIN_ROOT/cache/odin_wind/  wind field display data:                    
    │SpaServerActor │   │     │       ▼                        
    │    ┏━━━━━━━━━━━━┓ │     │     (*.csv {h,u,v,w} grid)      (particle animation)                
    │    ┃ WindSevice ┃ │  ╭╌╌┼╌╌╌╌ (*.csv {x,y,z} vectors)     (static vector field)     
    │    ┗━━━━━━━━━▲━━┛ │  ┊  │     (*.json windspeed contour)  (polygons)
    │              │    │  ┊  │                               
    └──────────────┼────┼──┼──┘                               
                  wss msg  ┊ http GET                               
                   │    │  ┊                                     server       
───────────────────┼────┼──┼───────────────────────────────────────────       
                   │    │  │                                     clients (browser)        
                ┌──▼────▼──▼───┐    ┌──────────────┐                          
                │ odin_wind.js │────► glsl shaders ├┐                               
                └──────────────┘    └─┬────────────┘│
                                      └─────────────┘
```

The `WindService` is a [`odin_server::SpaService`](../odin_server/odin_server.md) implementation that waits
for incoming websocket JSON messages requesting wind forecasts for a new region. If this region is not already
in the list of active areas the request is passed on to the `WindActor`. Once the `SpaServerActor`
receives notifications for respective available  forecast steps it sends those over the websocket to connected
clients where they are processed in the [`odin_wind.js` JS module](../odin_server/client.md). If the user selects
a forecast step and visualization type (particle animation, vector field or windspeed contour polygons) the
`odin_wind.js` module retrieves associated data files over http GET and creates respective 
[CesiumJS](https://cesium.com/platform/cesiumjs/) visualization objects.

Vector fields and wind speed contour polygons map into normal [Cesium Entities](https://cesium.com/learn/ion-sdk/ref-doc/Entity.html).

The particle animation requires more effort involving [GLSL shaders](https://www.khronos.org/opengl/wiki/Core_Language_(GLSL)), which
are served through `WindService` routes from the `odin_wind/assets/wind-particles/glsl` directory
(see [https://cesium.com/blog/2019/04/29/gpu-powered-wind/] for a general description).


## Configuration

`odin_wind` has one configuration file for the `WindActor` that can normally reside inside the repository as it does not contain
authorization data:

```rust,ignore
WindConfig(
    max_age: Duration( secs: 3600, nanos: 0), // 1h - how long to keep cached data files
    max_forecasts: 9, // max number of forecasts to keep for each region (in ringbuffer)

    windninja_cmd: "$ODIN_ROOT/bin/WindNinja_cli", // pathname for windninja executable (if not absolute path it has to be in PATH)
    mesh_res: 150, // windninja mesh resolution in meters
    wind_height: 10, // above ground in meters

    //dem: Server("http://localhost:9019"),
    dem: File("$ODIN_ROOT/data/3dep13-conus-i16/3dep13-conus-i16.vrt"),
    dem_res: 25.0, // pixel size in meters

    // keep the set of variables/levels small to reduce the download size (those fields are required by WindNinja)
    hrrr_fields: ["UGRD", "VGRD", "TCDC", "TMP" ],
    hrrr_levels: ["lev_2_m_above_ground", "lev_10_m_above_ground", "lev_80_m_above_ground", "lev_entire_atmosphere"]
)
```

The values for `hrrr_fields` and `hrrr_levels` are the ones required for WindNinja. Since HRRR supports a large number of
fields (see [https://nomads.ncep.noaa.gov/gribfilter.php?ds=hrrr_2d]) which could be used for additional purposes we support
field/level specification so that we only have to query the HRRR server once.


## Example

This is a minimal application that uses a `WindActor`, a `HrrrActor` and a `SpaServer` to display windfields for regions
selected from shared items:

```rust
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_share::prelude::*;
use odin_hrrr::{self,HrrrActor,HrrrConfig,HrrrFileAvailable,schedule::{HrrrSchedules,get_hrrr_schedules}};
use odin_wind::{ 
    actor::{WindActor,WindActorMsg, AddClientResponse, server_subscribe_action, server_update_action}, 
    ForecastStore, Forecast, 
    wind_service::WindService
};

run_actor_system!( actor_system => {
    let pre_server = PreActorHandle::new( &actor_system, "server", 64);
    let pre_hrrr = PreActorHandle::new( &actor_system, "hrrr", 8);

    // spawn a shared store actor - the JS module only allows forecast region requests for shared GeoRects
    let hshare = spawn_server_share_actor(&mut actor_system, "share", pre_server.to_actor_handle(), default_shared_items(), false)?;

    let hwind = spawn_actor!( actor_system, "wind", WindActor::new(
        odin_wind::load_config("wind.ron")?,
        pre_hrrr.to_actor_handle(),
        server_subscribe_action( pre_server.to_actor_handle()),
        server_update_action( pre_server.to_actor_handle()) 
    ))?;

    let hrrr = spawn_pre_actor!( actor_system, pre_hrrr, HrrrActor::with_statistic_schedules(
        odin_hrrr::load_config( "hrrr_conus-8.ron")?,
        data_action!( let hwind: ActorHandle<WindActorMsg> = hwind.clone() => |data: HrrrFileAvailable| {
            Ok( hwind.try_send_msg( data)? )
        })
    ).await? )?;

    let hserver = spawn_pre_actor!( actor_system, pre_server, SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "wind",
        SpaServiceList::new()
            .add( build_service!( let hshare = hshare.clone() => ShareService::new( "odin_share_schema.js", hshare)) )
            .add( build_service!( => WindService::new( hwind) ))
    ))?;

    Ok(())   
});
```

Since the `WindActor` to `SpaServer` interaction is fairly uniform (as described above) `odin_wind::actor` provides
`server_subscribe_action( h_server)` and `server_update_action( h_server)` functions to simplify respective 
[action](../odin_action/odin_action.md) setup.


## WindServer

Apart from that WindNinja requires potentially large input data sets (DEM source and repeated HRRR weather reports) it
can also run in high fidelity mode (conservation of mass and momentum) which might overwhelm both available
network bandwidth and computational resources (memory, speed) of user servers. Moreover, the produced forecast
data files (wind fields) are relatively small (compressed CSV). As a consequence this is a prime example of
computation we want to be able to offload from a user server and delegate to a remote [edge server](../intro.md)
that runs at a location with sufficient connectivity and compute power (e.g. data center / cloud).

We can achieve this by moving the `WindActor` into an `WindServer` edge server and replacing it in the user facing web
server with a `WindServerClient` actor that is an adapter between clients and the remotely running `WindActor`.
We basically split a local `WindActor` into a `WindServerClient` / `WindServer` pair running on different machines:

```
                   ┌───────────┐       ╔═══════════╗                        
                   │ WindActor │◄─────►║ WindNinja ║                        
                   └──┬─────▲──┘       ║ (process) ║                        
                      │     │          ╚═══════════╝                                   
                 ┏━━━━▼━━━━━┷━━━┓                                         
                 ┃  WindServer  ┃                                         
                 ┗━━━━▲━━━▲━━━━━┛                  tier 3: edge server    
──────────────────────┼───┼───────────────────────────────────────────────
           ┌──────────┘   └───────────┐                                  
  ┏━━━━━━━━▼━━━━━━━━┓        ┏━━━━━━━━▼━━━━━━━━┓                          
  ┃WindServerClient1┃   ╎    ┃WindServerClientM┃                          
  ┗━━━━━┯━━━━━▲━━━━━┛   ╎    ┗━━━━━┯━━━━━▲━━━━━┛                          
        │     │         ╎          │     │                                
  ┌─────▼─────┼─────┐   ╎    ┌─────▼─────┼─────┐                          
  │SpaServer1 │     │   ╎    │SpaServerM │     │                          
  │  ┌───────────┐  │  ...   │  ┌───────────┐  │                          
  │  │WindService│  │   ╎    │  │WindService│  │                          
  │  └───────────┘  │   ╎    │  └───────────┘  │                          
  └──────▲──▲───────┘   ╎    └──────▲──▲───────┘   tier 2: user server    
─────────┼──┼────────── ╎ ──────────┼──┼──────────────────────────────────
     ┌───┘  └────┐      ╎       ┌───┘  └────┐      tier 1: browser clients
┌────▼───┐   ┌───▼────┐ ╎  ┌────▼───┐   ┌───▼────┐                        
│browser1│...│browserN│ ╎  │browser1│...│browserN│                        
└────────┘   └────────┘ ╎  └────────┘   └────────┘                        
```

The edge server uses a `WindServer` actor instead of the (user server)`SpaServer`/`WindService` combo to
drive the `WindActor`. This allows the main computational chain to be reusable as-is. `WindActor`, `DemSource`, 
`HrrrActor`, `SpaServer`, `WindService` and associated `odin_wind.js` JS module can all be reused without modifications.

Remotely computed data from the edge server is cached by the `WindServerClient` on the local user server, which
means we only have to reach out to the edge server for new data.

This is a good example of how ODIN actors can help to make distributed computation scalable.

The main caveat is that the data structures that are used in both `WindServerClient` and `WindServer` (notably
`ForecastStore` and `Forecast`) should *not* contain transient information that is only required during the actual
computation by `WindActor` (e.g. `WnJob` or `HrrrDataSetRequest`). Care must be taken to separate the data model
into the shared and the `WindActor` private part.

We also have to be aware that (repeated) wind field computation is a subscription service, i.e. we have to keep track of
external clients and provide push capabilities to these clients through websockets. In the user server (`SpaServer`)
case the subscribers are connected browsers. For the `WindServer` the subscribers are the user servers (websocket
connections to remote `WindServerClient` instances). Both are repesented by `std::net::SocketAddr` values representing
remote websocket end points. Neither should the edge server receive browser `SocketAddr` values nor should the user
server unveil its own edge server subscription to browsers. We have to be aware of that we use (some of) the same data
structures (e.g. `AddWindClient`) with related but not identical semantics and that we now have a 3 tier distributed
system (edge server, user server, and browser clients).

Subscription / push capabilities mean that both the edge server and the user server(s) are stateful. Other edge
servers that provide one-way data streams or REST APIs can be considerably less complex.

The user server code looks like this:

```rust
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_share::prelude::*;
use odin_wind::{ 
    actor::{WindActorMsg, server_subscribe_action, server_update_action}, 
    server_client::WindServerClient,
    wind_service::WindService
};

run_actor_system!( actor_system => {
    let pre_server = PreActorHandle::new( &actor_system, "server", 64);

    // spawn a shared store actor - the JS module only allows forecast region requests for shared GeoRects
    let hshare = spawn_server_share_actor(&mut actor_system, "share", pre_server.to_actor_handle(), default_shared_items(), false)?;

    let hwind = spawn_actor!( actor_system, "wind", WindServerClient::new(
        odin_wind::load_config("wind_client.ron")?,
        server_subscribe_action( pre_server.to_actor_handle()),
        server_update_action( pre_server.to_actor_handle()) 
    ))?;

    let hserver = spawn_pre_actor!( actor_system, pre_server, SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "wind",
        SpaServiceList::new()
            .add( build_service!( let hshare = hshare.clone() => ShareService::new( "odin_share_schema.js", hshare)) )
            .add( build_service!( => WindService::new( hwind) ))
    ))?;

    Ok(())   
});
```

The remote edge server code is:

```rust
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_hrrr::{self,HrrrActor,HrrrConfig,HrrrFileAvailable,schedule::{HrrrSchedules,get_hrrr_schedules}};

use odin_wind::{ 
    actor::{WindActor, WindActorMsg}, 
    server_client::WindServerClient,
    ForecastStore, Forecast, 
    server::{WindServer,WindServerMsg, wind_server_subscribe_action, wind_server_update_action}
};

run_actor_system!( actor_system => {
    let pre_server = PreActorHandle::new( &actor_system, "server", 64);
    let pre_hrrr = PreActorHandle::new( &actor_system, "hrrr", 8);

    let hwind = spawn_actor!( actor_system, "wind", WindActor::new(
        odin_wind::load_config("wind.ron")?,
        pre_hrrr.to_actor_handle(),
        wind_server_subscribe_action( pre_server.to_actor_handle()),
        wind_server_update_action( pre_server.to_actor_handle()) 
    ))?;

    let hrrr = spawn_pre_actor!( actor_system, pre_hrrr, HrrrActor::with_statistic_schedules(
        odin_hrrr::load_config( "hrrr_conus-8.ron")?,
        data_action!( let hwind: ActorHandle<WindActorMsg> = hwind.clone() => |data: HrrrFileAvailable| {
            Ok( hwind.try_send_msg( data)? )
        })
    ).await? )?;

    let hserver = spawn_pre_actor!( actor_system, pre_server, WindServer::new(
        odin_wind::load_config("wind_server.ron")?,
        "wind",
        hwind
    ))?;

    Ok(())   
});
```