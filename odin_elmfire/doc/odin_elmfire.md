# odin_elmfire

The `odin_elmfire` crate is an application domain crate to run and display wildfire spread simulations with
[ELMFIRE](https://elmfire.io/) (Eulerian Level set Model of FIRE spread), an open source fire behavior model developed by
[Chris Lautenberger](https://github.com/lautenberger/elmfire). ELMFIRE is an external executable - `odin_elmfire` does
not re-implement the model but automates the creation of its inputs, runs the executable and post-processes its
outputs for visualization.

## Use Case

The primary use case for `odin_elmfire` is the quick assessment of new and emerging incidents over a limited time window (<8h), using
live fuel data and live or virtual ignition- and/or weather data. Both should support interactive "what if" scenarios such as spotting
or fuel-breaks and retardant drops and especially should support the use of local *micro-grid wind* computed by 
[`odin_wind`](../odin_wind/odin_wind.md) by means of the external [WindNinja](https://ninjastorm.firelab.org/windninja/) simulator.

As the end point we envision an automated processing pipeline that detects and locates ignitions from ground-, aerial- or 
space-sensors that automatically trigger predicted fire spread for up to 8h, and then allow operators to do interactive threat 
assessment and counter-measure planning such as air attack. Eventually this should include the wildland urban interface. 
With `odin_elmfire` we are not focused on simulating large, multi-shift incidents. 

ELMFIRE seems particularly well suited for this task as it - as a non-coupled model - does not require to set up a local weather 
model and hence lends itself to quick, interactive use on local servers. We envision future support for expensive, coupled 
fire-behavior and weather- simulators in dedicated crates.


## Dataflow and Core Functions

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
- `fixed.rs` : fixed (user provided) hourly wx and fuel moisture input values (`FixedInput`), including NML parsing for command line tools
- `barrier.rs` : mapping of user defined barriers (fuel breaks and air drops) to ELMFIRE inputs (see [Fuel Breaks and Air Drops](#fuel-breaks-and-air-drops))
- `toa.rs` : post processing of time-of-arrival output (isochrone line contours and cumulative burn area polygons)
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
    │   │   │   │   ├── toa.geojson      isochrone outlines (GeoJSON LineStrings with 'level' property)
    │   │   │   │   ├── toa_poly.geojson cumulative burn areas per level (GeoJSON MultiPolygons with holes)
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

Fuel moisture and wind inputs are computed with their own resolution (`wx_res_m` for fixed and NFDRS based inputs,
`wind_res_m` for WindNinja output). Current ELMFIRE builds do not handle different raster resolutions of their inputs
(they use the wind rasters as reference grid). If `uniform_res` is set (default) these inputs are therefore resampled
to the landfire grid (UTM bounding box and `lf_res_m`) after they are created - wind direction with nearest neighbour
(to avoid interpolation across the 0/360 degree discontinuity), all others with cubic interpolation. Resampled datasets
are compressed but still need considerably more disk space than the original (coarse) datasets.

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
    uniform_res: true,                        // resample computed (fuel moisture, wind) inputs to lf_res_m (see below)

    input_dt: Duration(secs: 3600, nanos:0),  // the time step for timed input data bands (1h increments)
    small_fire_cells: 4000,                   // fires with less burned cells get up-sampled before contouring

    fuel_break_fbfm40: 99,                    // fuel model for fuel breaks at least lf_res_m wide (default NB9: bare ground)
    drop_agents: [                            // air drop agents and their fuel moisture effects (see below)
        ( name: "LTR", moisture: [
            ( raster: "m1",  increase: Set(60.0), decay: Linear(24.0) ),
            ( raster: "m10", increase: Add(15.0), decay: Linear(24.0) ),
        ]),
        ( name: "water", moisture: [ ... ]),
    ],

    elmfire_cmd: "elmfire",                   // has to be in the $PATH if not absolute (e.g. using $ODIN_ROOT)

    // colorization levels for generated PNG image output
    toa_colors:  [0xff000000, 0xff000040, 0xff000080, 0xa00000ff, 0xff0000ff, 0xff4500ff, 0xffa500ff, 0xffd700ff, 0xffff00ff],
    flin_colors: [ ... ],
    vs_colors:   [ ... ],
    cf_colors:   [ ... ],
    fl_colors:   [ ... ]
)
```


## Fuel Breaks and Air Drops

Users can enter barriers in the "barriers" tab of the UI: hand lines and dozer lines (line strings), SEAT/LAT/VLAT
air drops (lines with a width) and type 1-3 helicopter bucket drops (ellipses). ELMFIRE itself only supports barriers through a single band `barriers.tif` raster
that holds the barrier width [m] for each cell (note the ELMFIRE documentation does not mention units, but the
source - `CHECK_BARRIER_BREACH` in `elmfire_subs.f90` - uses meters). A barrier cell still burns, but the fire only
spreads from it if `1.5 * flame length` exceeds the barrier width. Flame length includes canopy fire intensity (active
crown fire uses `2.5 * canopy height`), i.e. this is the "a fuel break has to be at least 1.5 times as wide as the flame
length" rule, which is intensity aware but has no time dimension (barriers are in effect for the whole simulation).
This fits fuel breaks but not air drops - retardant does not remove fuel and drops happen at a specific time.

We therefore map barriers to ELMFIRE inputs as follows (see `barrier.rs`):

| barrier | ELMFIRE input |
|---|---|
| fuel break narrower than `lf_res_m` (hand line, dozer line) | `barriers.tif` with the real width [m] |
| fuel break at least `lf_res_m` wide | `fbfm40.tif` set to `fuel_break_fbfm40` (non-burnable) |
| air drop (barrier with drop agent, line or bucket drop ellipse) | increase of fuel moisture inputs (`m1`, `m10`, ..) from the drop time step on |

### Fuel breaks

Narrow fuel breaks (e.g. 2ft hand lines, 12ft dozer lines) would be grossly exaggerated by setting whole 30m cells to
non-burnable fuel, which would also make them impenetrable for anything but spotting. They are written to `barriers.tif`
with their real width in meters, i.e. ELMFIRE breaches them once the flame length exceeds 2/3 of the line width. All
cells that are touched by the fuel break area (line plus width) are set, which avoids diagonal-only connections between
barrier cells. This is important since ELMFIRE uses a narrow band of only one cell when barriers are enabled - the level
set front could otherwise pass between two diagonally adjacent barrier cells without having to breach the barrier.
Crossing fuel breaks use the max width.

<img class="mono right" src="../img/elmfire-barrier-gaps.svg" width="40%"/>

The diagram shows the problem: when a cell burns, ELMFIRE tags its 3x3 neighborhood (dashed) for front advancement.
Barrier cells only stop the front from spreading further out of themselves. If a barrier line is rasterized with
Bresenham's algorithm (left), consecutive barrier cells only touch at their corners. A burning cell `B` next to such a
corner then tags cell `T` on the other side of the barrier without the front ever having to burn (and breach) a barrier
cell. Using all cells that are touched by the barrier area (right) adds cells that close these diagonal gaps - no
burning cell on one side of the barrier has a neighbor on the other side.

Fuel breaks that are at least as wide as a landfire cell (wide dozer lines, ridge top fuel breaks, clear cuts) remove
fuel at the model resolution and are modeled by setting the fuel model of all cells whose center is inside of the fuel
break to `fuel_break_fbfm40` (NB9 by default). We use a min width of `sqrt(2) * lf_res_m` for this to avoid diagonal
gaps. Non-burnable cells do not have surface fire and hence also no crown fire.

Since both inputs are single band rasters fuel breaks are in effect for the whole simulation (the UI does not allow
to set a time step for them).

### Air drops

Air drops are barriers with a drop agent (e.g. long-term retardant "LTR" or "water"). They are modeled as a time
dependent increase of fuel moisture inputs, which affects ELMFIRE as follows:

- `m1` (1h dead fuel moisture) is used in the Rothermel surface spread model (also for the dynamic dead fuel class),
  surface spread stops if dead fuel moisture exceeds the moisture of extinction of the fuel model. It also reduces the
  crown fire spread rate (Cruz: `exp(-0.17 * M1[%])`) and - by reducing surface fire intensity - crown fire initiation
- `m10`, `m100` only affect surface spread, `lh`/`lw` only live fuel

Spot fires are still ignited in drop cells (the ember ignition probability does not depend on fuel moisture), but they
spread slowly from there.

Drop agents are configured in `ElmfireConfig.drop_agents`. Each agent has a list of moisture modifications that specify
the input raster, the initial increase for fully covered cells and how this increase decays (down to the forecast value):

```rust,ignore
( name: "LTR", moisture: [
    ( raster: "m1",  increase: Set(60.0), decay: Linear(24.0) ),      // raise to 60%, linear decay over 24h
    ( raster: "m10", increase: Add(15.0), decay: Linear(24.0) ),      // add 15 percentage points
]),
( name: "water", moisture: [
    ( raster: "m1",  increase: Set(40.0), decay: Exponential(0.5) ),  // half life of 30min
    ...
```

- `increase`: `Add(d)` adds `d` percentage points, `Set(v)` raises the moisture to `v` percent (no change if the forecast
  value is already higher)
- `decay`: `Constant`, `Linear(hours)` or `Exponential(half_life_hours)`

The configured values are initial estimates that still need calibration. Note the decay depends on the agent - long-term
retardant keeps its effect after drying (until it is washed out or burned), water only until it evaporates.

The increase of each cell is scaled by the fraction of the cell that is covered by the drop area (line plus width), i.e.
drops are rendered anti-aliased. This avoids diagonal gaps and represents drops that are narrower than a cell (a 90ft LAT
drop covers at most ~90% of a 30m cell). Overlapping drops use the max increase. Drop footprints require the landfire grid
resolution, i.e. the modified moisture inputs are always resampled to the landfire grid (even if `uniform_res` is not set).

Since the moisture inputs have hourly bands, drops are entered with a time step (hours after the full hour that precedes
the first ignition, which is the time of the first band). The increase starts with the band of the selected time step.
The UI time step choice should be the full hour that precedes the actual drop time - ELMFIRE interpolates linearly between
bands, i.e. the full effect is reached at the full hour and the increase ramps up during the hour before. This means the
effect can start up to about one hour early, but it preserves the drop effect if the fire front arrives shortly after the
drop. Using the following hour could miss the front. Users can use the isochrone/time-of-arrival output of a previous run
to determine how much time there is for a drop.

### Bucket drops

Helicopter bucket drops produce elliptical ground patterns. They are entered like air drops with a line - the first
point is the start of the drop and the second point only determines the direction (heading of the major axis). The
pattern size is defined by the bucket type (`bucketPatterns` in `odin_elmfire_config.js`), the resulting
`odin_common::geo::GeoEllipse` (main.js `GeoEllipse`) is sent as the barrier value, i.e. the server does not need to
know bucket types and stored runs keep their actual drop patterns.

<img class="mono right" src="../img/elmfire-bucket-drop.svg" width="50%"/>

The ellipse starts at the first point and extends in the direction of the second point, i.e. its center is half the
pattern length ahead of the drop start and its major axis has the heading of the entered line. The distance to the
second point is not used.

The default pattern sizes are estimates for water at a coverage level of about 1 gal/100sqft. They are based on drop
tests of the Griffith 100 gal helibucket (USDA Forest Service MTDC report 0057-2829 "Ground Pattern Performance of the
Griffith Big Dipper Model 100 Helibucket", tested with 70 gal water: the longest line at coverage level 1 is 157ft,
i.e. a pattern area of ~7000sqft) and scaled with the square root of the bucket volume:

| bucket type | volume | pattern (length x width) |
|---|---|---|
| `bucket-T3` (type 3 helicopter) | 100 gal | 190ft x 70ft |
| `bucket-T2` (type 2 helicopter) | 400 gal | 375ft x 135ft |
| `bucket-T1` (type 1 helicopter) | 1000 gal | 590ft x 215ft |

Since the footprint is sized for a constant coverage level, larger buckets cover larger areas with the same effect per
covered cell (we do not use a per bucket type effectiveness factor). Actual patterns depend on drop height, air speed,
wind and terrain. Bucket drops default to the "water" agent, which with its 30min half life only has a significant effect
within the hour following the drop time step (the moisture inputs have hourly bands). Small patterns (type 3) cover only
a fraction of a 30m cell and hence only reduce spread within that cell.

### Limitations

- fuel breaks are static - lines that are only planned are in effect from the beginning of the simulation
- air drop timing has a one hour resolution
- the drop effect does not depend on fire intensity (e.g. faster evaporation or consumption of retardant ahead of an
  intense fire), only on coverage, time and agent - except that air drops reduce the flame length within drop cells
- moisture increase and decay values are estimates that need calibration with observed retardant effectiveness

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

The `odin_elmfire` crate has a single [UI window](../odin_server/client.md) to control visualization and data input. This
window contains three panels: "simulations" to view completed runs, "input" to enter and start new runs, and "display
parameters" to control how isochrones are rendered. The user interface serves two purposes:

- to assess progression of a single run - this is mostly aimed at real incidents with a single set of input types
- to compare "what if" scenarios - this looks at differences for runs based on input variations, i.e. shows (subsets
  of) the data for multiple runs at the same time.

Those purposes are reflected in the two selection modes described at the end of this section. 

<img class="left" src="../img/odin_elmfire-ui.png" width="40%">

The **simulations** panel shows a list of currently known Elmfire simulations (runs). Each line shows the processing state,
name and date of a run, plus a "show" checkbox that controls if the isochrone outlines of this run are displayed. Selecting a
run populates the rest of the UI window with the data for this run: the key/value table underneath the run list shows
statistics for the selected run, and the timestep list to the right shows the time, growth and area for each 30min interval.
Each timestep line has its own "⏿" checkbox to request the display of the respective isochrone outline. The column to the
right of the run list holds the "show sel" checkbox (see below), the "img" choice to display images generated from Elmfire
output (e.g. "flin" for fire line intensity), the "wnd" choice for wind output (not yet implemented), and the "∅" button.

Isochrones are displayed as outlines (polylines) of the fire area at the end of each 30min interval. Selecting a timestep
in the timestep list highlights its isochrone by filling the respective polygon. There is at most one filled polygon at a
time - the selected timestep of the selected run - and fills are only shown together with their (visible) outline.
Isochrone data is loaded on demand the first time any isochrone of a run has to be shown.

The isochrone display has two selection modes, which are controlled by the "show sel" checkbox:

- **show selected run** ("show sel" checked, default): selecting a run in the run list shows all isochrone outlines of
  this run, and only of this run. The "show" checkbox of the selected run is set automatically, isochrones of a previously
  selected run are removed.

- **show requested timesteps** ("show sel" not checked): only explicitly requested timesteps (with a checked "⏿" box in the
  timestep list) have their outlines displayed. Requests are kept per run, i.e. selecting another run does not remove the
  displayed timesteps of previously selected runs, which allows to compare isochrones of different runs (e.g. for different
  weather or fuel inputs). In this mode the "show" checkbox of a run is a shortcut to request (checked) or remove (unchecked)
  all timesteps of this run.

The modes are switched as follows:

- requesting a timestep (checking its "⏿" box) in "show selected run" mode automatically switches to "show requested
  timesteps" mode. This removes all previously displayed outlines so that only the requested timestep is shown, and clears
  the "show sel" checkbox
- checking "show sel" switches to "show selected run" mode. This removes all timestep requests of all runs but keeps the
  selected run and shows all of its outlines (the selected timestep remains filled)
- manually un-checking "show sel" switches to "show requested timesteps" mode without changing the display, i.e. all
  timesteps of the selected run become requested timesteps

In both modes un-checking the "show" checkbox of a run removes all outlines and fills of this run. The "∅" button removes
all displayed outlines, fills and images of all runs and clears the run and timestep selections, without changing the
selection mode.

The **display parameters** panel controls the stroke color and width of isochrone outlines, and the fill color and
alpha of the highlighted (selected) isochrone polygon. Parameter changes only apply to the selected run and to runs
that are selected afterwards - other displayed runs keep their rendering. Together with the "show requested timesteps" mode
this makes it easier to visually compare isochrones of different runs. The "reset all" button restores the configured
default rendering.

The **input** panel contains all the potential data required to create a new Elmfire simulation. The "pick now" button allows to quickly select an ignition point on the virtual globe and use the current time as the ignition time. This will populate the "loc" and "dtg" entries underneath (which can be changed subsequently). Before a new simulation request can be submitted by clicking the "simulate" button the user has to enter a respective name for the simulation. The "comp wind" checkbox controls if either actual or entered wind data is used directly or WindNinja output generated from it should be the input for Elmfire. The main part of this panel is a tab container for the different types of Elmfire input sources (using Fortran Namelist format). Each tab has a row underneath its source editor to create respective sources from chosen/default values.
