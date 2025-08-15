# odin_hrrr

The `odin_hrrr` crate is an application domain crate to automatically download NOAA 
[High Resolution Rapid Refresh(HRRR)](https://rapidrefresh.noaa.gov/hrrr/) weather forecast data. 
HRRR provides a rolling forecast with a 3km grid for both CONUS and Alaska that is updated every hour, each with a fixed
number of separate hourly forecast steps. HRRR data is distributed in the
[GRIB2](https://old.wmo.int/extranet/pages/prog/www/WMOCodes/Guides/GRIB/GRIB2_062006.pdf) gridded binary format, i.e.
it is not readily available for display purposes - it is generally input for complex functions such as computing local
wind predictions, which therefore require timely updates once new forecast steps become available. 

This crate mostly provides the functions to download HRRR data sets for specific areas and times, including periodic
download of live forecast data as it becomes available from the 
[NOAA HRRR server](https://nomads.ncep.noaa.gov/pub/data/nccf/com/hrrr/prod). 

Since HRRR data sets can be large and retrieval might require high bandwidth the `odin_hrrr` crate is primarily intended
for building [*edge servers*](../intro.md#edge-servers).


## 1. HRRR Schedules

If continuously downloaded this can be a lot of data - depending on required HRRR variables each forecast file can be ~0.8Gb
for conus. Moreover, there is a slightly varying delay of about 50min until the first forecast step for an hour becomes
available, and the following 18 forecast steps are each staggered by about 1-2min. Extended forecasts with 49 steps are 
computed at fixed forecast hours (0am, 6am, 12pm, 18pm).

HRRR reports are generated each hour for a range of fixed forecast hours (each forecast hour is distributed as a
separate file). We call the set of all forecast files for a given hour a '*forecast cycle*', and the hour for which this
cycle is the '*base hour*'. Base hours 0am, 6am, 12pm, 18pm are extended cycles covering 0..=48h (=number of forecast files to
retrieve), all other (regular) cycles cover 0..=18h.
 
```
     Bi   : base hour i (cycle base)
     s[j] : minutes since base hour for forecast step j availability (j: 0..=18 for regular, 0..=48 for extended)
     ◻︎    : forecast data set for t = Bi+s[j]
     
                   Bi                     Bi+1                   Bi+2
                   │0              s[0]   │60        s[N]        │120 
     cycle i-1 ...◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎       ┊     │           ┊          │
                   │                ┊     │  cycle i  ┊          │
                   │                ◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎          │
                   │    dm < s[0]   ┊ s[0]<= dm <=s[N]┊   dm > S[N]
                   │                      |                ◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎... cycle i+1
                   ├──────────────────────|────>T                │
                                    dm: minutes(T) + 60
```
 
File availability within a cycle is staggered. It currently (as of Oct 2024) takes about 50min from base hour until the
first step of a forecast cycle becomes available. Each consecutive forecast step takes about 2min for regular cycles and
1min for extended cycles. We assume that cycles can be processed sequentially.
 
Schedules can be either estimated from a given `HrrrConfig` (config file) or computed by parsing the directory listing from the 
[NOAA server](https://nomads.ncep.noaa.gov/pub/data/nccf/com/hrrr/prod) which has directories for the current and past day (this
might be brittle since the HTML directory listing format on the NOAA server can change). `HrrrConfig` files have the following structure:

```rust,ignore
HrrrConfig(
    region: "conus",
    url: "https://nomads.ncep.noaa.gov/cgi-bin/filter_hrrr_2d.pl",
    dir_url_pattern: "https://nomads.ncep.noaa.gov/pub/data/nccf/com/hrrr/prod/hrrr.${yyyyMMdd}/conus",

    // estimated schedules if we don't want to compute them from NOAA server directory listings
    reg_first: 48,
    reg_last: 84,
    reg_len: 19,

    ext_first: 48,
    ext_last: 108,
    ext_len: 49,

    delay: Duration(secs:60,nanos:0), // extra time added to each computed schedule minute

    check_interval: Duration(secs:30,nanos:0), // interval in which we check availability of new forecast steps
    retry_delay: Duration(secs:30,nanos:0), // how long to wait between consecutive attempts for not-yet-available files
    max_retry: 4, // how many times do we try to download not-yet-available files
    max_age: Duration(secs:21600,nanos:0), // how long to keep downloaded files (6h)
)
```
 

## 2. Forecast Data and Queries

HRRR forecasts do provide some 90 variables and about 50 altitude/atmospheric levels. This means full grib2 files for CONUS can
be >700 MB for each forecast step, of which there are at least 19 per hour. Continuously downloading this amount of data might
overwhelm some ODIN applications/servers. 

HRRR does support [queries for variable subsets and region boundaries](https://nomads.ncep.noaa.gov/gribfilter.php?ds=hrrr_2d). This
can be used to reduce application specific forecasts with a limited number of variables (e.g. temperature, cloud cover, u/v- wind)
for medium sized incident areas (~50x50mi) to about 3.5 MB. 

We therefore support configuration of both variables and regions of interest through the `HrrrDataSetConfig` struct, which can be 
serialized/deserialized like so:

```rust,ignore
HrrrDataSetConfig(
    name: "BigSur",
    bbox: GeoBoundingBox(
        west: LonAngle(-122.043),
        south: LatAngle(35.99),
        east: LonAngle(-121.231),
        north: LatAngle(36.594)
    ),
    fields: ["TCDC", "TMP", "UGRD", "VGRD"],
    levels: ["lev_2_m_above_ground", "lev_10_m_above_ground", "lev_entire_atmosphere"],
)
```

The `HrrrActor` supports multiple simultaneous regions of interest. Downloaded 
[`grib2`](https://old.wmo.int/extranet/pages/prog/www/WMOCodes/Guides/GRIB/GRIB2_062006.pdf) files are stored as they are received
from the NOAA server in `<ODIN-root>/cache/hrrr/hrrr-wrfsfcf-<region>-<subregion-name>-<date>-<base-hour>+<forecast-step>.grib2
(e.g. `.../hrrr-wrfsfcf-conus-bigsur-20241019-13+16.grib2`).

General parameters such as NOAA server URLs and maximum age of cached files can be configured with the `HrrrConfig`
struct mentioned above. All configuration is supported by the [`odin_build`](../odin_build/odin_build.md) crate, i.e.
respective serialized (`*.ron`) files can be inlined or looked up from `<ODIN-root>` directories or the source
repository.


## 3. Crate Functions

The `odin_hrrr` crate can be used from within or outside of [`odin_actor` actor systems](../odin_actor/odin_actor.md). The basic
functions are

- one-time download of complete sets of most recent forecasts for a given time and `HrrrDataSetConfig`
- periodic download of forecast steps for given `HrrrDataSetConfig` lists as they become available from the NOAA server


### 3.1 One-Time Download of Available Forecasts

The purpose of this function is to obtain all most recently updated (available) forecast steps for a given time point.
Forecast hours of cycles overlap (T(Bi + j) == T(Bi-1 + j+1)) but we only retrieve the step from the last cycle that
covered the forecast hour. 

Since every 6h we get an extended forecast cycle that covers 0..=48 forecast hours this means we have to retrieve
forecast steps from up to 3 cycles:

```
     ◻︎ : obsolete available forecast step (updated by subsequent cycle)
     ◼︎ : relevant available forecast to retrieve (most up-to-date forecast for base + step)
     ○ : not-yet-available forecast step
   
     ◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎  (3) last ext cycle
                          ▲
      ◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎ ┊
                          ┊
       ◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◻︎◼︎◼︎◼︎                                    (2) last cycle: always completely available
                       ▲
        ◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎◼︎○○○○                                   (1) current cycle: might only be partially available
```
 
This function is normally used when a new `HrrrDataSetRequest` is made, i.e. it is a one-time call per data set.
It is mostly implemented in the `queue_available_forecasts(..)` function.


### 3.2 Periodic Download of Forecasts

Since HRRR schedules are non-trivial and ODIN is primarily used to continuously and timely process data as it becomes
available this is the main function of the `odin_hrrr` crate. 

Periodic download of HRRR forecast data sets is an async function that has to run in its own task, which is created
by `odin_hrrr::spawn_download_task <A:DataAction<HrrrFileAvailable>>(config: Arc<HrrrConfig>, cache_dir: PathBuf, action: A)`.
The `action` parameter (see [odin_action](../odin_action/odin_action.md)) is what makes this function generic - it specifies
the async callback to be executed once a forecast step file has been fully downloaded by the task.

This task is automatically spawned from within a `HrrrActor` or from within the async `odin_hrrr::run_downloads(..)` function if
an edge server does not want to use a dedicated `HrrrActor` (see `get_hrrr.rs` binary). Dynamically adding/removing
`HrrrDataSetRequests` requires the actor. The task has its own request queue - it does not schedule new forecast steps itself
but depends on the application context (e.g. the `HrrrActor`) to do so, which is based on the HRRR schedule mentioned above.

As an auxiliary function the task also removes old HRRR data files according to the `max_age` value of the provided `HrrrConfig`,
i.e. it has to ensure disk space remains bounded.


## 4. Applications

An simple actor application that just prints out notifications for each downloaded HRRR file looks like so:

```rust,ignore
use std::sync::Arc;
use odin_common::define_cli;
use odin_actor::prelude::*;
use odin_hrrr::{load_config,HrrrActor, AddDataSet, HrrrConfig, schedule::{HrrrSchedules,get_schedules}, HrrrDataSetRequest, HrrrDataSetConfig, HrrrFileAvailable};

define_cli! { ARGS [about="NOAA HRRR download example using HrrrActor"] =
    hrrr_config: String [help="filename of HRRR config file", short,long,default_value="hrrr_conus.ron"],
    statistic_schedules: bool [help="compute schedules of available forecast files from server dir listing", short, long],
    ds_config: String [help="filename of HrrrDataSetConfig file"]
}

run_actor_system!( actor_system => {
    let hrrr_config: HrrrConfig = load_config( &ARGS.hrrr_config)?;
    let schedules: HrrrSchedules = get_schedules( &hrrr_config, ARGS.statistic_schedules).await?;
    let ds: HrrrDataSetConfig = load_config( &ARGS.ds_config)?;
    let req = Arc::new(HrrrDataSetRequest::new(ds));
    
    let himporter = spawn_actor!( actor_system, "hrrr_importer", HrrrActor::new(
        hrrr_config,
        schedules,
        data_action!( => |data: HrrrFileAvailable| {
            println!("file available: {:?}", data.path.file_name().unwrap());
            Ok(())
        })
    ))?;

    actor_system.start_all().await?;
    himporter.send_msg( AddDataSet(req)).await?;
    actor_system.process_requests().await?;

    Ok(())
});
```
