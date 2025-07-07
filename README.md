# ODIN - Open Data Integration Framework

## Introduction

This is the readme for the odin-rs repository, which is the Rust version of the Open Data Integration (ODIN) framework developed at the NASA Ames Research Center.

ODIN is a software framework to efficiently create servers that support disaster management. 

More specifically it is a framework to make it easy to import and process an open number of external data sources for information such as weather, ground-/aerial- and space-based sensors, threat assessment, simulation and tracking. The over-arching goal is to improve situational awareness of responders by making more - and more timely - information available in stakeholder-specific applications. ODINs goal is *not* to create yet another website running in the cloud. To that end it is open sourced under Apachev2, highly extensible and supports running ODIN servers within stakeholder organizations on a variety of hardware and operating systems.

The online documentation for odin-rs can be found on https://odin-fire.org/book (or built from `odin_book` in this repository).

To get an idea of what such ODIN servers might look like we refer to two of our TFRSAC talks:

  * [spring 2023](https://www.youtube.com/watch?v=b9DfMBYCe-s&t=4950s)
  * [fall 2022](https://www.youtube.com/watch?v=gCBXOaybDLA)

ODIN-RS is a Rust based successor of the [race-odin](https://nasarace.github.io/race-odin/) system that was implemented in Scala/Java (see [RACE](https://nasarace.github.io/race/)). This is still an on-going effort - please contact us if you are interested in the current status.

POC: [Peter.C.Mehlitz\@nasa.gov](mailto:Peter.C.Mehlitz@nasa.gov) 

## Building ODIN-RS from source

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

  3. [GDAL](https://gdal.org/) - this platform specific native library for handling geospatial data is required by the `odin_gdal` crate and should be installed through respective package managers for your operating system

     * Linux: gdal packages are available for all major Linux distributions through their native package managers
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

For IDEs we recommend [Visual Studio Code with the Rust Analyzer extension](https://code.visualstudio.com/docs/languages/rust) - just choose "File->Open Folder" with the directory this repository was cloned to and you should be ready to go.

## Further Documentation

Many ODIN applications require configuration and asset data that is not part of its source distribution. To learn about how to set up ODIN development systems, how to deploy production applications and about the details of existing ODIN sub-crates please refer to the `odin_book` which is part of the odin-rs repository and can be built/viewed through [`mdbook`](https://rust-lang.github.io/mdBook/) which is part of the Rust toolchain:

```shell
$> cd odin_book
$> mdbook serve
2024-07-18 10:07:57 [INFO] (mdbook::book): Book building has started
2024-07-18 10:07:57 [INFO] (mdbook::book): Running the html backend
2024-07-18 10:07:57 [INFO] (mdbook::cmd::serve): Serving on: http://localhost:3000
...
```

The online version of `odin_book` is available on https://odin-fire.github.io/odin-rs/

Please also look at the `examples/` directories of various ODIN crates. 

## License

Copyright © 2024, United States Government, as represented by the Administrator of the National Aeronautics and Space Administration. All rights reserved.

The “ODIN” software is licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0.

Unless required by applicable law or agreed to in writing, software distributed under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. See the License for the specific language governing permissions and limitations under the License.
