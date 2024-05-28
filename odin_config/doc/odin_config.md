# odin_config

Many ODIN applications rely on configuration data that should not be hardcoded, either because it can change or because it
should not be public (although the code is open sourced). Examples are URIs and authentication data for external servers,
update/retrieval/cleanup schedules, number of data sets to keep in memory, list of external contacts to notify and many more.

We want to be able to access such data

- from configured or platform specific filesystem locations of the local machine
- from automatically generated binary data linked into the (stand-alone, single-executable) application
- in clear text or encrypted (user authenticated) form

The mechanism should only require to specify the target mode during build-time, without the need to change any of the source
code using the data.

The `odin_config` system crate uses [Cargo features](https://doc.rust-lang.org/cargo/reference/features.html) and 
[build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html) to support a single API to access configuration
from within applications. This is usually located in the main module of executables (main.rs) and involves four steps:

```rust
use odin_config::prelude::*;  // ① import odin_config macros
...
use_config!();  // ② inline generated config data
...
fn main() {
    ... run_configured(..config_for!("my_config")..) ...  // ③ instantiate structs from config data 
}
...
fn run_configured( .. config: MyConfig ...) { ... } // ④ use config structs
```

Steps ① and ② are not application specific and need to be done in the main application module.

Step ③ uses the `config_for!(«config-name»)` macro to instantiate respective structs from configuration data according
to the targeted build mode. The names/types of such structs are application specific.

All places using the configuration (step ④) can be completely agnostic as to what build mode is used - this is plain
Rust code referring to user defined structs.

Definition of application specific config structs uses the well established [Rust Serde](https://serde.rs/)
compile-time serialization/deserialization system through its `#[derive(Deserialize)]` attribute macro:

```rust
#[derive(Deserialize,...)]
pub struct LiveGoesRHotspotImporterConfig {
    pub satellite: u8,  // 16 or 18
    pub s3_region: String, // e.g. "us-east-1"
    pub bucket: String, // e.g. "noaa-goes18"
    pub source: String, // e.g. "ABI-L2-FDCC"
    pub init_files: usize, // number of most recent data files to retrieve
    pub cleanup_interval: Duration,
    ...
}
```

At this point we use the [Rusty Object Notation](https://docs.rs/ron/latest/ron/) text format to define the configuration 
values that are parsed by serde:

```ron
LiveGoesRHotspotImporterConfig(
    satellite: 18,
    s3_region: "us-east-1",
    bucket: "noaa-goes18",
    source: "ABI-L2-FDCC",
    init_files: 3,
    cleanup_interval: Duration(secs:3600,nanos:0)  // purge old files every hour
    ...
)
```

The target mode is specified by using Cargo features when building the application like so:
```shell
cargo build --features «config-mode» ...
```

Note that build features are transitive, i.e. the application crate `Cargo.toml` does *not* have define them
- it only needs a dependency entry for `odin_config`.

Currently `odin_config` supports 5 build modes via the following features:


### (1) `config_local` feature
This is the mode that is normally used during development, which locates config files either from a `ODIN_LOCAL` 
environment variable or a `./local/config` directory of the crate if no `ODIN_LOCAL` is set. Normally we use the
following directory layout:
```
local-odin/           # outside the odin_rs repository, in the same directory as odin_rs
   my_crate/
      config/
         myconfig.ron

odin_rs/              # from odin_rs source repository
   my_crate/          # odin_rs sub-crate from which the application is built
      src/bin/
          my_app.rs   # application to build
```

To build the application using the `local-odin` config files we run cargo from within the respective executable crate like so
(on a Linux/macOS system):

```shell
cd ~/odin_rs/my_crate
ODIN_LOCAL=../../local-odin/ cargo run --bin my_app
```

If the `ODIN_LOCAL` value does not end with a `/` then config files are directly looked up in that directory. If there is
a trailing '/' the system appends `«crate-name»/config` to locate the RON config files.

  
### (2) `config_xdg` feature 
This uses the cross-platform [XDG](https://specifications.freedesktop.org/basedir-spec/basedir-spec-latest.html) convention
to locate config files. This mode is useful for running applications in a controlled environment (e.g. a cloud server).


### (3) `config_embedded` feature 
This generates a `_config_data_.rs` file (via `odnin_config/build.rs`) with compressed bytes of the local config data which is
then inlined into the main application module. While this prevents directly reading the clear text from executable binaries
the data is not encrypted and could be extracted. This mode is mostly for executables that should be distributed as 
stand-alone binary files.

The config data that gets translated into the transient `_config_data_.rs` is located through a `IN_DIR` environment variable.
Respective config files have to be copied into this directory prior to the build. The location of the `_config_data_.rs` can
be controlled by an `OUT_DIR` environment variable, which is set by cargo to `odin_rs/target/` if not specified otherwise.

To build in this mode (using a `my-config-dir` to hold the config data outside the source repository) run cargo like so:
```shell
IN_DIR=../../my-config-dir  cargo build --features config_embeddded ...
```

All the following modes use the same `IN_DIR` environment setting to locate the configs to include. 


### (4) `config_embedded_pw` feature
This works similar to `config_embedded` but uses a build-time provided passphrase to encrypt the data:
```shell
ODIN_PP=my-super-secret-passphrase  cargo build --features config_embeddded_pw ...`
```
Upon startup of the application the user has to enter this passphrase (once - it is remembered for a short period of time
covering the executable start). Note the passphrase is *not* stored anywhere and hence cannot be retrieved from the application.
The default encryption uses aes256.

  
### (5) `config_embedded_pgp` feature
This is currently the most secure mode and generates a [PGP](https://en.wikipedia.org/wiki/Pretty_Good_Privacy) encrypted 
`_config_data_.rs` based on a user provided public key file. At runtime, the target application needs access to the users
private key and the user needs to enter the respective passphrase, i.e. this is a two-factor-authentication mechanism.
Since the build does not require the private key this avoids any shared secrets between developer and user.

Assuming the user's public key is stored in `«dir»/«user-name»_public.asc` the build command is:
```shell
ODIN_KEY=«dir»/«user-name»  cargo build --features config_embedded_pgp ...
```


Only modes (1) and (2) require to store configuration secrets on the machine running the application. All the embedded
modes avoid sharing such clear-text secrets (e.g. for accessing 3rd party servers) between developer and user.