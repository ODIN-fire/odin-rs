# odin_dem

The `odin_dem` crate is a system crate that provides a [digital elevation model (DEM)](https://en.wikipedia.org/wiki/Digital_elevation_model).
This crate is basically just a wrapper around the virtual [GDAL VRT](https://gdal.org/en/latest/drivers/raster/vrt.html) driver that is
used to extract elevation data sets for given rectangular bounding boxes from a large mosaic.


## Obtaining and Building the DEM VRT

Prior to using this crate you have to obtain respective source DEM image tiles, e.g. from
https://apps.nationalmap.gov/downloader. Be aware that such data sets can be large ([3dep](https://www.usgs.gov/3d-elevation-program) 1/3 arc
sec data for CONUS takes about 300 GB of disk space). The `odin_dem` crate provides the `fetch_dem_files` tool to download a file list of tiles 
via HTTP but this can also be done through publicly available 3rd party tools.

DEM data is available from several servers such as

- https://apps.nationalmap.gov/downloader
- https://earthexplorer.usgs.gov/
- https://opendap.cr.usgs.gov/opendap/hyrax/SRTMGL1_NC.003/N00E006.SRTMGL1_NC.ncml.dmr.html

We do provide sample file list for standard areas in the `resource/` directory of this crate but be advised that such data
might change - you should retrieve your own DEM tiles from the above sites.


Once the DEM tiles have been retrieved the [GDAL `gdalbuildvrt`](https://gdal.org/en/latest/programs/gdalbuildvrt.html) tool has to be
used to create a `*.vrt` file from the downloaded DEM tiles. This `*.vrt` file is the basis for extracting the DEM for regions
of interest such as incident areas, either as a synchronous function from within an existing server or through a simple standalong
[edge server](../intro.md#edge_servers).

Manual steps to retrieve tiles and build the VRT using the publicly available

- [GNU `wget`](https://www.gnu.org/software/wget/manual/wget.html)
- [GDAL `gdalbuildvrt`](https://gdal.org/en/latest/programs/gdalbuildvrt.html)

cross-platform command line tools are:

```shell
$> wget -nv -i <file-list>
$> gdalbuildvrt <region-name>.vrt *.tif
```


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
