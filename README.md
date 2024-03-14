# ODIN - Open Data Integration Framework

**note - ODIN open source release is still pending. DO NOT DISTRIBUTE OUTSIDE NASA OR WITHOUT THIS WARNING**

## Introduction

This is the readme for the odin-rs repository, which is the Rust version of the Open Data Integration (ODIN) framework developed at the NASA Ames Research Center.

The goal of the ODIN framework is to efficiently create servers for static and dynamic data that support disaster management in order to improve situational awareness of responders. You can get an idea about ODIN applications by watching our [TFRSAC](https://fsapps.nwcg.gov/nirops/pages/tfrsac) presentations:

  * [sring 2023](https://www.youtube.com/watch?v=b9DfMBYCe-s&t=4950s)
  * [fall 2022](https://www.youtube.com/watch?v=gCBXOaybDLA)

ODIN-RS is a Rust based successor of the [race-odin](https://nasarace.github.io/race-odin/) system that was implemented in Scala/Java (see [RACE](https://nasarace.github.io/race/)). It is our intention to open source ODIN-RS under Apache v2 license.

At this point (03/2024) this repository holds work in progress. Not all crates do currently work and some might not even build.

POC: [Peter.C.Mehlitz\@nasa.gov](mailto:Peter.C.Mehlitz@nasa.gov) 

## Building ODIN-RS from source

This Rust repository contains a [Cargo workspace](https://doc.rust-lang.org/cargo/reference/workspaces.html) that consists of several sub-crates (`odin_actor`, `odin_config`, ..) that can be built or executed separately.

### Prerequisites

  1. [Rust toolchain](https://www.rust-lang.org/tools/install) - we recommend to manage the toolchain via `rustup`
     At this point ODIN-RS uses the nightly toolchain, which can be enabled via `rustup default nightly`

  2. [GDAL](https://gdal.org/) - this platform specific native library for handling geospatial data is required by the `odin_gdal` crate
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

## Further Documentation

Apart from [in-source Rust documentation](https://doc.rust-lang.org/rustdoc/how-to-write-documentation.html) for its crates, ODIN-RS will eventually include an `odin_book` sub-directory containing a separate user/developer guide that is based on the same [`mdbook`](https://rust-lang.github.io/mdBook/) tool that is used to generate standard Rust documentation.

At this early development stage the `examples` directories of respective crates might often hold the best documentation.