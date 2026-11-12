# odin_dem

The `odin_dem` crate is a system crate that provides a [digital elevation model (DEM)](https://en.wikipedia.org/wiki/Digital_elevation_model).
This crate is basically a wrapper around the virtual [GDAL VRT](https://gdal.org/en/latest/drivers/raster/vrt.html) driver that is
used to extract elevation data sets for given rectangular bounding boxes from a large mosaic. Since ODIN has to be able to cover
areas around the globe the DEM [GeoTIFF](https://www.ogc.org/standards/geotiff/) or [Cloud-Optimized-GeoTIFF (COG)](https://www.usgs.gov/faqs/what-are-cloud-optimized-geotiffs-cogs) sources can be large (>350GB for CONUS at 1/3 arcsec). This is
not just a matter of available disk space but also of disk IO bandwidth (for random reads). Not all applications require the same
elevation resolution and accuracy (wildfire in general less so than flooding). For this reason ODIN can be configured to use
different represenations and comes with tools to generate both respective GeoTiff and VRT files. Please note the standard
`gdalbuildvrt` only covers a subset of those options and hence the ODIN tool `build_dem_vrt` should be preferred.


## Obtaining and Building the DEM VRT

Prior to using this crate you have to obtain respective source DEM image tiles, e.g. from
https://apps.nationalmap.gov/downloader. Be aware that such data sets can be large ([3dep](https://www.usgs.gov/3d-elevation-program) 1/3 arcsec data for CONUS takes about 350 GB of disk space). The `odin_dem` crate provides the `fetch_dem_files` tool to download a file list of tiles via HTTP but this can also be done through publicly available 3rd party tools.

DEM data is available from several servers such as

- https://apps.nationalmap.gov/downloader
- https://earthexplorer.usgs.gov/
- https://opendap.cr.usgs.gov/opendap/hyrax/SRTMGL1_NC.003/N00E006.SRTMGL1_NC.ncml.dmr.html

We do provide sample file list for standard areas in the `resource/` directory of this crate but be advised that such data
might change - you should retrieve your own DEM tiles from the above sites.


Once the DEM tiles have been retrieved they are (optionally) re-encoded with `encode_dem` and then referenced by a
[GDAL VRT](https://gdal.org/en/latest/drivers/raster/vrt.html) file that is created with `build_dem_vrt` (see
[DEM Encodings](#dem-encodings) below). This `*.vrt` file is the basis for extracting the DEM for regions of interest
such as incident areas, either as a synchronous function from within an existing server or through a simple standalone
[edge server](../intro.md#edge_servers).

The steps to retrieve tiles (using [GNU `wget`](https://www.gnu.org/software/wget/manual/wget.html)), encode them and
build the VRT are:

```shell
$> wget -nv -i <file-list>                                    # download original tiles into current dir
$> encode_dem -o <tile-dir> <download-dir>                     # encode (default U16), skips existing files
$> build_dem_vrt -o <tile-dir>/<region-name>.vrt <tile-dir>   # build the VRT (default: dem.ron vrt_path)
```

The original 3DEP Float32 tiles can also be referenced directly (without `encode_dem`) by an F32 VRT, they are already
tiled COGs with overviews. Do not use `gdalbuildvrt` for U16 tiles, it creates VRTs that return raw (encoded) values.

The default encoding and VRT path of the tools are taken from `$ODIN_ROOT/configs/odin_dem/dem.ron` (see
`odin_dem/configs/dem.ron` for a template), which is also the config file of `serve_dem`:

```rust
DemConfig(
    vrt_path: "$ODIN_ROOT/data/odin_dem/3dep13-conus-u16/3dep13-conus-u16.vrt",
    wms_capabilities_path: None, // if not set serve_dem does not serve WMS
    max_cache: 268435456,        // serve_dem file cache size
    encoding: U16                // U16 (default), I16 or F32
)
```


## DEM Encodings

ODIN supports three encodings for DEM source tiles:

| encoding | data type | heights | resolution | NoData | VRT band |
|---|---|---|---|---|---|
| `U16` (default) | UInt16 | `v * 0.1 - 500` (-499.9 .. 6053.5 m) | 0.1 m (±0.05 m) | 0 | Float32 m |
| `I16` | Int16 | `v` | 1 m (±0.5 m) | -32767 | Int16 m |
| `F32` | Float32 | `v` | lossless | -999999 | Float32 m |

`I16` used to be the only ODIN encoding. It is sufficient for wildfire related applications (which mostly depend on
averaged slope and aspect of fuel cells) but not for applications such as flood modeling, which need sub-meter
relative height precision. The Float32 precision of the original 3DEP tiles (~0.1mm at 2000m) on the other hand
exceeds the information content of the data by orders of magnitude - the absolute vertical accuracy of 3DEP 1/3 arcsec
data is in the order of 1m. `U16` keeps 0.1m resolution at a quarter of the original size.

All encodings produce the same tile layout: tiled GeoTIFFs with 512x512 blocks, ZSTD compression (with horizontal
or floating point predictor) and internal overviews for decimation levels 4, 8, 16, 32 and 64. The 2x overview level
is omitted since it accounts for about 2/3 of the overview size (averaged heights compress worse than the source) but
is not needed: resampled reads that would use it (e.g. Cesium terrain tiles at level 13, which sample about every
3.7 source pixels) only touch a few source blocks at full resolution.

The VRT created by `build_dem_vrt` always returns meters, i.e. clients do not need to know about the encoding. All VRT
sources have to use the same encoding and SRS but can have different resolutions (the VRT uses the finest one). Sources
are added in the order of the command line arguments (files of directories are sorted by name) and later sources
take precedence where they overlap, which allows to add higher resolution areas on top of coarser base layers.
`build_dem_vrt` warns about sources that do not have overviews, are not tiled or are not aligned with the VRT grid.

### U16 VRTs

GDAL can apply scale and offset of VRT sources (`ScaleOffset` and `ScaleRatio` elements of `ComplexSource`) but then
does not use source overviews for resampled reads. Reading a 65x65 grid from a 2x2 deg VRT with bilinear resampling took
10.9s instead of 0.08s. GDAL's virtual overviews (`OverviewList`) do not help. Derived bands read their sources at the
requested resolution but the `scale` pixel function requires band `Offset`/`Scale` metadata, which `gdalwarp` copies into
its (already unscaled) results so that clients such as QGIS would apply it again. U16 VRTs therefore consist of three files:

- `<name>.u16.vrt` - UInt16 mosaic of the sources (raw values, NoData 0)
- `<name>.u16x.vrt` - derived Float32 band that multiplies the mosaic by 0.1 (pixel function `mul`)
- `<name>.vrt` - derived Float32 band that adds -500 (pixel function `sum`), NoData -999999. This is the VRT to use

### Disk Space

Sizes of `encode_dem` results relative to the original 3DEP 1/3 arcsec tiles (LZW compressed Float32 COGs with 2x to
32x overviews), measured for tiles with flat (n39w085), mountainous (n38w120) and coastal (SF Bay Area) terrain:

| encoding | flat | mountainous | coastal (4 tiles) | CONUS (est., 967 tiles) |
|---|---|---|---|---|
| original Float32 | 100% | 100% | 100% | 339 GB |
| `F32` | 60% | 61% | 60% | ~205 GB |
| `U16` | 18% | 24% | 16% | ~65 GB |
| `I16` | 8% | 12% | 7% | ~30 GB |

Converting all CONUS tiles takes about 5s per tile and thread (e.g. less than 20 min with 8 threads, reading 339 GB
from a USB SSD takes about 6 min). The legacy ODIN Int16 tiles (DEFLATE, strips of 10812x1 pixels, no overviews) take
23 GB but cannot be used efficiently for resampled reads at low resolution (e.g. terrain tiles of a whole state).

Other evaluated options: lossless ZSTD Float32 (74% with 2x overviews), Int32 centimeters (40-52%). LERC compression
with a max error would be the purpose-built option for lossy Float32 but is not available in all GDAL builds.

### Runtime

The encoding is a trade-off between computation (decompression, unscaling) and disk IO. Measured on an Apple M4 Max (16 cores),
source files in the OS page cache (i.e. CPU only) unless noted otherwise:

| operation | original Float32 (LZW) | `F32` (ZSTD) | `U16` | `I16` |
|---|---|---|---|---|
| decode one 512x512 block (source file) | 1.70 ms | - | 0.79 ms (incl. 0.02 ms unscaling) | - |
| 65x65 tile from ~59x59 pixels (source file) | 1.72 ms | - | 0.77 ms | - |
| 65x65 tile from ~237x237 pixels (source file) | 1.10 ms | - | 0.58 ms | - |
| 65x65 grid from 2x2 deg (VRT, bilinear) | 0.10 s | 0.08 s | 0.08 s | 0.08 s |
| `gdalwarp` 40x40 km at 10 m (VRT) | 0.77 s ¹ | 0.62 s | 0.83 s | 0.57 s |
| `gdalwarp` 40x40 km at 100 m (VRT) | 0.46 s ¹ | 0.35 s | 0.56 s | 0.30 s |
| first 60 km ray (`get_terrain_hit`, VRT) | 90 ms ¹ | 30 ms | 40-45 ms | 25 ms |
| cached ray | 229 µs | 228 µs | 236 µs | 235 µs |

¹ originals read from a USB SSD (T7), not from the page cache

Unscaling itself is negligible (2.5% of decoding a block) and decoding UInt16/ZSTD is about 2x faster than decoding
the original Float32/LZW tiles. Through the U16 VRT, the two derived bands add about 30-50% CPU overhead compared to
the F32 VRT, i.e. for cached data `U16` is about as fast as the original tiles and slightly slower than `F32` tiles.
For data that is not in the page cache, `U16` has to read a quarter of the data (a USB SSD reads about 950 MB/s
sequentially but random 180 KB reads take ~1.8 ms, mostly latency), and a larger area of the DEM fits into the page
cache. Once blocks are in a `DemBlockCache` all encodings perform the same (blocks are stored as Float32).

### Encoding and VRT Tools

```shell
$> encode_dem [--encoding u16|i16|f32] [-j <jobs>] [--force] [--ovr-levels 4,8,16,32,64|none] -o <out-dir> <file-or-dir>..
$> build_dem_vrt [--encoding u16|i16|f32] [--force] [-o <vrt-file>] <file-or-dir>..
```

`encode_dem` writes to `<file>.part` and renames on success, i.e. interrupted runs can be continued (existing output
files are skipped unless `--force` is set). It reports max encoding errors and clamped values (heights outside of the
encoding range) for each file. `build_dem_vrt` does not overwrite existing VRTs without `--force`.

The respective library functions are `odin_dem::encode_dem_file(..)` and `odin_dem::build_dem_vrt(..)`.


## DEM extraction

The basic function of `odin_dem` is the synchronous

```rust
fn get_dem (bbox: &BoundingBox<f64>, srs: DemSRS, img_type: DemImgType, vrt_file: &str) -> Result<(String,File)>
```

which takes the target [Spatial Reference System (SRS)](https://en.wikipedia.org/wiki/Spatial_reference_system), the
bounding box (in SRS coordinates), the required result image type and the path to the `*.vrt` file (mosaic directory) to
use and returns a `std::fs::File` of the extracted DEM.

This function can be called synchronously (no async operation involved) but - depending on the size of the VRT and/or the
DEM to retrieve - it can take up to several seconds to execute.

The `get_dem` command line tool is a simple wrapper around this function.

DEMs with a given resolution are extracted with `get_res_dem` (local VRT), `query_res_dem` (`serve_dem` server) or
`DemSource::get_res_dem` (either one), which take the GDAL resampling algorithm (`odin_dem::ResampleAlg`) as a parameter:

- `ResampleAlg::NearestNeighbour` preserves terrain roughness, which is preferable for coarse grids such as WindNinja meshes
- interpolating algorithms (`Bilinear`, `Cubic`) or `Average` are more suitable for high resolutions and derived products
  such as contours or viewsheds, where nearest neighbour sampling creates noise

The algorithm is part of the cached DEM filename. The `serve_dem` `GetResDem` request has a respective optional
`resample` parameter (GDAL name such as `near`, `bilinear` or `cubic`), which defaults to `near`.


## Serving a DEM

Since the underlying DEM data for this crate does require large amounts of disk space we provide a simple stand alone
[edge server](../intro.md#edge_servers) to run on dedicated machinery. This edge server needs to be configured
with the path of the `*.vrt` file that references the tile data.


## Terrain Ray Casting

The `raycast` and `blockcache` modules compute the first intersection of rays (e.g. camera view directions) with the
terrain. The main use is to compute the geo location of image points (e.g. from image segmentation) of cameras with
known position and view direction - see `Camera::locate_image_points(..)` in [odin_alertca](../odin_alertca/odin_alertca.md).

Rays are specified as `RayQuery` objects and computed by the process that has the local DEM, i.e. this is part of the
`DemSource` interface:

```rust
let queries = vec![ RayQuery::from_ground( lon, lat, height_above_ground, bearing, pitch, max_dist), ..];
for hit in dem_src.ray_intersections( &queries).await? { // Vec<Option<RayHit>>
    if let Some(hit) = hit { println!("terrain hit at {},{} ({:.0}m, distance {:.0}m)", hit.lon, hit.lat, hit.height, hit.distance) }
}
```

For `DemSource::File` this uses a process wide `DemBlockCache` for the DEM file, for `DemSource::Server` it sends a
`RayIntersections` request to the respective `serve_dem` server, which uses its own block cache:

```
POST <host>:<port>/RayIntersections
  body:     [ {"lon":..,"lat":..,"height":..,"aboveGround":true,"bearing":..,"pitch":..,"maxDist":..,"curvCoeff":..}, ..]
  response: [ {"lon":..,"lat":..,"height":..,"distance":..} | null, ..]
```

`aboveGround` (default false) means `height` is relative to the DEM terrain at the ray origin, otherwise it is in the
vertical datum of the DEM (normally MSL). `curvCoeff` is optional (default 0.85714). Requests fail if a ray origin is
outside of the DEM or below the terrain.

### Ray model

- the ray height at horizontal distance `d` is `h0 + d·tan(pitch) - d²·k/(2R)`, with `R` being the earth radius at the
  origin latitude and `k` the curvature coefficient (`1 - refraction`, default 0.85714). This is the same model as used
  by the GDAL viewshed computation (see [odin_gdal](../odin_gdal/odin_gdal.md))
- the ray is sampled in steps of half the DEM pixel size, using bilinear interpolation of terrain heights, i.e. narrow
  terrain features cannot be missed. The exact intersection is computed by bisection between the last two samples

### DemBlockCache (native DEM)

The `DemBlockCache` samples the source DEM (which has to use a geographic SRS, as our VRTs do) in its native resolution.
It does not build grids but lazily loads only the source blocks (e.g. 128x128 pixels for the 3DEP VRT) the ray passes
through until it hits the terrain. Loaded blocks are kept in a LRU cache (default max 1024 blocks, i.e. 64MB for 128x128
blocks) so that subsequent queries for the same area (e.g. other image points of the same camera) do not have to read
them again. The horizontal ray track is the geodesic from the origin with the given bearing.

### DemGrid (UTM grid)

A `DemGrid` is an in-memory UTM raster around a position that can be obtained from any `DemSource` with
`DemGrid::around(..)`, e.g. to cast rays on remote DEM servers that do not support `RayIntersections` queries. Bearings
are converted into grid bearings by correcting for the grid convergence (which can be several degrees at the edges of
UTM zones), UTM scale distortions (< 0.1%) are ignored.

### Performance

Measured on the 3DEP 1/3 arcsec VRT (~10m) for 40km rays:

| | first query (cold) | per ray (cached) |
|---|---|---|
| `DemBlockCache` | 40-100ms (only blocks along the ray) | 20-150µs (5m steps) |
| `serve_dem` `RayIntersections` | same + HTTP | + ~0.1ms per request |
| `DemGrid` (80x80km at 30m) | 3.4s (warp), 40ms if grid file is cached | 2-10µs (15m steps) |

Results of both implementations agree within the resampling differences (tens of meters at 30km on real terrain,
< 1m on smooth synthetic terrain). `DemBlockCache` is the better choice for single (or few) queries per area and uses
the full DEM resolution, `DemGrid` is better for very many queries in the same area.

The `get_terrain_hit` tool can be used to test intersections and runtimes (`--grid` uses a `DemGrid`, `--dem` can be a
VRT file or a `serve_dem` URL):

```shell
$> cargo run --release --bin get_terrain_hit -- --lon -118.620451 --lat 34.122602 --height 10 --bearing 336.5 \
                                                --pitch -3 --pitch-uncertainty 0.5 --repeat 100
first query of 3 rays (including DEM access): 42.1ms
repeated queries: 18.1µs per ray
 hit (pitch -3.00): -118.64081,34.16152 height 296.4m at 4707.4m
near (pitch -3.50): -118.63886,34.15779 height 283.1m at 4256.6m
 far (pitch -2.50): -118.64608,34.17159 height 283.5m at 5926.1m
```
