# Build and Install

This Rust repository contains a [Cargo workspace](https://doc.rust-lang.org/cargo/reference/workspaces.html) that consists of several sub-crates (`odin_actor`, `odin_config`, ..) that can be built or executed separately.

### Prerequisites

  1. Git VCS

  2. [Rust toolchain](https://www.rust-lang.org/tools/install) - we recommend to manage the toolchain via `rustup`
     At this point ODIN-RS uses the nightly toolchain, which can be enabled via `rustup default nightly`

  3. External (native) libraries
     Those might eventually be bundled in platform specific containers but for now have to be installed in the OS 
     - [GDAL](https://gdal.org/) - this platform specific native library for handling geospatial data is required by the `odin_gdal` crate
       and should be installed through respective package managers for your operating system, such as:

       * macOS: [homebrew](https://brew.sh/): `brew install gdal`
       * windows: [vcpkg](https://learn.microsoft.com/en-us/vcpkg/get_started/overview)


### Build instructions

Building and running ODIN-RS executables is normally done through the command line [`cargo`](https://doc.rust-lang.org/cargo/index.html) tool which is installed by `rustup` as mentioned above. While ODIN-RS can be built directly from the directory where this repository was cloned to we recommend to switch to the respective crate you are interested in, e.g.

```shell
$> cd odin_actor
$> cargo build --example hello_world
   Compiling proc-macro2 v1.0.79
   ...
     Finished `dev` profile [unoptimized + debuginfo] target(s) in 17.20s
     Running `.../odin-rs/target/debug/examples/hello_world`
hello world!
hello me!
```

For IDEs we recommend [Visual Studio Code with the Rust Analyzer extension](https://code.visualstudio.com/docs/languages/rust) - just choose "File->Open Folder" with the directory this repository was cloned to and you should be all set.
