# odin_nfdrs

The `odin_nfdrs` crate is an application domain crate to compute and display fuel moisture and fire danger indices of the
US National Fire Danger Rating System (NFDRS). NFDRS was originally defined in

- J.E. Deeming, R.E. Burgan, J.D. Cohen: [*The National Fire-Danger Rating System - 1978*](https://fs.usda.gov/rm/pubs_series/int/gtr/int_gtr039.pdf),
  USDA Forest Service, Intermountain Forest and Range Experiment Station, General Technical Report INT-39, 1977

`odin_nfdrs` uses the current version 4 of the system (NFDRS4), which simplifies fuel models and improves live and dead fuel
moisture calculations. NFDRS4 is described in

- W.M. Jolly, P.H. Freeborn, L.S. Bradshaw, J. Wallace, S. Brittain: [*Modernizing the US National Fire Danger Rating System
  (version 4): Simplified fuel models and improved live and dead fuel moisture calculations*](https://doi.org/10.1016/j.envsoft.2024.106181),
  Environmental Modelling & Software, vol. 181, 106181, 2024

The NFDRS4 model is implemented by the open source [NFDRS4](https://github.com/firelab/NFDRS4) library and its `NFDRS4_cli`
command line application from the USDA Forest Service [Missoula Fire Sciences Laboratory](https://www.firelab.org/). As with
other 3rd party models, `odin_nfdrs` does not re-implement NFDRS but creates the inputs for `NFDRS4_cli`, executes it and
processes its output.

NFDRS4 computes its outputs for a single (weather station) location from

- hourly weather observations in FW21 format (temperature, relative humidity, precipitation, wind
  speed and direction, solar radiation, snow flag), which have to include enough history (>100h) for dead fuel moisture
  models. `odin_nfdrs` obtains past and forecast hourly weather from [Open-Meteo](https://open-meteo.com/) through
  [odin_openmeteo](../odin_openmeteo/odin_openmeteo.md)
- the average annual precipitation of the location (also obtained from the Open-Meteo archive)
- static location properties: latitude, slope class and NFDRS fuel model, which are derived from
  [LANDFIRE](https://www.landfire.gov/) data through [odin_landfire](../odin_landfire/odin_landfire.md)

To compute area (grid) data `odin_nfdrs` runs NFDRS4 for each point of a regular grid that is aligned with the
weather model resolution (e.g. 3km for HRRR), resulting in the following data flow (solid lines (`──►`) denote dynamic
inputs that change with each computation, dotted lines (`┄┄►`) denote (semi) static inputs):

```
   external          ODIN node
   (servers)         (executables orchestrated by ODIN)
                  ╎
┌──────────┐      ╎  ╭──────────╮
│ landfire │────────►│  cache   │┄┄┄┄┄┄┄┄┄┄┄┄┐
└──────────┘      ╎  ╰──────────╯            ┆ - slope class
                  ╎                          ┆ - fuel model
                  ╎                          ┆
┌──────────┐      ╎  ┌──────────┐      ┏━━━━━▼━━━━━┓      ┌──────────┐     ┌──────────┐
│Open-Meteo│────────►│ FW21 wx  │─────►┃   NFDRS4  ┃─────►│ datasets │────►│ ODIN app │
└──────────┘      ╎  │annual pcp│      ┃(per point)┃      │ tif, png │     └──────────┘
(per region)      ╎  └──────────┘      ┗━━━━━┯━━━━━┛      └──────────┘
   - hourly wx    ╎                          │
     (past+fc)    ╎                          │ fuel moisture
   - daily precip ╎                          ▼
     (past year)  ╎                   ┌─────────────┐
                  ╎                   │ odin_elmfire│
                  ╎                   └─────────────┘
```

Note that Open-Meteo queries are performed per region (i.e. using the bounding box), which limits the size of the
bounding boxes based on the wx model that is used (Open-Meteo limits queries to ~1000 location points). NFDRS4 has to be run for each grid cell (point) within this region.

The main functions of the `odin_nfdrs` crate are:

1. creation of NFDRS4 inputs (FW21 weather data, location classification, configuration) for given locations and time windows
2. execution of `NFDRS4_cli` and parsing of its output records (`NfdrsRecord`)
3. computation of hourly fuel moisture and fire danger index grids for given areas, including GeoTIFF datasets
   (EPSG:4326), color mapped PNG images and area statistics (min/max/avg per hour)
4. async, queued processing of grid computation requests in a [`odin_actor`](../odin_actor/odin_actor.md) `NfdrsActor`
5. micro-service interfacing of this actor through a `NfdrsService` [`odin_server::spa::SpaService`](../odin_server/odin_server.md)
   implementation

Point based fuel moisture records are also used by other crates such as [odin_elmfire](../odin_elmfire/odin_elmfire.md) to
create fuel moisture inputs for fire behavior simulation.

The following `NfdrsRecord` fields are supported as grid products:

| field       | description                                  | unit |
|-------------|----------------------------------------------|------|
| `dfm_1h`    | 1h dead fuel moisture                        | %    |
| `dfm_10h`   | 10h dead fuel moisture                       | %    |
| `dfm_100h`  | 100h dead fuel moisture                      | %    |
| `lfm_herb`  | live herbaceous fuel moisture                | %    |
| `lfm_woody` | live woody fuel moisture                     | %    |
| `bi`        | burning index                                |      |
| `erc`       | energy release component                     |      |
| `sc`        | spread component                             |      |
| `ic`        | ignition component                           |      |
| `gsi`       | growing season index                         |      |
| `kbdi`      | Keetch-Byram drought index                   |      |


## Obtaining and Building NFDRS4

NFDRS4 is an open source C++ library and command line application that is available from
[GitHub](https://github.com/firelab/NFDRS4). Building and installing it is outside of the ODIN realm - please see the
respective documentation. The build requires a C++ toolchain and [CMake](https://cmake.org/), which can be installed
through native package managers such as [Homebrew](https://brew.sh/) (on macOS). As of 10/2026 a typical build process on
macOS looks like:

```shell
brew install cmake

git clone https://github.com/firelab/NFDRS4.git
cd NFDRS4

# build the bundled config4cpp dependency first
cd extern/config4cpp
make
cd ../..

# configure & build (might require setting CONFIG4CPP_DIR and CONFIG4CPP_LIB in CMakeCache.txt
# to point to extern/config4cpp/include and extern/config4cpp/lib/libconfig4cpp.a)
cmake .
make
```

This creates the `bin/NFDRS4_cli` executable, which has to be either in your `$PATH` or configured with an absolute path in the `nfdrs_cmd` field of the `odin_nfdrs` configuration (see below). Depending on version and platform NFDRS4 builds might require additional steps such as patches.

NFDRS examples can be found in the separate [NFDRS4-TechDoc](https://github.com/firelab/NFDRS4-TechDoc) repository.


## Crate Modules

- `lib.rs` : data model definitions (`NfdrsConfig`, `Fw21Record`, `NfdrsRecord`, `NfdrsLocationClassification`) and the
  functions to create NFDRS4 inputs (from the `templates/nfdrs_config.askama` and `templates/nfdrs_init.askama` templates),
  execute `NFDRS4_cli` and parse its output for single locations (`compute_nfdrs_recs()`)
- `grid.rs` : computation of hourly NFDRS grid datasets and PNG images for areas (`create_nfdrs_grid_datasets()`,
  `create_interpolated_field_pngs()`), the supported fields and their color maps (`NfdrsField`), and the request/result types
  used by the actor (`NfdrsRequest`, `NfdrsRun`, `NfdrsOutput`)
- `actor.rs` : [odin_actor](../odin_actor/odin_actor.md) based `NfdrsActor` that queues and executes grid computation requests
  (`NfdrsRequest` messages) and stores their results in a `NfdrsStore`
- `service.rs` : defines `NfdrsService` - a [odin_server](../odin_server/odin_server.md) based `odin_server::spa::SpaService`
  implementation that makes `NfdrsActor` data available in the context of a `odin_server::spa::SpaServer` single page web
  application server and forwards computation requests from the browser to the actor
- `errors.rs` : the `odin_nfdrs` error definitions and corresponding external error mapping

The diagram below shows how these components are connected:

```
                                                                 odin │ external
    ODIN/                                                             │
      configs/            ┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓      │
        odin_nfdrs/       ┃ NfdrsActor (queued runs)           ┃      │  ┌────────────┐
          nfdrs.ron ─────►┃                                    ┃   ┌────►│ Open-Meteo │
                          ┃  ┌─────────────────────────┐       ┃   │  │  │   server   │
    ODIN/                 ┃  │ executor task (per run) ┼───────────┤  │  └────────────┘
      cache/              ┃  │  1. get wx, precip      │       ┃   │  │  ┌────────────┐
        odin_nfdrs/    ◄─────┼  2. run NFDRS4/point    │       ┃   ├────►│  LANDFIRE  │
          grids/          ┃  │  3. create tif, png     │       ┃   │  │  └────────────┘
            <run-id>/     ┃  └────────────┬────────────┘       ┃   │  │  ┌────────────┐
              *.json *.tif┃               │                    ┃   └────►│   NFDRS4   │
              *.png       ┃               │ RunFinished        ┃      │  └────────────┘
                          ┃               │                    ┃      │
                          ┃               │                    ┃      │
                          ┃  ┌────────────▼────────────┐       ┃      │
                          ┃  │       NfdrsStore        │       ┃      │
                          ┃  └─────────────────────────┘       ┃      │
                          ┃   update_action, remove_action     ┃      │
                          ┗━━━━━━━━▲━━━━━━━━━━━━━━━▲━━━━━━━│━━━┛      │
                     exec_snapshot │ NfdrsRequest, │       │
                                   │RemoveNfdrsRun │       │
              ┌────────────────────┼───────────────┼───────┼────┐
              │ SpaServerActor     │               │       │    │
              │  ┏━━━━━━━━━━━━━━━━━┷━━━━━━━━━━━━━━━┷━━━━┓  │    │
              │  ┃             NfdrsService             ┃  │    │
              │  ┗━━━━━━━━━━━━━━━━━┯━━━━━━━━━━━━━━━▲━━━━┛  │    │
              └────────────────────┼───────────────┼───────┼────┘
                           http:// │               │wss:// │            server
              ─────────────────────┼───────────────┼───────┼────────────────────
              nfdrs-data/ (png)    │       request │       │updates     client
┌──────────────────────┐   ┌───────▼───────────────┴───────▼─┐
│ odin_nfdrs_config.js ┼──►│           odin_nfdrs.js         │
└──────────────────────┘   └─────────────────────────────────┘
```

Since NFDRS4 is executed once per grid point (and grids of overlapping requests can share NFDRS4 input and output files)
the `NfdrsActor` only computes one run at a time - additional requests are queued. The grid resolution is derived from the
configured weather model (e.g. 3km for `ncep_hrrr_conus`) since there is no point in computing more NFDRS points than the
weather model supports. The pixel resolution of the generated datasets is configured separately (see below). Generated
datasets are resampled (cubic spline) from the computational grid and therefore have a continuous appearance.

All products of a run are kept in a dedicated *run-id* directory that follows a
`❬name❭__❬date❭__❬bbox❭__❬grid-res❭__❬hours❭` encoding scheme:

```diagram
.
└── ❬odin-root-dir❭/                     $ODIN_ROOT
    ├── cache/
    │   ├── odin_nfdrs/
    │   │   ├── grids/
    │   │   │   ├── ❬run-id❭/
    │   │   │   │   ├── nfdrs_run.json   ODIN serialization of `NfdrsRun` objects (request and results)
    │   │   │   │   ├── fw21__*.json     cached Open-Meteo weather data for the computation grid
    │   │   │   │   ├── annual_precip__*.json  cached Open-Meteo annual precipitation for grid points
    │   │   │   │   ├── erc.tif          hourly banded GeoTIFF datasets (one per field, EPSG:4326)
    │   │   │   │   ├── erc_0.png        color mapped images (one per field and hour)
    │   │   │   │   ├── ...
    │   │   │   ├── ...
    │   │   ├── nfdrs_conf__*.cfg    NFDRS4 inputs and outputs for single locations
    │   │   ├── nfdrs_init__*.cfg
    │   │   ├── fw21__*.csv
    │   │   ├── nfdrs_output__*.csv
    │   │   ├── ...
```

The crate also includes the following executables in `src/bin`:

- `show_nfdrs` : minimal web application to compute and display NFDRS grids (`SpaServer` with `NfdrsService` and `NfdrsActor`)
- `nfdrs_grid` : command line tool to compute NFDRS grid datasets (and optionally PNG images) for a given bounding box
- `run_nfdrs` : command line tool to create NFDRS4 inputs for a single location and run NFDRS4
- `get_fw21` : command line tool to retrieve FW21 weather data for a single location
- `get_annual_precipitation` : command line tool to retrieve the annual precipitation for a single location


### Color Maps

Each `NfdrsField` has an associated color map with 8 classes (from "extremely dry" to "very wet" for fuel moisture, and
from "very low" to "extreme" for indices). Color maps are sent to clients so that they can be displayed as legends.
PNG images are created with `create_interpolated_field_pngs()`, which linearly interpolates colors between class colors that
are anchored at the class centers (i.e. values at class centers have the exact legend color). Alternatively
`create_field_pngs()` creates images with discrete class colors.

Note that the current class thresholds (`NfdrsField::thresholds()`) are generic values that have not yet been validated
for operational use.


## Configuration

`odin_nfdrs` uses a `NfdrsConfig` file (normally `$ODIN_ROOT/configs/odin_nfdrs/nfdrs.ron`):

```rust,ignore
NfdrsConfig (
    nfdrs_cmd: "NFDRS4_cli", // has to be absolute or in $PATH
    pixel_res_m: 250.0,      // pixel resolution of generated grid datasets in meters (approximated for EPSG:4326)
)
```

The weather source ([odin_openmeteo](../odin_openmeteo/odin_openmeteo.md) config and weather model) and the LANDFIRE
source ([odin_landfire](../odin_landfire/odin_landfire.md) config) are passed in separately when creating a `NfdrsActor`.


## Example

This is the `show_nfdrs` application that uses a `NfdrsActor` and a `SpaServer` with a `NfdrsService` micro service layer
to request and display NFDRS grids on top of a virtual globe in a single page web application. It also uses a
`ShareService` so that users can select shared areas of interest:

```rust,ignore
use std::str::FromStr;
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_share::prelude::*;
use odin_openmeteo::{self, ids::ModelId};
use odin_landfire;
use odin_nfdrs::{NfdrsService, NfdrsActor, load_config, actor::{server_update_action, server_remove_action}};

run_actor_system!( actor_system => {
    let pre_server = PreActorHandle::new( &actor_system, "server", 64);
    let pre_nfdrs = PreActorHandle::new( &actor_system, "nfdrs", 8);

    // spawn a shared store actor so that we can use shared areas of interest
    let hshare = spawn_server_share_actor(&mut actor_system, "share", pre_server.to_actor_handle(), default_shared_items(), false)?;

    let hnfdrs = spawn_pre_actor!( actor_system, pre_nfdrs,
        NfdrsActor::new(
            load_config( "nfdrs.ron")?,
            odin_openmeteo::load_config( "openmeteo.ron")?,
            ModelId::from_str( "ncep_hrrr_conus")?,
            odin_landfire::load_config( "lf_2025_conus.ron")?,
            server_update_action( pre_server.to_actor_handle()),
            server_remove_action( pre_server.to_actor_handle())
        )
    )?;

    let _hserver = spawn_pre_actor!( actor_system, pre_server, SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "nfdrs",
        SpaServiceList::new()
            .add( build_service!( let hnfdrs = hnfdrs.clone() => NfdrsService::new( hnfdrs) ))
            .add( build_service!( let hshare = hshare.clone() => ShareService::new( "odin_share_schema.js", hshare)) )
    ))?;

    Ok(())
});
```

Grids can also be computed from the command line, e.g.

```shell
cargo run --bin nfdrs_grid -- --hours 6 --png --dir /tmp/nfdrs -- -122.30 37.23 -122.09 37.37     # <west> <south> <east> <north>
cargo run --bin run_nfdrs -- --show-result 37.25 -121.95                                          # <lat> <lon>
```


## Web Application User Interface

The `odin_nfdrs` crate has a single [UI window](../odin_server/client.md) with three panels: "input" to define and request
new NFDRS runs, "data" to view requested/completed runs and their hourly products, and "color map" to show the legend of
the displayed product.

<img class="left" src="../img/odin_nfdrs-ui.png" width="40%">

The **input** panel shows a tree of shared areas (bounding boxes) at the top. Selecting a shared area or picking an area on
the virtual globe ("pick" button) populates the "area" entry, which can also be edited directly (west,south,east,north in
degrees). The "show" checkbox controls if the area outline is displayed on the globe, "zoom" moves the view to the area.
New runs are requested by clicking the "compute" button after entering a run "name", the base "date" (which is rounded to the
full hour) and the number of forecast "hours". The "update" checkbox marks requests that should be re-computed when new
forecasts become available (not yet implemented). Selecting an entry in the "data" panel populates the input fields with the
values of the selected run.

The **data** panel contains a list of requested and completed runs. Each entry shows the run status (pending, completed or
failed), its name, base date, number of hours and summary values for the whole forecast duration (minimum 1h and 10h dead
fuel moisture, maximum Keetch-Byram drought index and ignition component). Selecting a run populates the tab container
underneath, which holds a tab for each supported NFDRS variable. Each tab contains a list with one entry per forecast hour, showing the minimum, maximum and average value within the area. If the "show" checkbox to the left of the tabs is set, the grid image for the selected hour of the showing tab is displayed on the virtual globe (only one image at a time). The selected hour is kept when switching tabs or runs, which allows to compare different variables for the same forecast hour. The "hide all" button removes the displayed image, the context menu of the run list can be used to remove runs. Double clicks on a NFDRS run zooms in on the respective bounding box on the virtual globe (Cesium display).

The **color map** panel shows the color classes of the selected NFDRS variable together with their value ranges and
descriptions.
