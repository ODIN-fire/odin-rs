# odin_dem

The `odin_dem` crate is a system crate that provides a [digital elevation model (DEM)](https://en.wikipedia.org/wiki/Digital_elevation_model).
This crate is basically just a wrapper around the virtual [GDAL VRT](https://gdal.org/en/latest/drivers/raster/vrt.html) driver that is
used to extract elevation data sets for given rectangular bounding boxes from a large mosaic.


## 1. Obtaining and Building the DEM VRT

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


## 2. DEM extraction

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


## 3. Serving a DEM

Since the underlying DEM data for this crate does require large amounts of disk space we provide a simple stand alone
[edge server](../intro.md#edge_servers) to run on dedicated machinery. This edge server needs to be configured
with the path of the `*.vrt` file that references the tile data.