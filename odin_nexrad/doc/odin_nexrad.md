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

Products are resampled from the polar sweep data onto a geographic grid that covers the max range of the used
sweeps (±460km). Cell sizes are configured in meters (`resolution_m`, default 250m = the Level II gate interval) and
converted into degrees per station so that cells are square at the station latitude (at 250m this is a ~3670x3670
grid). Since the azimuthal gate width grows with range (0.5° super-resolution radials are 87m wide at 10km, 960m at
110km and 4km at 460km) no fixed grid resolution matches the polar data everywhere. Each cell therefore gets the max
valid reflectivity of the used sweeps from two passes:

1. sampling of the nearest gate at each cell center - this gives each cell a value where gates are larger than cells
   (far range)
2. mapping of each gate center into its cell - this makes sure no gate is lost where gates are smaller than cells
   (near range). Without it a 0.005° grid would skip about half of the gates within 110km (3 out of 4 within 30km),
   which can miss small cores close to the radar in composite reflectivity

For base reflectivity this is just the value of the lowest sweep. `resolution_m` is therefore only a trade-off between
display detail and file size / computation time - larger cells do not lose max values. Computing both products of a
volume at 250m takes about 0.9sec (release build, 5sec debug build). The GeoTIFFs are tiled, compressed (~0.6-0.9MB at 250m) and have
overviews and a color table with the reflectivity scale of `nexrad-render`. Reflectivity below `min_dbz` (default 5
dBZ, rendered black by `nexrad-render`) is transparent.

## overview mosaic

If `mosaic` is configured the actor periodically (`interval`, only if there were updates since the last one) computes an
overview mosaic of all stations for each product, which is distributed as GeoJSON polygons of reflectivity bands. This
is meant for wide area / low resolution display - the station layers remain the source for high resolution display
and value lookup. Inputs are the latest time steps of all stations that are not older than `max_age` relative to the
newest one. The mosaic is computed in five steps (see `odin_nexrad::mosaic`):

1. the station grids are reduced to about the mosaic resolution (`resolution_m`, default 1km) by averaging linear
   reflectivity (Z = 10^(dBZ/10), the physical area mean, which is dominated by strong echoes). Cells that are mostly
   range folded are missing data
2. the reduced grids are blended onto the mosaic grid (which covers the max range of all configured stations),
   preferring the station nearest to each cell. Stations that are less than `blend_m` (default 20km) farther away than
   the nearest one contribute with linearly decreasing weights, and weights also taper off towards the max range of a
   station. This avoids seams where the preferred station changes, which would otherwise show up as contour edges
3. gaussian smoothing (`smooth_m` sigma, default 1.5km)
4. GDAL contouring into polygons for each band between `levels` (default 10, 20, 30, 40, 50 dBZ - widespread
   stratiform rain is mostly 10-25 dBZ, hence the low first level)
5. polygon simplification (`simplify_m` tolerance, default 250m) and removal of polygons and holes smaller than
   `min_area_km2` (default 4km²)

Blending and smoothing are done in dBZ with "no echo" being 5 dBZ below the lowest level. Each band is a GeoJSON
MultiPolygon feature with `lo`, `hi` (dBZ, `hi` is null for the top band) and `color` (reflectivity scale, clients can
override it) properties. The FeatureCollection has an additional `stations` member with the used station time steps.

Mosaics are the overview time steps. Each one is stored as `<cache>/mosaic/<product>_mosaic_<date>.geojson`, where
`<date>` is the newest scan time of the used station time steps. A new mosaic with the same date as the previous one
(e.g. because a delayed time step of another station came in) replaces it. The actor keeps the last `max_mosaics`
(default 12) per product, removes the files of dropped ones and restores mosaics from these files on startup.

For a widespread storm over all five configured stations (Nov 18 2026, ~8-10% of the 1326x1385km mosaic area above
10 dBZ) a mosaic takes ~130-145ms per product on an M4 Max (release build): reading and reducing the station grids
~50ms, blending ~4ms, smoothing ~3ms, contouring ~45ms and simplification ~30ms. The result has ~300-360 polygons
with ~15-19k vertices (~50-60k without simplification), i.e. 300-375KB of GeoJSON (~250KB with `simplify_m: 500`).
Reduced station grids are cached by the actor, so only new time steps have to be read. Use the `nexrad_mosaic` tool
to tune parameters.

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

<img class="left" src="../img/odin_nexrad-ui.png" width="40%">

- **stations** - the configured stations with the age of their (displayed or available) time step at the time cursor
  in minutes and the scan time of their latest time step. The "show" checkbox of a station (set by default) controls
  if the station contributes to the display, i.e. if its time steps are listed, displayed and queried by "pick". Since
  stations overwrite each other where they overlap this is how users select the stations that are relevant for a
  region (closer stations have lower beams and higher resolution). The age column shows which stations are current.
  De-selected stations keep their time step as a hidden layer. Stations are also shown on the map, selecting a station
  symbol selects the station in the list, double click in the list zooms to the station. Zooming in on a station
  shows an info label with its name, displayed time step and age, and latest scan time
- **time cursor** - what is displayed is determined by a time cursor T (shown left of the "latest showing"
  checkbox): each shown station displays its newest time step <= T of the active product, unless that is older than
  T - max age (display panel). With "latest showing" T is the newest time step of the shown stations and
  follows new imports
- **time steps** - one tab per product ("base", "composite") with the time steps of all shown stations (newest on
  top), with station id and a "show" checkbox for each, i.e. the checked entries are what is displayed. The list
  controls below the tabs step the time cursor through the time steps of the active tab (resetting "latest showing"),
  the clear button hides all time steps. Explicitly checking/unchecking a time step (manual mode) only changes the
  display of its station
