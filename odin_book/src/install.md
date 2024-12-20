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

     Periodic updates of the toolchain can be done by executing `rustup update`

     To install the [`mdbook`](https://rust-lang.github.io/mdBook/) tool to compile and serve online documentation
     please run

     ```shell 
     $> cargo install mdbook
     ```

     If you are new to Rust you can find documentation and tutorials on [https://www.rust-lang.org/learn](https://www.rust-lang.org/learn). 
     Information about the vast Rust ecosystem of available 3rd party libraries is available on [https://crates.io](https://crates.io). 

  3. [GDAL](https://gdal.org/) - this platform specific native library for handling geospatial data is required by the
     `odin_gdal` crate and should be installed through respective package managers for your operating system

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
