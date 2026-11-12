# odin_cesium

This is a system crate that depends on [`odin_server`], providing `odin_server::SpaService` implementations that are related to using
[CesiumJS](https://cesium.com/platform/cesiumjs/) for user interfaces (UI) on top of a virtual globe display that is rendered by means
of [WebGL](https://developer.mozilla.org/en-US/docs/Web/API/WebGL_API) inside of a standard web browser. 

This crate provides two main services:

- `CesiumService` is responsible for providing the 3rd party CesiumJS code for the virtual globe rendering, and the
  `odin-rs` specific user interface code that controls it. It also includes some general UI elements that are always
  present (e.g. clock)
- `ImgLayerService` is a configurable service to provide and control background imagery from own and external (proxied) sources.
  It is especially the layer that controls the map display

## How to interactively use the Cesium Display

Apart from normal mouse based Cesium view control

- left-click + drag : pan
- scroll wheel / swipe up/down : zoom in/out  
- ctrl + left-click + drag : tilt and rotate

We provide a number of additional mouse interactions and hotkeys to control both the virtual globe view and ui windows:

- alt + left-click : center display on cursor position
- alt + arrow-left/arrow-right or arrow-up/arrow-down :  move center display left/right or up/down
- alt + home : pan/zoom back to home (initial) view
- alt + page-down : reset orthogonal view angle (towards center of earth)
- alt + 1/2/3/4/5/6 : zoom to (`odin_cesium_config.js` configured) camera height level
- alt + 'i'/'o' : zoom in/out
- alt + '[' / ']' : decrease/increase brightness of the shown globe imagery in 0.1 steps (e.g. to make entities stand out)
- esc : reset entity selection indicator (the green rectangle around selected objects)
- alt + left-click in titlebar of ui window : move window to the left 
- alt + 'l'/'r' or 't'/'b' : move top window to left/right or top/bottom
- alt + 'n'/'p' : cycle forwards/backwards through ui windows
- alt + print-screen (or F13) : save a screenshot of the browser tab (globe plus ui windows) as an image file
- alt + shift + print-screen (or F13) : start/stop recording the browser tab as a video file (webm, or mp4 on Safari).
  Recording can also be stopped with the browser's "Stop sharing" button. Resolution and bitrate are configured in the
  `screenRecording` section of `odin_cesium_config.js`

Screenshots and recordings use the browser Screen Capture API, i.e. the user has to confirm (Chrome) or select (Firefox,
Safari) the captured tab each time. The filename is entered in a dialog and the file is saved as a browser download.


## How to access CesiumJS

Since CesiumJS is a large 3rd party library we do support options for how it is accessed by the client. This can be either by

- directly downloading it from https://cesium.com/downloads/cesiumjs/releases/⟨version⟩/Build/Cesium,
- proxying this URL with the `odin-rs` server - this is the default
- providing it as a static asset from $ODIN_ROOT/assets/odin_cesium/cesiumjs

Those alternatives are controlled by the Cargo features `cesium_external`, `cesium_proxy` and `cesium_asset`.
Please note that crates depending on `odin_cesium` should support "pass-through" features in their respective `Cargo.toml` like so:

```toml
...
[dependencies]
...
odin_cesium = { workspace = true }

[features]
...
cesium_asset = ["odin_cesium/cesium_asset"]
cesium_external = ["odin_cesium/cesium_external"]
```

There is no need to pass through `cesium_proxy` as it is the default. While this is convenient for setting up development
environments with minimal configuration this might cause reloading and should be avoided in a production environment.

To obtain, strip and store the requires cesium assets in `<ODIN-ROOT>/odin_cesium/cesiumjs/` we provide the
`install_cesium` binary tool, which you can run like so: `cargo run --bin install_cesium -- --version=<cesium-version>`

In all cases the Cesium version to use is specified as a `CESIUM_VERSION` constant in `odin_orbital/src/lib.rs`. If you use
the `cesium_asset` option this has to correspond with the downloaded/installed version.

To see the current CesiumJS version please visit https://cesium.com/downloads/.


## Local Terrain

By default CesiumJS gets its terrain from [Cesium World Terrain](https://cesium.com/platform/cesium-ion/content/cesium-world-terrain/),
which requires external network access from each browser and a Cesium Ion access token. The Ion terrain cannot be proxied
since Cesium.js authenticates with dynamically created, short lived tokens. It is also known to hang on Safari when
connected to some VPNs (the reason for the `fakeTerrain` option in `odin_cesium_config.js`).

As an alternative `CesiumService` can serve terrain tiles that are computed on demand from a local DEM VRT, normally the
[`odin_dem`](../odin_dem/odin_dem.md) 3DEP 1/3 arcsec VRT created by `encode_dem` and `build_dem_vrt`. This requires the
`terrain` Cargo feature (which is default and adds a GDAL dependency) and a `$ODIN_ROOT/configs/odin_cesium/terrain.ron`
config file:

```rust,ignore
TerrainConfig(
    vrt_path: None,  // None: use `vrt_path` from $ODIN_ROOT/configs/odin_dem/dem.ron
    geoid_path: Some("$ODIN_ROOT/data/odin_dem/us_nga_egm08_25.tif"), // EGM2008 geoid grid from PROJ
    global_levels: 5,      // levels 0..=5 are available world-wide
    max_level: None,       // None: computed from source resolution (15 for 1/3 arcsec)
    seed_level: Some(6),   // pre-compute missing tiles up to level 6 when the server starts
    cache_dir: None,       // None: $ODIN_ROOT/cache/odin_cesium/terrain
)
```

If there is no `terrain.ron` (or the crate is built without the `terrain` feature) the browser uses Cesium World Terrain
as before. On the client side `odin_cesium.js` selects the terrain provider in this order:

1. an explicitly configured `terrainProviderPromise` in `odin_cesium_config.js`
2. the local terrain provider, if the server has one (`terrain/layer.json` exists) and `localTerrain` is not set to `false`
   in `odin_cesium_config.js`
3. Cesium World Terrain

### Tile Format and Computation

Tiles use the Cesium [heightmap-1.0](https://github.com/CesiumGS/cesium/wiki/heightmap-1.0-terrain-format) format in a
geographic (EPSG:4326) TMS tiling scheme with 2x1 tiles at level 0. Each tile has 65x65 u16 height samples (adjacent tiles
share edge samples) with `h = v/5 - 1000`, i.e. 0.2m height resolution, followed by a child tile mask byte. Tiles are
served gzipped (4-6 kB per tile) from the routes

- `<app>/terrain/layer.json` - tiling scheme and tile availability
- `<app>/terrain/{z}/{x}/{y}.terrain`

To compute a tile we read the tile area from the VRT at about the tile sample resolution (GDAL uses the source overviews
for coarse levels), bilinearly interpolate the 65x65 sample points and add the geoid undulation. DEM heights are
orthometric (NAVD88 for 3DEP) whereas Cesium uses ellipsoidal (WGS84) heights. Without the geoid correction terrain
would be 20-40m too high in CONUS relative to positions with ellipsoidal heights (aircraft, satellites, camera
positions). The EGM2008 grid is loaded into memory at 5' spacing (37 MB), which is accurate to a few centimeters. EGM2008
differs from NAVD88 by about 1-2m, which is irrelevant for display. DEM NoData (e.g. ocean) becomes mean sea level, i.e.
the geoid surface.

The `layer.json` lists the available tiles for each level, which is required by `Cesium.sampleTerrainMostDetailed()`
(used e.g. by `terrainIntersection()` in `odin_cesium.js`). Levels `0..=global_levels` are available world-wide (outside
of the DEM these tiles only contain the geoid surface). Availability for higher levels is computed from the extents and
resolutions of all VRT sources (following nested VRTs), i.e. adding coarser regions outside of CONUS to the VRT
automatically limits their max level. Cesium upsamples parent tiles where there are no available tiles.

Computed tiles are stored in `<cache_dir>/<version>/{z}/{x}/{y}.terrain`. The version is derived from the modification
times of the VRT files and the geoid grid, i.e. rebuilding the VRT automatically invalidates both the disk cache and
browser caches (tile URLs include `?v=<version>`). Old version directories can be deleted manually or by `odin_scrub`.

### Performance

Measured on an Apple M4 Max with the CONUS 3DEP U16 VRT (967 source files, page-cached):

| level | sample spacing | compute time per tile |
|-------|----------------|-----------------------|
| 0     | 2.8°           | ~7 s                  |
| 3     | 0.35°          | ~1.1 s                |
| 7     | 0.022°         | 10-20 ms              |
| 9     | 0.0055°        | 0.2-12 ms             |
| 11-15 | 1.4"-0.31"     | 0.1-4 ms              |

Cached tiles are served in less than 5ms. Low levels are expensive because they touch hundreds of source files (each
open plus overview read takes ~4ms). This is the reason for `seed_level`: missing tiles up to that level are computed in
a background thread (4 threads) when the server starts. Levels 0-6 have about 2900 tiles (most of them outside of the DEM
and hence cheap). Since tiles are cached this only takes time after the VRT changes. Seeding up to level 8 (5568 tiles,
31 MB) takes about 9s with 16 threads. The CONUS 3DEP set has about 32 million available level 15 tiles, i.e. higher
levels are only computed on demand.

The `terrain_tiles` tool shows the configuration and per-level availability, computes single tiles and seeds the cache:

```shell
terrain_tiles                                      # show config and availability
terrain_tiles --tile 0/0/0,12/1314/2916            # compute tiles (uncached) and show time and height range
terrain_tiles --seed 8 -j 16                       # pre-compute missing tiles up to level 8
```

Heightmap tiles have no vertex normals, i.e. terrain lighting is flatter than with Cesium World Terrain. There is no
terrain relief outside of the areas covered by the VRT (CONUS for 3DEP) since Cesium cannot combine terrain providers.
Other regions can be added to the VRT with `build_dem_vrt` (see `odin_dem`).


## Testing Data and UI Rendering

Since [client side](../odin_server/client.md) `odin-rs` code is written in Javascript there is - depending on the development
environment in use - only limited type checking available. It is therefore good practice to test often and early, especially
since this applies to both the [`odin-rs` user interface](../odin_server/ui_library.md) and to CesiumJS / WebGL based service-specific
data rendering.

To simplify testing of UI and rendering without a fully functional `SpaService` we provide the `cesium_test` tool. This is a 
test harness for `odin-rs` Javascript modules that can make full use of the `odin-rs` [UI library](../odin_server/ui_library.md) and
the CesiumJS library (including its `odin-rs` specific extensions in `odin_cesium.js`). The test module is provided as an explicit
file pathname when launching `cesium_test` like so:

```shell
cargo run --bin cesium_test  --  <JS-module-pathname>
```

Test modules have to use the `exportFuncToMain(..)` function of `main.js` to set a global parameterless `start()` function that
can be triggered by a respective "start" button which is automatically added to the right of the icon box at the top of the page.
This function should in turn execute the behavior that is supposed to be tested. Look at the minimal `odin_cesium/test/test.js`
example for details:

```javascript
import * as main from "../odin_server/main.js";

/* import on demand */
// import * as util from "../odin_server/ui_util.js";
// import * as data from "../odin_server/ui_data.js";
// import * as ui from "../odin_server/ui.js";
// import * as odinCesium from "./odin_cesium.js";

function start() {
    console.log("get started.");
    // trigger your real test here
}
main.exportFuncToMain(start);

// your test functions/data goes here...
```

Since `cesium_test` is a test and debug tool it automatically sets the `ODIN_RELOAD_ASSETS` environment variable used by
[`odin_build`](../odin_build/odin_build.md) and therefore it does not have to be relaunched if any of the involved Javascript
modules is changed - just refresh the page in the browser and hit "start" again to see the change effect.