- **display** - "nearest station on top" (default on) re-stacks the displayed layers after each camera move so that
  where stations overlap the station closest to the view center is on top (it has the lowest beam and the highest
  resolution). Together with "max age" (default 15min, 5-60min) this approximates a mosaic of the shown stations
  without additional server side products, while users can still exclude stations explicitly. Opacity applies to
  all layers. Initial values are set in `odin_nexrad_config.js` (`nearestOnTop`, `maxAge`, `opacity`)
- **value** - the value of all displayed time steps at a map position can be looked up with the "pick" button or
  shift+click (WMS GetFeatureInfo). The age column shows the age of the respective time step at the time cursor
- **overview** - the "overview" checkbox (right of "latest showing") shows server mosaics of the active product
  instead of the station layers (which are hidden but kept). Selecting it checks all stations as shown since mosaics
  include all of them (unchecking a station turns the overview off). In overview mode the product tabs list the
  mosaics ("ovw" entries, newest on top) instead of the station time steps, and "latest showing", the "show"
  checkboxes and the list controls work on mosaics the same way as for station time steps. The time cursor shows the
  date of the displayed mosaic, the station list and ages refer to the station time steps used in it. Without "latest
  showing" switching between overview and station display keeps the time, i.e. the overview shows the newest mosaic
  at or before the station time cursor and vice versa. GeoJSON files are only loaded when displayed. Pick shows the
  reflectivity band at the position. `odin_nexrad_config.js` sets the initial state (`overview`, default on) and the band fill
  colors (`overviewColors` - `[dBZ, color]` pairs, a band gets the color of the highest entry <= its lower bound)

## server routes and messages

| route | description |
|-------|-------------|
| `<app>/nexrad/wms` | WMS (GetCapabilities, GetMap, GetFeatureInfo) for all product time steps |
| `<app>/nexrad/mosaic/<file>` | overview mosaic GeoJSON files |

Websocket messages are `snapshot` (products, all stations with positions, extents and time steps and all stored
mosaics, sent to new connections), `update` (added and removed time steps of a station) and `mosaic` (product, date,
file and the used station time steps of a new mosaic plus the dates of mosaics it dropped).

## configuration

`configs/norcal_nexrad.ron` (`NexradConfig` for Northern California stations):

```rust,ignore
NexradConfig (
    bucket: "unidata-nexrad-level2",
    region: "us-east-1",
    stations: [ "KMUX", "KDAX", "KBBX", "KHNX", "KBHX" ],
    products: [ BaseReflectivity, CompositeReflectivity ],
    resolution_m: 250.0, // product grid cell size [m]
    min_dbz: 5.0,        // reflectivity below this is transparent
    max_downloads: 12,   // time steps we keep per station and product
    backfill: 6,         // most recent volumes per station retrieved on startup
    check_delay: "30s",  // added to the expected availability time of the next volume
    retry_delay: "1m",   // retry interval if the volume is not available yet
    keep_volumes: false,
    stations_csv: Some("$ODIN_ROOT/data/odin_nexrad/Weather_Radar_Stations.csv"),
    mosaic: Some(( interval: "2m", max_age: "15m", max_mosaics: 12, resolution_m: 1000.0, smooth_m: 1500.0, blend_m: 20000.0,
                   simplify_m: 250.0, min_area_km2: 4.0, levels: [10.0, 20.0, 30.0, 40.0, 50.0] )), // None: no mosaic
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

`nexrad_mosaic` computes the mosaics of the cached time steps and shows the time spent in each step and the size of
the result. Options override the `mosaic` config values:

```sh
nexrad_mosaic --simplify 500 --levels 15,25,35,45 --out /tmp/mosaic
```

All tools use the NexradConfig given by `--config` (default `norcal_nexrad.ron`) for bucket, region, stations and
station CSV if it is available.

## S3 access

The S3 functions of `odin_common::s3` (`s3` feature) are used for bucket access (anonymous): `list_s3_objects`
(paginated ListObjectsV2 with `start_after`, e.g. for incremental listings), `list_s3_prefixes` (e.g. the station
directories of a date) and `get_s3_object_bytes`. The AWS access of the `nexrad-data` crate is not used.

## running

```sh
cargo run --bin show_nexrad [-- --config <nexrad-config>]
```

starts a stand-alone server (`http://127.0.0.1:9009/nexrad` with the default `spa_server.ron`) for the given
NexradConfig (default `norcal_nexrad.ron`). Applications such as `odin_live` use the actor and service like this:

```rust,ignore
let config: NexradConfig = odin_nexrad::load_config("norcal_nexrad.ron")?;
let wms = NexradWms::new( &config); // shared between the actor actions and the service
let hnexrad = spawn_actor!( actor_system, "nexrad", NexradActor::new(
    config,
    server_init_action( hserver.clone(), "nexrad", wms.clone()),
    server_update_action( hserver.clone(), wms.clone()),
    server_mosaic_action( hserver.clone())
))?;
let svc_list = svc_list.add( build_service!( let hnexrad = hnexrad.clone(), let wms = wms.clone() => NexradService::new( hnexrad, wms)));
```

## tests

`tests/test_nexrad.rs` covers config parsing, volume names, timing statistics, reflectivity encoding and palette,
station lookup, product names, the store and cache rebuild. `tests/test_mosaic.rs` covers grid reduction, station
blending (including seam continuity), smoothing, contouring, mosaic input selection, the mosaic history of the store and mosaic restore from files with synthetic data. Decoding and product computation of a real volume are
tested if `ODIN_NEXRAD_SAMPLES` points to a directory with `KMUX20261116_235440_V06`, S3 listings are tested if
`ODIN_NEXRAD_S3` is set.
