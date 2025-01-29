# `odin_gdal`

[GDAL](https://gdal.org/en/stable/index.html) is the prevalent native (`odin-rs`-external) library to read, translate and modify geospatial data sets, supporting a wide range of formats (NetCDF, hdf5, tiff, png, jpeg, webp and many more). It is one of the key dependencies of `odin-rs`.

The role of `odin_gdal` is to simplify import and use of this complex library by means of adding utility functions and tools on top of
the underlying [gdal](https://crates.io/crates/gdal) crate that does the linking to the native gdal libraries.


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