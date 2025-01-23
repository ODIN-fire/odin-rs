# Build and Install

This Rust repository contains a [Cargo workspace](https://doc.rust-lang.org/cargo/reference/workspaces.html) that consists of several sub-crates (`odin_actor`, `odin_config`, ..) that can be built or executed separately.

### Prerequisites

  1. [Git](https://git-scm.com/) - the version control system that is by now ubiqitous 

  2. [Rust toolchain](https://www.rust-lang.org/tools/install) - we recommend to manage the toolchain via `rustup`
     At this point ODIN-RS uses the nightly toolchain. To get, (locally) install `rustup` and switch to the nightly toolchain execute:

     ```shell
     $> curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
     ...
     $> rustup default nightly
     ``` 

     To check if the basic Rust installation is working correctly you can create, build and run a simple test project by executing

     ```shell
     $> cargo new my_test
         Creating binary (application) `my_test` package
     $> cd my_test
     $> cargo run
         Compiling my_test v0.1.0 ...
         ...
         Running `target/debug/my_test`
     Hello, world!
     ```

     Periodic updates of the toolchain can be done by executing `rustup update`

     To install the [`mdbook`](https://rust-lang.github.io/mdBook/) tool to compile and serve online documentation
     please run

     ```shell 
     $> cargo install mdbook
     ```

     If you are new to Rust you can find documentation and tutorials on [https://www.rust-lang.org/learn](https://www.rust-lang.org/learn). 
     Information about the vast Rust ecosystem of available 3rd party libraries is available on [https://crates.io](https://crates.io). 

  3. [GDAL](https://gdal.org/) - this native library is optional - it is only required if you run applications that use 
     [odin_gdal](odin_gdal/odin_gdal.md) to process external input such as satellite data. The basic examples (e.g. from `hello_world`
     from [odin_actor`](odin_actor/odin_actor.md)) do not require it so you can leave this to the [Next Steps](#next-steps) section below
     but ultimately you probably need it for general odin-rs development so we recommend to install it upfront. GDAL should be installed through 
     the native package manager of your system:
  
     * Linux: gdal packages are available for all major Linux distributions through their native package managers.
       Please note that Ubuntu 20.04 only supported old versions of GDAL which might require to [install/build from source](https://gdal.org/en/latest/development/building_from_source.html#building-from-source)
     * macOS: [homebrew](https://brew.sh/): `brew install gdal`
     * windows: [vcpkg](https://learn.microsoft.com/en-us/vcpkg/get_started/overview)

  4. odin-rs sources - downloadable via [Git](https://git-scm.com/) from [https://github.com/ODIN-fire/odin-rs](https://github.com/ODIN-fire/odin-rs):

     ```shell
     $> git clone https://github.com/ODIN-fire/odin-rs
     ```

### Directory Structure

Since many ODIN applications require configuration or other data files at runtime it is recommended to keep the repository
and such files under a single root directory. To conform with the `odin_build` crate we recommend the following structure:

```
.
└── ❬odin-root-dir❭/                    created before cloning odin-rs
    ├── configs/...                       read-only data deserialized into config structs
    ├── assets/...                        read-only binary data served by ODIN app
    ├── data/...                          persistent runtime data for ODIN apps
    ├── cache/...                         transient runtime data for ODIN apps
    │
    └── odin-rs/...                     ⬅︎ directory into which ODIN source repository is cloned
```

The name of the ❬odin-root-dir❭ can be chosen at will. You can have several root dirs with different odin versions/branches and/or resource files. An installation as outlined above does not require any environment variables to be set.

Resource directories (configs/, assets/ and data/) can be populated upon demand later-on - please see the [odin_build] documentation for further details.

On a Unix/macOS system this amounts to a sequence of commands like:
```shell
$> mkdir odin
$> cd odin
$> mkdir configs assets data cache
$> git clone https://github.com/ODIN-fire/odin-rs  # or other odin-rs repository URL
...
$> cd odin-rs
```

### Build instructions

Building and running ODIN-RS executables is normally done through the command line [`cargo`](https://doc.rust-lang.org/cargo/index.html) tool which is installed by `rustup` as mentioned above. While ODIN-RS can be built directly from the directory where this repository was cloned to we recommend to switch to the respective crate you are interested in, e.g.

```shell
$> cd odin_actor
$> cargo run --example hello_world
   Compiling proc-macro2 v1.0.79
   ...
     Running `.../odin-rs/target/debug/examples/hello_world`
hello world!
```

For IDEs we recommend [Visual Studio Code with the Rust Analyzer extension](https://code.visualstudio.com/docs/languages/rust) - just choose "File->Open Folder" with the directory this repository was cloned to and you should be all set.

To build/browse this documentation you have to install the Rust [`mdbook`](https://rust-lang.github.io/mdBook/) tool:
```shell
$> cargo install mdbook
...
$> cd odin_book
$> mdbook serve
2024-07-18 10:07:57 [INFO] (mdbook::book): Book building has started
2024-07-18 10:07:57 [INFO] (mdbook::book): Running the html backend
2024-07-18 10:07:57 [INFO] (mdbook::cmd::serve): Serving on: http://localhost:3000
...
```
Once the mdbook server is running you can view the odin_book contents in any browser at http://localhost:3000 


### Next Steps

Most likely you are interested in `odin-rs` to run web applications. If those involve importing external geospatial data (e.g. NetCDF files)
and/or visualization on a virtual globe there are two additional native dependencies: 

- [GDAL](https://gdal.org/)
- [CesiumJS](https://cesium.com/platform/cesiumjs/)

The first one is mandatory if external data is involved (such as GOES-R hotspots or NOAA HRRR weather forecasts). There are native GDAL packages
for Linux, macOS and Windows but the names depend on your native package manager (e.g. [homebrew](https://brew.sh/) on macOS. [vcpkg](https://vcpkg.io/) on Windows, or `apt-get` on Ubuntu Linux).

On macOS it is:

```shell
brew install gdal
```

The [CesiumJS](https://cesium.com/platform/cesiumjs/) install is optional. Per default build options respective `odin-rs` applications proxy the
CesiumJS server but for production environments it is recommended to download and strip the distribution. The [`odin_cesium`](odin_cesium/odin_cesium.md) crate contains a `install_cesium` tool that can be used like so:

```shell
cd odin_cesium
cargo run --bin install_cesium
```

This should leave you with a populated `../../assets/odin_cesium/cesiumjs/` directory. Since we do this to have a working production
environment it is also recommended to get a free [Cesium Ion access token](https://ion.cesium.com/tokens?page=1), copy the default
`ODIN-ROOT/odin-rs/odin_cesium/assets/odin_cesium_config.js` to `ODIN-ROOT/assets/odin_cesium/` and edit it to set

```javascript
Cesium.Ion.defaultAccessToken = "<YOUR ACCESS TOKEN HERE>";
...
```

You can read about *assets* and *configs* directories in [odin_build](odin_build/odin_build.md) and about Cesium in 
[odin_cesium](odin_cesium/odin_cesium.md). Other applications/crates (such as `odin_sentinel`) can require more assets and configs.

The above steps should be enough to run the next install test:

```shell
$> cd .../odin_goesr
$> cargo run --bin show_goesr_hotspots
...
    Running `target/release/show_goesr_hotspots`
serving SPA on http://127.0.0.1:9009/goesr
```

Now you can open a browser tab on http://localhost:9009/goesr which should display a virtual globe with live updated hotspots
detected by [GOES-R](https://www.goes-r.gov/) satellites (see [odin_goesr](odin_goesr/odin_goesr.md) for details).

At this point you should have a fully functional `odin-rs` development platform for a wide gamut of applications.



### Known Installation Pitfalls

#### MacOS

##### wrong or Missing Xcode command line tools
On MacOS Rust does require reasonably updated Xcode command line tools. This problem manifests itself in different ways (e.g. CC link errors)
but early on. You can verify outside of `odin-rs` by running the `cargo new my_test; cd my_test; cargo run` test mentioned above. 

The xtools command line tools can be installed as part of Xcode from the Apple AppStore or - if you already have Xcode - by running `xcode-select –-install`.

##### native GDAL package install fails
[GDAL](https://gdal.org/) is a native library for geospatial image processing with a huge dependency set (tiff,jpeg,png,hdf5,netcdf) and hence
changes quite frequently. It should be installed and updated through a native package manager, e.g. [homebrew](https://brew.sh/):

```shell
$> brew install gdal
...
$> brew upgrade gdal
```

However, the homebrew gdal package is building from source and requires even more 3rd party dependencies such as python3 and some of these
like to collide with libraries that come preinstalled on macOS (depending on its version). Although `odin-rs` does not use those dependencies
they can break the gdal homebrew install. 

Three ways to solve this:

- wait for a homebrew update to fix the broken package (it usually just takes a couple of days to get fixed)
- edit the offending formula to avoid the broken package (through `homebrew edit ...` but requires some homebrew knowledge)
- build gdal from source and tell odin-rs to use your version

The last option is the preferred one as it is easy to switch back to using the native package manager once the 3rd party dependency is fixed
and it allows you to tailor its dependencies (`odin-rs` only requires a fraction of them to be installed).

Apart from the Xcode command line tools you still need a native package manager to install the `cmake` and `proj` packages but those rarely make problems. You can get the GDAL sources from https://github.com/OSGeo/gdal.git and follow its [build instructions](https://gdal.org/en/stable/development/building_from_source.html) but it boils down to the following sequence:

```shell
brew install cmake proj  # if those aren't installed yet

mkdir -p ~/libraries
cd ~/libraries
git clone https://github.com/OSGeo/gdal.git
cd gdal
mkdir build
cd build
cmake -DCMAKE_BUILD_TYPE=Release -DCMAKE_CXX_STANDARD=17 -DCMAKE_CXX_FLAGS="-I$HOME/homebrew/include" \
-DBUILD_PYTHON_BINDINGS=OFF -DBUILD_JAVA_BINDINGS=OFF -DBUILD_APPS=OFF -DCMAKE_INSTALL_PREFIX=install ..
cmake --build .
... this will take a while
cmake --build . --target install
```

This should leave you with a gdal installation in `./install/{include,lib}`.

To direct `odin-rs` to use your GDAL library you have to set two environment variables that should be present as long as you don't want to go back to a native package manager version:

```shell
export GDAL_HOME=$HOME/libraries/gdal/build/install
export DYLD_LIBRARY_PATH=$GDAL_HOME/lib:$DYLD_LIBRARY_PATH
```

To make this permanent you can set them in your `~/.profile`. Make sure your GDAL_HOME directory is not deleted or moved as long as you want to use your GDAL version as it would otherwise prevent your `odin-rs` applications to load.

The upside of this is that with some additional steps it also allows to build and link a static GDAL lib, which can considerably easy distribution of `odin-rs` applications