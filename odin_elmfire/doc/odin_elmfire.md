# odin_elmfire

The `odin_elmfire` crate is an application domain crate to run and display wildfire spread simulations with
[ELMFIRE](https://elmfire.io/) (Eulerian Level set Model of FIRE spread), an open source fire behavior model developed by
[Chris Lautenberger](https://github.com/lautenberger/elmfire). ELMFIRE is an external executable - `odin_elmfire` does
not re-implement the model but automates the creation of its inputs, runs the executable and post-processes its
outputs for visualization.

ELMFIRE requires a substantial set of inputs for each simulation, including

- fuel and topography ([FBFM40](https://www.landfire.gov/fuel/fbfm40), canopy, slope, aspect, elevation) from [odin_landfire](../odin_landfire/odin_landfire.md)
- hourly weather forecasts from [Open-Meteo](https://open-meteo.com/) through [odin_openmeteo](../odin_openmeteo/odin_openmeteo.md)
- dead and live fuel moisture computed from weather history and forecasts with [odin_nfdrs](../odin_nfdrs/odin_nfdrs.md)
- optional micro-grid (terrain adjusted) wind computed with [WindNinja](https://ninjastorm.firelab.org/windninja/) through
  [odin_wind](../odin_wind/odin_wind.md)

A number of these inputs are computed by means of 3rd party applications such as [WindNinja](https://github.com/firelab/windninja) and [NFDRS4](https://github.com/firelab/NFDRS4) (encapsulated in their respective ODIN crates such as `odin_wind` and `odin_nfdrs`).
The main task of `odin_elmfire` therefore is to orchestrate these data acquisitions and computations in order to implement the
following data flow (solid lines (`──►`) denote dynamic inputs that change with each simulation (forecasts), dotted lines (`┄┄►`) denote
(semi) static inputs):

```
   external          ODIN node
   (servers)         (executables orchestrated by ODIN)
                  ╎                       (semi) static
┌──────────┐      ╎  ╭──────────╮
│ landfire │────────►│  cache   │┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┐
└──────────┘      ╎  ╰──────────╯                       ┆
                  ╎       │                             ┆ - fuel type
                  ╎       ▼                             ┆ - topo
                  ╎  ┌──────────┐  fuel moisture (fc)   ┆
  ┌─────────────────►│  NFDRS   │───────────────┐       ┆
  │ - historical  ╎  └──────────┘               │       ┆
  │ - current+fc  ╎                             │       ┆         - orchestration
  │  ┌──────────┐ ╎                    dynamic  │       ┆         - pre-/post-
  │  │   FEMS   │───────────────────────────────┤       ┆           processing
  │  └──────────┘ ╎                             ▼       ▼         - visualization
  │               ╎  ╭──────────╮           ┏━━━━━━━━━━━━━━━┓     ┌──────────┐
  │               ╎  │local data│┄┄┄┄┄┄┄┄┄┄►┃    Elmfire    ┃────►│ ODIN app │
  │               ╎  ╰──────────╯           ┗━━━━━━━━━━━━━━━┛     └──────────┘
  │               ╎  - structures               ▲       ▲              │
  │               ╎  - population               │       ┆              │
┌─┴────────┐      ╎                             │       ┆              │
│Open-Meteo│────────────────────────────────────┤    ╭──┴─────╮        │
└─┬────────┘      ╎                             │    │ config │◄───────┘
  │               ╎  ┌──────────┐  wind (fc)    │    ╰────────╯
  ├─────────────────►│WindNinja │───────────────┘
  │               ╎  └──────────┘                    - spotting
┌─┴────────┐      ╎       ▲                          - wx
│   HRRR   │      ╎  ╭──────────╮                    - wui
└──────────┘      ╎  │   3dep   │
                  ╎  ╰──────────╯
```

Simulations can run either with current (i.e. automatically acquired) weather and forecasts, or they can be performed as "what if"
scenarios with user defined weather and fuel moisture data. In addition, synthetic or actual wind can be used either directly or as input to [WindNinja](https://ninjastorm.firelab.org/windninja/) in order to run Elmfire on diurnal, terrain aware wind simulation output.


The main functions of the `odin_elmfire` crate are:

1. creation of a simulation domain (UTM bounding box and time window) around user provided ignition data
   (location and time)
2. acquisition and computation of ELMFIRE input rasters (fuels, topography, weather, fuel moisture, wind)
3. generation of the ELMFIRE `elmfire.data` input file and execution of the ELMFIRE executable
4. post-processing of ELMFIRE raster output (time of arrival isochrones, fireline intensity, spread rate, flame length,
   crown fire, fire size statistics)
5. async orchestration of above steps in a [`odin_actor`](../odin_actor/odin_actor.md) `ElmfireActor` 
6. micro-service interfacing of this actor through a `ElmfireService` [`odin_server::spa::SpaService`](../odin_server/odin_server.md)
   implementation


## Obtaining and Building ELMFIRE

ELMFIRE is an open sourced external application that is available from [GitHub](https://github.com/lautenberger/elmfire). Building
and installing it is outside of the ODIN realm. Since it is written in Fortran and uses MPI it requires respective platform
specific dependencies that have to be installed through native package managers such as [Homebrew](https://brew.sh/) (on macOS):

```shell
brew install csvkit gfortran jq openmpi pigz virtualenv
```

Please see respective documentation on https://github.com/lautenberger/elmfire for detailed build instructions. As of 09/2026 a typical build process looks like:

```shell
# edit setenv.sh to match your directories
source setenv.sh

cd elmfire/build/linux
./make_gnu.sh
```

Depending on version and platform ELMFIRE builds might require additional steps such as patches.

This installs the ELMFIRE executables into `elmfire/bin`. The build creates versioned executables
(e.g. `elmfire_perf_2025.1002`) - if you want a plain `elmfire` command, add a symbolic link to the respective debug or perf
version. The executable has to be either in your `$PATH` or configured with an absolute path in the `elmfire_cmd` field
of the `odin_elmfire` configuration (see below).

If micro-grid wind is used, a WindNinja installation is also required (see [odin_wind](../odin_wind/odin_wind.md)).


## Crate Modules

- `lib.rs` : data model definitions (`ElmfireConfig`, `ElmFire`, `ElmfireRun`, `ElmfireOutput`) and the functions to create
  the `elmfire.data` input file (from the `templates/elmfire.askama` template), execute ELMFIRE and process its output
- `input.rs` : creation of the ELMFIRE input rasters for a simulation - landfire fuels and topography, weather and NFDRS fuel
  moisture based inputs, fixed inputs, and WindNinja based micro-grid wind
- `fixed.rs` : parsing of fixed (user provided) hourly input steps (`FixedInputStep`)
- `actor.rs` : [odin_actor](../odin_actor/odin_actor.md) based `ElmfireActor` that manages and executes simulation requests
  (`ElmFire` messages) and stores their results in an `ElmfireStore`
- `service.rs` : defines `ElmfireService` - a [odin_server](../odin_server/odin_server.md) based `odin_server::spa::SpaService`
  implementation that makes `ElmfireActor` data available in the context of a `odin_server::spa::SpaServer` single page web
  application server and forwards simulation requests from the browser to the actor
- `errors.rs` : the `odin_elmfire` error definitions and corresponding external error mapping

The diagram below shows how these components are connected:

```
                                                                 odin │ external
    ODIN/                                                             │
      configs/            ┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓      │
        odin_elmfire/     ┃ ElmfireActor                       ┃      │  ┌────────────┐
          elmfire.ron ───►┃                                    ┃   ┌────►│ Open-Meteo │
                          ┃  ┌─────────────────────────┐       ┃   │  │  │   server   │
    ODIN/                 ┃  │ executor task (per run) ┼───────────┤  │  └────────────┘
      cache/              ┃  │  1. create inputs       │       ┃   │  │  ┌────────────┐
        odin_elmfire/  ◄─────┼  2. run ELMFIRE         │       ┃   ├────►│  LANDFIRE  │
          <fire-id>/      ┃  │  3. process outputs     │       ┃   │  │  └────────────┘
            inputs/       ┃  └────────────┬────────────┘       ┃   │  │  ┌────────────┐
            outputs/      ┃               │                    ┃   ├────►│   NFDRS    │
            elmfire.ron   ┃               │ RequestFinished    ┃   │  │  └────────────┘
                          ┃               │                    ┃   │  │  ┌────────────┐
                          ┃               │                    ┃   ├────►│ WindNinja  │
                          ┃  ┌────────────▼────────────┐       ┃   │  │  └────────────┘
                          ┃  │      ElmfireStore       │       ┃   │  │  ┌────────────┐
                          ┃  └─────────────────────────┘       ┃   └────►│  ELMFIRE   │
                          ┃   update_action, remove_action     ┃      │  └────────────┘
                          ┗━━━━━━━━▲━━━━━━━━━━━━━▲━━━━━━━━━│━━━┛      │
                     exec_snapshot │    ElmFire, │         │
                                   │RemoveElmFire│         │
              ┌────────────────────┼─────────────┼─────────┼────┐
              │ SpaServerActor     │             │         │    │
              │  ┏━━━━━━━━━━━━━━━━━┷━━━━━━━━━━━━━┷━━━━━━┓  │    │
              │  ┃            ElmfireService            ┃  │    │
              │  ┗━━━━━━━━━━━━━━━━━┯━━━━━━━━━━━━━▲━━━━━━┛  │    │
              └────────────────────┼─────────────┼─────────┼────┘
                           http:// │             │ wss://  │            server
              ─────────────────────┼─────────────┼─────────┼────────────────────
       elmfire-data/ (png,geojson) │      request│         │updates     client
┌──────────────────────┐   ┌───────▼─────────────┴─────────▼─┐
│odin_elmfire_config.js┼──►│          odin_elmfire.js        │
└──────────────────────┘   └─────────────────────────────────┘
```

All inputs and outputs for ELMFIRE runs are kept in dedicated *simulation-id* directories that follow a 
`❬name❭__❬date❭__❬location❭` encoding scheme: 

```diagram
.
└── ❬odin-root-dir❭/                     $ODIN_ROOT
    ├── cache/  
    │   ├── odin_elmfire/
    │   │   ├── ❬simulation-id❭/
    │   │   │   ├── elmfire.ron          ODIN serialization of `ElmfireRun` objects
    │   │   │   ├── inputs/              ELMFIRE inputs
    │   │   │   │   ├── elmfire.data     Elmfire run config file 
    │   │   │   │   ├── ws.tif           input raster files (GeoTiff)
    │   │   │   │   ├── ...
    │   │   │   ├── outputs/             ELMFIRE outputs & derived data
    │   │   │   │   ├── toa.geojson      output files such as isochrones in GeoJSON format 
    │   │   │   │   ├── ...
    │   │   │   ├── ...
    │   │   ├── ...
```

Each Elmfire input set is defined by a single `elmfire.data` file (in [Fortran Namelist](https://jules-lsm.github.io/vn6.0/namelists/intro.html) format). At minimum, the input rasters required by ELMFIRE are:

- internal inputs (spread rate adjustment `adj.tif` and initial level set `phi.tif`)
- landfire inputs (`fbfm40.tif`, `cbd.tif`, `cbh.tif`, `cc.tif`, `ch.tif`, `slp.tif`, `asp.tif`, `dem.tif`)
- either weather based inputs (Open-Meteo forecasts → NFDRS fuel moisture and wind) or fixed inputs, resulting in the
   hourly banded `m1.tif`, `m10.tif`, `m100.tif`, `lh.tif`, `lw.tif`, `ws.tif` and `wd.tif` rasters

If micro-grid wind is requested (`compute_wind`), WindNinja is run for each hour on a HRRR compatible weather input
file generated in `inputs/wn/` (either from the Open-Meteo forecast grid or from fixed input values), and `ws.tif` / `wd.tif`
are created from the WindNinja output.

The crate also includes the following executables in `src/bin`:

- `show_elmfire` : minimal web application to run ELMFIRE (`SpaServer` with `ElmfireService` and `ElmfireActor`)
- `run_live` : command line tool to create inputs from live data for a point ignition and run ELMFIRE
- `run_fixed` : command line tool to create inputs from a fixed input file and run ELMFIRE
- `process_output` : command line tool to (re-)process ELMFIRE output from a given output directory


### Fixed Inputs

Fixed inputs are specified as Fortran namelist style `key=val,..` lines with one value per hour. If a line has fewer values
than simulation hours, its last value is repeated:

```
ws=10,12,15        ! 20ft wind speed [mph]
wd=270,280,290     ! wind direction (where the wind is coming from) [deg]
m1=4               ! 1h dead fuel moisture [%]
m10=5              ! 10h dead fuel moisture [%]
m100=7             ! 100h dead fuel moisture [%]
lh=60              ! live herbaceous fuel moisture [%]
lw=90              ! live woody fuel moisture [%]
tcc=10             ! total cloud cover [%] - only used for WindNinja
t2=85              ! 2m temperature [F] - only used for WindNinja
```

Each value is either a single number (interpreted as a 1 item list) or a list of comma separated numbers. This list is automatically extended to the selected duration of the simulation in hours (plus 1 step for the full hour preceding the first ignition) by replicating the last entered value.
Different variables do not need to have the same value lengths. In the above case `ws` uses 10mph for the first step, 12mph for the 1h simulation step and 15mph for the rest of the required simulation steps.

These variables are not directly used/passed to Elmfire (as opposed to other interactive input) but used by ODIN to generate gridded input data (geotiffs) for Elmfire. If "compute wind" is not selected (i.e. we don't want to simulate using computed micro-grid wind) the `tcc` and `t2` variables are not required. 


## Configuration

`odin_elmfire` uses a `ElmfireConfig` file (normally `$ODIN_ROOT/configs/odin_elmfire/elmfire.ron`) that refers to the
configurations of the other ODIN crates it uses for input acquisition:

```rust,ignore
ElmfireConfig (
    lf_src_name: "lf_2025_conus.ron",         // filename of odin_landfire source to use (->odin_gdal::GeoDs)
    lf_res_m: 30,                             // landfire pixel resolution in meters

    nfdrs_config_name: "nfdrs.ron",           // odin_nfdrs::NfdrsConfig filename (loaded through odin_nfdrs)
    nfdrs_past_days: 6,                       // history length for fuel moisture computation (>100h)

    wx_config_name: "openmeteo.ron",
    wx_res_m: 3000,                           // meters per pixel for Elmfire weather input datasets
    wx_model: "ncep_hrrr_conus",              // the weather model to use

    bbox_size_m: 24000,                       // edge size of UTM bounding box for simulation in meters (even multiple of wx_res_m)

    wind_config_name: "wind.ron",             // odin_wind::WindConfig filename in case we compute wind
    wind_res_m: 150,                          // micro-grid wind resolution in meters

    input_dt: Duration(secs: 3600, nanos:0),  // the time step for timed input data bands (1h increments)
    small_fire_cells: 4000,                   // final perimeters below this number of cells need contour pre-processing

    elmfire_cmd: "elmfire",                   // has to be in the $PATH if not absolute (e.g. using $ODIN_ROOT)

    // colorization levels for generated PNG image output
    toa_colors:  [0xff000000, 0xff000040, 0xff000080, 0xa00000ff, 0xff0000ff, 0xff4500ff, 0xffa500ff, 0xffd700ff, 0xffff00ff],
    flin_colors: [ ... ],
    vs_colors:   [ ... ],
    cf_colors:   [ ... ],
    fl_colors:   [ ... ]
)
```


## Example

This is the `show_elmfire` application that uses an `ElmfireActor` and a `SpaServer` with a single `ElmfireService` micro
service layer to define, run and display ELMFIRE simulations on top of a virtual globe in a single page web application:

```rust,ignore
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_elmfire::{ElmfireService, actor::{ElmfireActor,ElmfireActorMsg, server_update_action, server_remove_action}, load_config};

run_actor_system!( actor_system => {
    let pre_fire = PreActorHandle::new( &actor_system, "elmfire", 8);

    let hserver = spawn_actor!( actor_system, "server",
        SpaServer::new(
            odin_server::load_config("spa_server.ron")?,
            "elmfire",
            SpaServiceList::new()
                .add( build_service!( let hfire: ActorHandle<ElmfireActorMsg> = pre_fire.to_actor_handle() => ElmfireService::new( hfire) ))
        )
    )?;

    let hfire = spawn_pre_actor!( actor_system, pre_fire,
        ElmfireActor::new(
            load_config("elmfire.ron")?,
            server_update_action( hserver.clone()),
            server_remove_action( hserver.clone())
        )
    )?;

    Ok(())
});
```

Simulations can also be run from the command line, e.g.

```shell
cargo run --bin run_live -- --fc-hours 12 --compute-wind MyFire -121.95 37.25               # <name> <lon> <lat>
cargo run --bin run_fixed -- --fc-hours 6 MyFire 37.25 -121.95 my_fixed_inputs.nml          # <name> <lat> <lon> <fixed-file>
```

## Web Application User Interface

The `odin_elmfire` crate has a single [UI window](../odin_server/client.md) to control visualization and data input. This window contains two main panels: "simulations" to view completed runs and "input" to enter and start new runs.

<img class="left" src="../img/odin_elmfire-ui.png" width="40%">

The **simulations** panel shows a list of currently known completed Elmfire simulations. The "toa" checkbox in each line can be used to load and show the 30min isochrones for the selected run. The "show img" choice menu to the right of it can display associated images generated from Elmfire output (e.g. "flin" for fire line intensity). Selecting a simulation automatically populates the rest of the UI window with the data for this simulation. The key/value table to the left underneath the simulation list shows statistics for the selected run, the list to the right shows the time, growth and area for each 30min interval. Selecting a time step entry fills the respective isochrone polygon displayed on the virtual globe.

The **input** panel contains all the potential data required to create a new Elmfire simulation. The "pick now" button allows to quickly select an ignition point on the virtual globe and use the current time as the ignition time. This will populate the "loc" and "dtg" entries underneath (which can be changed subsequently). Before a new simulation request can be submitted by clicking the "simulate" button the user has to enter a respective name for the simulation. The "comp wind" checkbox controls if either actual or entered wind data is used directly or WindNinja output generated from it should be the input for Elmfire. The main part of this panel is a tab container for the different types of Elmfire input sources (using Fortran Namelist format). Each tab has a row underneath its source editor to create respective sources from chosen/default values.
