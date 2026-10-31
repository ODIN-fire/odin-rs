# `odin_gdal`

[GDAL](https://gdal.org/en/stable/index.html) is the prevalent native (`odin-rs`-external) library to read, translate and modify geospatial data sets, supporting a wide range of formats (NetCDF, hdf5, tiff, png, jpeg, webp and many more). It is one of the key dependencies of `odin-rs`.

The role of `odin_gdal` is to simplify import and use of this complex library by means of adding utility functions and tools on top of
the underlying [gdal](https://crates.io/crates/gdal) crate that does the linking to the native gdal libraries.


### WMS Server

The `wms_srv` module provides a generic [WMS](https://www.ogc.org/standards/wms/) implementation for large (tiled)
GeoTIFF datasets that computes GetMap images on demand, i.e. without derived data products such as tile sets. It
supports the WMS subset required by `Cesium.WebMapServiceImageryProvider` (GetMap for WMS 1.1.1 and 1.3.0, PNG images,
GetFeatureInfo with GeoJSON responses) plus generated GetCapabilities. Requested areas are warped (nearest neighbour, preserving the data cells) into memory
and colorized according to the requested WMS style:

- `cmap` : value lookup tables for categorical 8 or 16 bit integer data. Color maps are data source specific and read
  by a `cmap::CmapReader` function that is provided by the respective data source crate
- `alpha` : a single color (vendor specific `COLOR=RRGGBB` parameter) with the value mapped to the alpha channel, for
  continuous integer or float data. The value range is configured and can be overridden with `VRANGE=min,max`

Nodata pixels (declared band nodata, configured nodata values, nodata color map entries and NaN) are rendered with the
vendor specific `NDCOLOR=RRGGBBAA` color (transparent by default). `wms_srv::WmsService` contains the request handlers
for WMS and color map legend requests, including image caching, a configurable limit for concurrent image computations
and transparent images for requests that are much coarser than the data resolution. Data source crates only have to
add the configuration (`wms_srv::WmsConfig`) and the routes for their `odin_server::spa::SpaService` implementation -
see [`odin_landfire`](../odin_landfire/odin_landfire.md) for an example.

GetFeatureInfo responses are GeoJSON FeatureCollections with one Point feature per queried layer that has `layer`,
`value` (null for nodata), `nodata` and `name` properties (color map entry name for categorical data). Values are read
from the original datasets, i.e. they do not depend on any rendering parameters.

The pixel buffer colorization functions are in the `cmap` module.

### Building GDAL from Source

Although it is recommended to install GDAL through native platform package managers it is possible to build from source, which
is available on https://github.com/OSGeo/gdal.git an documented [here](https://gdal.org/en/stable/development/building_from_source.html).

You can either build a shared or a static gdal library, but keep in mind that a static build requires to add - depending on build configuration -
all referenced sub-dependencies (tiff, netcdf, hdf5, jpeg etc.). This can be a daunting task.

The procedure to do a shared library build follows this scheme:

```shell
mkdir ~/opt
cd ~/opt
git clone https://github.com/OSGeo/gdal.git
...
cd gdal
mkdir build
cd build
cmake -DCMAKE_BUILD_TYPE=Release -DBUILD_PYTHON_BINDINGS=OFF -DBUILD_JAVA_BINDINGS=OFF -DBUILD_APPS=OFF -DCMAKE_INSTALL_PREFIX=install ..
...
cmake --build .
...
cmake --build . --target install
...
```

Depending on the native package management that was used for installing GDAL dependencies you might have to add cmake build opts such
as `-DCMAKE_CXX_FLAGS="-I/opt/homebrew/include`.

To use the library in your `odin-rs` builds you need to set respective environment variables:

```shell
export GDAL_VERSION=...
export GDAL_HOME=$HOME/opt/gdal/build/install # based on example above - adapt to your install location
export DYLD_LIBRARY_PATH=$GDAL_HOME/lib
```

Since we change an external library it is recommended to run `cargo clean` before any subsequent `cargo build` or `cargo run`.