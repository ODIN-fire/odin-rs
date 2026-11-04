# odin_landfire

`odin_landfire` provides access to [LANDFIRE](https://landfire.gov) data sets (vegetation, fuel models, canopy and
topographic data) for CONUS. They are used for two purposes:

- as input for fire behavior models (e.g. [odin_elmfire](../odin_elmfire/odin_elmfire.md)), which query values for given
  locations or areas through [`odin_gdal`](../odin_gdal/odin_gdal.md)`::geods::GeoDs` (see `configs/lf_2025_conus.ron`)
- for display in the browser, using the `LandfireService` WMS (see `configs/landfire.ron` and
  `assets/odin_landfire_config.js`), which is a [odin_server](../odin_server/odin_server.md) micro service that displays
  the data sets as imagery layers of the [odin_cesium](../odin_cesium/odin_cesium.md) virtual globe

Both use the original landfire.gov GeoTIFF distributions without any derived data products. The only exception
is a one-time fix for the topographic data sets (see [below](#post-processing-of-lf2020-topo-data)).


## setting up `$ODIN_ROOT/data/odin_landfire`

### directory layout

```
$ODIN_ROOT/data/odin_landfire/
├── 2025/                         ← one directory per LANDFIRE release
│   ├── LF2025_EVT_CONUS.tif      ← existing vegetation type
│   ├── LF2025_EVT.csv            ← color map / category names for EVT
│   ├── LF2025_FBFM40_CONUS.tif   ← Scott and Burgan fire behavior fuel models
│   ├── LF2025_FBFM40_FA26.tif    ← seasonal (fall 2026) FBFM40 update for parts of CONUS
│   ├── LF2025_FBFM40_CONUS.vrt   ← standard data set plus seasonal update (used by all configs)
│   ├── LF2025_FBFM40.json        ← manually edited color map with extended descriptions
│   ├── LF2025_FBFM40.csv
│   ├── LF2025_CC_CONUS.tif       ← forest canopy cover
│   ├── LF2025_CC.csv
│   ├── LF2025_CH_CONUS.tif       ← forest canopy height
│   ├── LF2025_CH.csv
│   ├── LF2025_CBH_CONUS.tif      ← forest canopy base height
│   ├── LF2025_CBH.csv
│   ├── LF2025_CBD_CONUS.tif      ← forest canopy bulk density
│   └── LF2025_CBD.csv
└── topo/                         ← topographic data sets (no CSV color maps)
    ├── LF2020_Elev_CONUS.tif     ← elevation [m]
    ├── LF2020_SlpD_CONUS.tif     ← slope [deg]
    └── LF2020_Asp_CONUS.tif      ← aspect [deg]
```

The file paths are configured in `landfire.ron` (WMS layers) and `lf_2025_conus.ron` (GeoDs layers), i.e. switching
to a new LANDFIRE release requires updating those configs.

### downloading

The data sets are distributed as zip files on the
[LANDFIRE full extent downloads page](https://landfire.gov/data/FullExtentDownloads) (select the CONUS region and the
respective theme/version). The download links follow the pattern
`https://landfire.gov/data-downloads/CONUS_LF<version>/LF<version>_<product>_CONUS.zip`, e.g.

```sh
wget https://landfire.gov/data-downloads/CONUS_LF2025/LF2025_EVT_CONUS.zip
```

Please check the download page for the current versions and links (especially for the topo data sets, which have
their own versioning). Note that the zip files are large (up to ~9GB for the topo data sets).

### extracting

Each zip file contains a `<name>/Tif/` directory with the GeoTIFF and a `<name>/CSV_Data/` directory with the CSV
color map (categorical data sets only), plus metadata we don't need. We only extract the `*.tif` and `*.csv`
files (`-j` junks the paths inside of the zip):

```sh
cd $ODIN_ROOT/data/odin_landfire/2025

for p in EVT FBFM40 CC CH CBH CBD; do
    unzip -j ~/Downloads/LF2025_${p}_CONUS.zip "LF2025_${p}_CONUS/Tif/LF2025_${p}_CONUS.tif" "LF2025_${p}_CONUS/CSV_Data/LF2025_${p}.csv"
done
```

```sh
cd $ODIN_ROOT/data/odin_landfire/topo

for p in Elev SlpD Asp; do
    unzip -j ~/Downloads/LF2020_${p}_CONUS.zip "LF2020_${p}_CONUS/Tif/LF2020_${p}_CONUS.tif"
done
```

The GeoTIFFs use the NAD83 / CONUS Albers projection with a 30m resolution, Int16 values and internal 128x128
tiles. The tiling is what allows fast access to small areas - do not convert them into striped (non-tiled)
formats such as PNG.

The zips also contain `*.tif.ovr` overview files, which are not used by `LandfireService` (the client only displays
the layers below a configurable camera height) and hence don't need to be extracted.

The WMS functionality is provided by the generic [`odin_gdal`](../odin_gdal/odin_gdal.md)`::wms_srv::WmsService`,
`LandfireService` adds the configuration, a reader for the landfire.gov CSV color maps and the routes. The CSV files
are used directly to colorize WMS layers and to create their legends (`landfire/cmap/<layer>`). Each WMS layer has one
or more styles (configured in `landfire.ron`, selected with the standard WMS `STYLES` parameter):

- `cmap` : colors from the landfire.gov CSV color map (categorical data sets)
- `alpha` : a single color per layer that is configured in `assets/odin_landfire_config.js`, with the value mapped to the
  alpha channel (values <= min are transparent, values >= max are opaque). This is used for the topo layers and
  optionally for the continuous canopy layers (CC, CH, CBH, CBD), for which zero is a valid (transparent) value

Shift-clicking on the globe looks up the data value at the clicked position for the selected source (WMS
GetFeatureInfo), selects the respective color map entry and shows the value underneath the color map list.

Nodata pixels - ocean, outside of CONUS or missing data - are rendered with a single configurable color
(`NODATA_COLOR` in `assets/odin_landfire_config.js`) so that valid data boundaries can be recognized. Nodata values are
the declared band nodata value, the "Fill-NoData" values of the CSV color maps and additional values configured per
layer in `landfire.ron`. The latter is required since many landfire.gov GeoTIFFs declare 32767 as nodata but
predominantly use -9999.

### custom color maps

Instead of the landfire.gov CSV files, layers can also use manually edited ODIN JSON color maps (`*.json`, in the same
`[{"color":"#rrggbb","code":"..","descr":".."},..]` format as the color map legends served by `landfire/cmap/<layer>`).
This is used to provide extended (unicode) descriptions that are not overwritten when updating the data sets, e.g.

```
( name: "LF2025_FBFM40", ..
  nodata: [-9999],
  styles: [ Cmap("$ODIN_ROOT/data/odin_landfire/2025/LF2025_FBFM40.json") ] ),
```

Entries with a "Fill-NoData" description are treated as nodata values. Since JSON color maps might not have such an
entry, the respective layer should configure its nodata values explicitly.

### seasonal updates

Landfire distributes seasonal updates for some data sets (e.g. `LF2025_FBFM40_FA26.tif`) that only cover parts of CONUS.
They can be combined with the standard data set by means of a GDAL VRT that uses the standard GeoTIFF as the first and
the update as the second `ComplexSource` (later sources take precedence). Each source needs a `<NODATA>` element with
the declared nodata value of the respective GeoTIFF so that cells outside of the update are taken from the standard
data set. Update GeoTIFFs must not use other values for areas without data (check with `gdalinfo -approx_stats -hist`).
The WMS layer `path` then refers to the VRT (e.g. `LF2025_FBFM40_CONUS.vrt`).

All configs that use FBFM40 (`landfire.ron` for display and `lf_2025_conus.ron` for simulation inputs) refer to the
`LF2025_FBFM40_CONUS.vrt`, i.e. display and simulations use the same (seasonally updated) fuel models.

Note that cached WMS images (`$ODIN_ROOT/cache/odin_landfire/wms/<layer>/`) have to be deleted when the data of a layer
changes.

### checking the setup

`wms_tile` renders a WMS image without starting the server, which can be used to check the data and the layer
configuration (and to see rendering times):

```sh
cargo run --release --bin wms_tile -- --size 512 LF2025_EVT -122.30,37.10,-121.95,37.45 evt.png
cargo run --release --bin wms_tile -- --size 512 --color ff4000 LF2020_SlpD -122.30,37.10,-121.95,37.45 slope.png
cargo run --release --bin wms_tile -- --size 512 --style alpha --color 00a000 --ndcolor ff00ff60 LF2025_CC -122.6,36.8,-121.8,37.6 cc.png
```

Layers that cannot be loaded are reported (and skipped) when the tool or the server starts. `show_landfire` runs a
server with just the `LandfireService` (and its Cesium dependencies).


## post-processing of LF2020 topo data

The `LF2020_{Elev,Asp,SlpD}_CONUS.tif` files have a `nodata` metadata value of 32767 for i16 bands but only use that
value for a small number of pixels (~4255). Around coastal areas they use zero values (-1 for aspect) for coastal
water and -9999 out at sea, meaning they have several de facto nodata values that throw off interpolation/regridding,
ending in negative values that in turn throw off clients such as NFDRS and WindNinja. This has to be fixed once after
extracting the files.

(1) set a consistent, band-type compatible nodata value:

```sh
gdal_edit -a_nodata -9999 -unsetstat <filename>
```

The `-unsetstat` is important to see the effects since the data sets are so large they store/cache the statistics.

(2) convert the remaining 32767 values into the declared nodata value:

```sh
swap_values --src 32767 --tgt-nodata [--dry-run] <filename>
```

This uses the `odin_gdal` `swap_values` tool (`--dry-run` only counts the matching pixels). Afterwards
`gdalinfo <filename>` should report `NoData Value=-9999`.
