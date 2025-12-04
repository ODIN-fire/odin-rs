# Build and Install

This Rust repository contains a [Cargo workspace](https://doc.rust-lang.org/cargo/reference/workspaces.html) that consists of several sub-crates (`odin_actor`, `odin_config`, ..) that can be built or executed separately.

### Prerequisites

  1. [Git](https://git-scm.com/) - the version control system that is by now ubiqitous 

  2. [Rust toolchain](https://www.rust-lang.org/tools/install) - we recommend to manage the toolchain via `rustup`
     As of Rust 1.89 ODIN-RS uses the stable toolchain. To get it (locally) install `rustup` and execute:

     ```shell
     $> curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
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

     Update when new Rust versions (announced on the [Rust blog](https://blog.rust-lang.org/)) are released by executing `rustup update`.

     To install the [`mdbook`](https://rust-lang.github.io/mdBook/) tool to compile and serve online documentation
     please run

     ```shell 
     $> cargo install mdbook
     ```

     If you are new to Rust you can find documentation and tutorials on [https://www.rust-lang.org/learn](https://www.rust-lang.org/learn). 
     Information about the vast Rust ecosystem of available 3rd party libraries is available on [https://crates.io](https://crates.io). 

  3. [GDAL](https://gdal.org/) - this native library is required if you run applications that use  [odin_gdal](odin_gdal/odin_gdal.md) to
     process external input such as satellite data. The basic examples (e.g. from `hello_world` from [odin_actor`](odin_actor/odin_actor.md)) 
     do not require it so you can leave this to the [Next Steps](#next-steps) section below but ultimately you probably need it for general
     odin-rs development so we recommend to install it upfront. GDAL should be installed through the native package manager of your system:
  
     * Linux: gdal packages are available for all major Linux distributions through their native package managers.
       Please note that Ubuntu 20.04 only supported old versions of GDAL which might require to [install/build from source](https://gdal.org/en/latest/development/building_from_source.html#building-from-source)
     * macOS: [homebrew](https://brew.sh/): `brew install gdal` - **make sure to install homebrew in its default location (`/opt/homebrew/`
       on Apple silicon) to avoid build problems with various GDAL dependencies**
     * windows: [vcpkg](https://learn.microsoft.com/en-us/vcpkg/get_started/overview)

  4. odin-rs sources - downloadable via [Git](https://git-scm.com/) from [https://github.com/ODIN-fire/odin-rs](https://github.com/ODIN-fire/odin-rs):

     ```shell
     $> git clone https://github.com/ODIN-fire/odin-rs
     ```

### Directory Structure

Since many ODIN applications require configuration or other data files at runtime it is recommended to keep the repository
and such files under a single root directory. To conform with the [`odin_build`](odin_build/odin_build.md) crate we recommend the
following structure:

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

Resource directories (configs/, assets/ and data/) can be populated upon demand later-on - please refer to the 
[`odin_build`](odin_build/odin_build.md) documentation for further details.

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
   Compiling ...
   ...
     Running `.../odin-rs/target/debug/examples/hello_world`
hello world!
```

For IDEs and editors we recommend:

- [Visual Studio Code with the Rust Analyzer extension](https://code.visualstudio.com/docs/languages/rust) - just choose "File->Open Folder" 
  with the directory this repository was cloned to and you should be all set
- [Zed](https://zed.dev/) - as a more editor oriented but faster GUI alternative (Zed is implemented in Rust)
- [Helix](https://helix-editor.com/) - is a text-mode editor (i.e. works over ssh) that is implemented in Rust and can be installed as part
  of the Rust toolchain

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
Once the mdbook server is running you can view the latest version of the `odin_book` contents in any browser at http://localhost:3000 


### Next Steps

Most likely you are interested in `odin-rs` to run web applications. If those involve importing external geospatial data (e.g. NetCDF files)
and/or visualization on a virtual globe there are two additional 3rd party dependencies: 

- [GDAL](https://gdal.org/)
- [CesiumJS](https://cesium.com/platform/cesiumjs/)

The first one is required to read/process many external geospatial data sets such as GOES-R hotspots or NOAA HRRR weather forecasts. There are native GDAL packages for Linux, macOS and Windows but the names depend on your native package manager (e.g. [homebrew](https://brew.sh/) on macOS. [vcpkg](https://vcpkg.io/) on Windows, or `apt-get` on Ubuntu Linux).

On macOS using [homebrew](https://brew.sh/) this is:

```shell
brew install gdal
```

The [CesiumJS](https://cesium.com/platform/cesiumjs/) install is optional. Per default build option respective `odin-rs` applications proxy the
CesiumJS server but for production environments it is recommended to download and strip the distribution to speed up load times and reduce
network downloads. The [`odin_cesium`](odin_cesium/odin_cesium.md) crate contains a `install_cesium` tool that can be used like so:

```shell
# from within odin-rs/
cd odin_cesium
mkdir -p ../../assets/odin_cesium
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

If you open a browser tab on `http://localhost:9009/goesr` it should display a virtual globe with live updated hotspots
detected by [GOES-R](https://www.goes-r.gov/) satellites (see [odin_goesr](odin_goesr/odin_goesr.md) for details). If you
click on the last icon in the upper left corner you should see a window showing the lates GOES-R data sets. If this shows
live data entries it means 

- external data access is working (not blocked by firewall etc.)
- your GDAL installation to read this data is working
- the CesiumJS browser library is working

You can terminate the server with Ctrl-c.

As a last step you can test your local CesiumJS installation (obtained through `install_cesium`) by re-running the same
application with the respective `cesium_asset` build feature and release mode optimizations:

```shell
 cargo run --features cesium_asset --release --bin show_goesr_hotspots
```

At this point you should have a fully functional `odin-rs` development system.


### Known Installation Pitfalls

#### MacOS

##### wrong or Missing Xcode command line tools
On MacOS Rust does require reasonably updated Xcode command line tools. This problem manifests itself in different ways (e.g. CC link errors)
but early on. You can verify outside of `odin-rs` by running the `cargo new my_test; cd my_test; cargo run` test mentioned above. 

The xtools command line tools can be installed as part of Xcode from the Apple AppStore or - if you already have Xcode - by running `xcode-select –-install`.

##### native GDAL package install fails
[GDAL](https://gdal.org/) is a native library for geospatial image processing with a huge dependency set (tiff, jpeg, png, hdf5, netcdf etc.) and hence is updated quite frequently. It should be installed and updated through a native package manager, e.g. [homebrew](https://brew.sh/).

Since GDAL itself has a lot of dependencies it is highly recommended to use a standard [homebrew](https://brew.sh/) installation (which on Apple silicon is in `/opt/homebrew/`). Non-standard locations might force buiding packages from source, which is prone to fail for complex packages such as python. While it is possible to build and install GDAL manually - and to configure `odin-rs` accordingly - we do not recommend this as it would still require a working `homebrew` for the GDAL dependencies. Please refer to the [`odin_gdal`](odin_gdal/odin_gdal.md) documentation
for how to build/use GDAL libraries from source.

As of 11/20/2025 the current Rust [gdal v0.18.0](https://crates.io/crates/gdal) crate that is used as a wrapper around the native `GDAL` library does not compile due to changes in `GDAL 3.12.0`. Until the Rust project releases a new version the workaround is either to install an older `GDAL 3.11.x` version through the native package manager (e.g. homebrew) or to fall back in the `odin-fire` workspace `Cargo.toml` to directly use the GDAL repository URL in the dependency spec like so:

```toml
...
#gdal = { version = "0.18", features = ["array", "bindgen"] }  # <<< reinstate when a new crate version is released
#gdal-sys = { version = "0.11", features = ["bindgen"]}        # <<< reinstate when a new crate version is released
gdal = { git = "https://github.com/georust/gdal.git", features = ["array", "bindgen"] }
gdal-sys = { git = "https://github.com/georust/gdal.git", features = ["bindgen"] }
...
```

Alternatively you can install an older `gdal` package through your native package manager. On a Mac using the `homebrew` package manager this
will require manual intervention as `homebrew` formulas usually just support the latest versions. To install manually follow these steps:

1. download the historical version to install from <https://github.com/Homebrew/homebrew-core/blob/main/Formula/g/gdal.rb> (use
"history" button to the right, then select the "view code at this point" file symbol)
2. run `brew install <downloaded-gdal.rb>`
3. if this refuses to link gdal run `brew unlink gdal; brew link gdal --force`

Be aware of that unless you do a subsequent `brew pin gdal` this will be overwritten each time you upgrade outdated packages. In general it
is recommended not to interfere with the native package manager versions as this can quickly cause 
["dependency hell"](https://en.wikipedia.org/wiki/Dependency_hell).