# odin_nexrad

`odin_nexrad` imports NEXRAD WSR-88D Level II radar data of configured stations from the public AWS S3 bucket
`unidata-nexrad-level2` (see [NOAA NEXRAD on AWS](https://registry.opendata.aws/noaa-nexrad/)), computes
geo-referenced (EPSG:4326) products such as base and composite reflectivity with the
[`nexrad`](https://github.com/danielway/nexrad) crate and stores them as GeoTIFFs, which are served as time step layers
of a WMS endpoint for display on the [odin_cesium](../odin_cesium/odin_cesium.md) virtual globe.

## data source

Level II volumes are stored per station and UTC date as

```
<YYYY>/<MM>/<DD>/<station>/<station><YYYYMMDD>_<HHMMSS>_V06
```

where the time is the scan start (which we use as the unique time stamp of a volume). The bucket also contains
`..._V06_MDM` model data files, which are ignored. A volume contains all elevation sweeps of a scan with the
reflectivity, velocity, spectrum width and dual-pol moments (see the
[WSR-88D ICD](https://archive.unidata.ucar.edu/data/WSR-88D_ICD.pdf) and
[Level II data types](https://www.roc.noaa.gov/level-two-data-types.php)). Volumes are 2-10MB.

Stations scan with different, weather dependent intervals (the volume coverage pattern, e.g. VCP 35 in clear air
mode or VCP 212 in precipitation mode) and volumes are published after the scan is complete, i.e. the publication
delay is about the scan interval. A snapshot of all 198 stations showed median intervals of 3.5-10min (median 7min)
and similar publication delays. Update times therefore have to be estimated from the listings of each station
(`odin_nexrad::VolumeTiming`): the next volume is expected at

```
last scan start + last interval + median publication delay (+ check_delay)
```

Station positions are taken from the site registry of the `nexrad` crate, with an optional CSV file
(`$ODIN_ROOT/data/odin_nexrad/Weather_Radar_Stations.csv`) as fallback for stations that are not in the registry.

## products

| product | name | description |
|---------|------|-------------|
| `BaseReflectivity` | `bref` | reflectivity of the lowest elevation sweep (the first one with the longest range) |
| `CompositeReflectivity` | `cref` | max reflectivity over all sweeps |

Product GeoTIFFs store reflectivity as 8 bit codes with the encoding of Level II reflectivity data (dBZ =
(code - 66) / 2, code 0 is below threshold and 1 range folded), i.e. this does not lose any precision of the source
data (all Level II reflectivity values are on a 0.5 dBZ grid, -32..94.5 dBZ). Product files are stored as
`$ODIN_ROOT/cache/odin_nexrad/<station>/<product>/<station>_<product>_<scan-time>.tif`, which is also the WMS layer
name (e.g. `KMUX_bref_20261117T001157Z`).

Analysis that requires the original polar data or other moments (e.g. tracking wildfires with reflectivity maxima
along radials, see [McCarthy et al. 2022](https://doi.org/10.1029/2021JD036158)) should use the Level II volumes,
which can be kept in the cache (`keep_volumes`) as `$ODIN_ROOT/cache/odin_nexrad/<station>/volumes/<volume-name>`.

Products are resampled from the polar sweep data onto a geographic grid with `resolution` degree cells that covers
the max range of the used sweeps (±460km, i.e. ~2070x1650 cells at 0.005°). Each cell gets the max valid reflectivity
of the used sweeps at its position (for base reflectivity this is just the value of the lowest sweep). Computing both
products of a volume takes about 1.5sec (debug build). The GeoTIFFs are tiled, compressed (~200-300KB) and have
overviews and a color table with the reflectivity scale of `nexrad-render`. Reflectivity below `min_dbz` (default 5
dBZ, rendered black by `nexrad-render`) is transparent.

## operation

The `NexradActor` initializes its store from the product files in the cache (removing files that are not needed
anymore) and executes its `init_action`, i.e. clients get the available time steps before any new import. It then
starts one import task per configured station, which

1. lists the recent volumes of the station and imports the last `backfill` ones we don't have yet
2. waits until the expected availability time of the next volume (see above) and then lists the new volumes
   (S3 `start-after` listing), retrying every `retry_delay` until there is a new volume

Imported volumes are downloaded into memory (and stored if `keep_volumes` is set), decoded and processed into product
GeoTIFFs (in a blocking task). The actor keeps the last `max_downloads` time steps of each product and station. If a
new volume drops older time steps the actor first executes its `update_action` (which removes their WMS layers) and
then deletes their files (and kept volumes that are not referenced anymore).

The `NexradService` serves all product time steps as dynamic layers of [`odin_gdal::wms_srv::WmsService`](../odin_gdal/odin_gdal.md).
Layers are colorized with the color table and support `GetFeatureInfo` value lookup (e.g. "35.5 dBZ").

## user interface

The "NEXRAD Radar" window has the following panels:

- **stations** - the configured stations with the scan time of their latest time step. Stations are also shown on the
  map, selecting a station symbol selects the station in the list (double click in the list zooms to the station).
  The "show" checkbox of a station (set by default) controls if its time step is displayed and queried by "pick".
  De-selected stations keep their time step (still updated by "show latest"), explicitly showing a time step of a
  de-selected station selects it again
- **time steps** - one tab per product ("base", "composite") with the time steps of the selected station (newest on
  top) and a "show" checkbox for each. Each station shows at most one time step at a time. "show latest of all
  stations" shows the newest time step of the selected product tab for all stations (and follows new imports).
  Explicitly showing a time step resets "show latest" (other stations keep their time steps)
- **display** - layer opacity
- **value** - the value of all displayed time steps at a map position can be looked up with the "pick" button or
  shift+click (WMS GetFeatureInfo)

## server routes and messages

| route | description |
|-------|-------------|
| `<app>/nexrad/wms` | WMS (GetCapabilities, GetMap, GetFeatureInfo) for all product time steps |

Websocket messages are `snapshot` (products and all stations with positions, extents and time steps, sent to new
connections) and `update` (added and removed time steps of a station).

## configuration

`configs/nexrad.ron` (`NexradConfig`):

```rust,ignore
NexradConfig (
    bucket: "unidata-nexrad-level2",
    region: "us-east-1",
    stations: [ "KMUX", "KDAX", "KBBX", "KHNX", "KBHX" ],
    products: [ BaseReflectivity, CompositeReflectivity ],
    resolution: 0.005,   // product grid cell size [deg]
    min_dbz: 5.0,        // reflectivity below this is transparent
    max_downloads: 12,   // time steps we keep per station and product
    backfill: 6,         // most recent volumes per station retrieved on startup
    check_delay: "30s",  // added to the expected availability time of the next volume
    retry_delay: "1m",   // retry interval if the volume is not available yet
    keep_volumes: false,
    stations_csv: Some("$ODIN_ROOT/data/odin_nexrad/Weather_Radar_Stations.csv"),
    wms: ( title: "ODIN NEXRAD WMS", max_cache: 268435456, max_renders: 4, max_downsample: 64.0 ),
)
```

## tools

`list_nexrad` lists the available stations or the volumes of stations:

```sh
list_nexrad --stations                 # stations with volumes today (with positions)
list_nexrad --date 2026-11-16 KMUX     # volumes of a station for a date
list_nexrad --hours 1                  # volumes of the configured stations within the last hour
```

`nexrad_timing` shows scan interval and publication delay statistics (min/median/max) and the expected time of
the next volume. `--decode` downloads and decodes the latest volume of each station and shows its VCP, sweeps and
reflectivity encoding. `--ron <file>` writes the timings as a RON list:

```sh
nexrad_timing --hours 3 KMUX KDAX
nexrad_timing --decode KMUX
nexrad_timing --all --ron station_timings.ron
```

Both tools use `nexrad.ron` (bucket, region, stations, station CSV) if it is available.

## S3 access

The S3 functions of `odin_common::s3` (`s3` feature) are used for bucket access (anonymous): `list_s3_objects`
(paginated ListObjectsV2 with `start_after`, e.g. for incremental listings), `list_s3_prefixes` (e.g. the station
directories of a date) and `get_s3_object_bytes`. The AWS access of the `nexrad-data` crate is not used.

## running

```sh
cargo run --bin show_nexrad
```

starts a stand-alone server (`http://127.0.0.1:9009/nexrad` with the default `spa_server.ron`). Applications use the
actor and service like this:

```rust,ignore
let config: NexradConfig = odin_nexrad::load_config("nexrad.ron")?;
let wms = NexradWms::new( &config); // shared between the actor actions and the service
let hnexrad = spawn_actor!( actor_system, "nexrad", NexradActor::new(
    config,
    server_init_action( hserver.clone(), "nexrad", wms.clone()),
    server_update_action( hserver.clone(), wms.clone())
))?;
let svc_list = svc_list.add( build_service!( let hnexrad = hnexrad.clone(), let wms = wms.clone() => NexradService::new( hnexrad, wms)));
```

## tests

`tests/test_nexrad.rs` covers config parsing, volume names, timing statistics, reflectivity encoding and palette,
station lookup, product names, the store and cache rebuild. Decoding and product computation of a real volume are
tested if `ODIN_NEXRAD_SAMPLES` points to a directory with `KMUX20261116_235440_V06`, S3 listings are tested if
`ODIN_NEXRAD_S3` is set.
