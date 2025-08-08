# odin_build

`odin_build` is a library crate that is used in a dual role both for utility functions called by ODIN crate specific 
[build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html) and at application runtime to locate resources and global directories.

## Background
The primary use of ODIN is to create servers - either interactive web-servers or edge-servers used by other applications. To that end ODIN servers support four general categories of data: 

1. configs - essential runtime invariant configuration data (e.g. URIs and user credentials for external servers)
2. assets - essential runtime invariant data that is served/used by ODIN (e.g. CSS and Javascript modules for web servers) 
3. data - global persistent data for ODIN applications that can change independently of the ODIN application using them
4. cache - global transient data for ODIN applications (e.g. cached responses from proxied servers)

Configs and assets are essential **resources**, i.e. applications can rely on their existence (but not their values). For `data` and `cache` we only guarantee that respective directories exist at runtime - the use of those directories is up to individual applications.

Common to all categories is that such data can change independently of the ODIN Rust sources using them and hence do need a consistent, well defined lookup mechanism used throughout all ODIN applications. That mechanism is implemented in `odin_build`, mostly through four functions:

- `❬crate❭::load_config<C> (file_name: &str)->Result<C>` (`C` being the type of the requested, deserializable config struct)
- `❬crate❭::load_asset (file_name: &str)->Result<Bytes>`
- `odin_build::data_dir()->&'static PathBuf`
- `odin_build::cache_dir()->&'static PathBuf`

The reason why the first two functions reside in the crates defining respective resources is that we also support stand-alone ODIN applications that can be distributed as single executable files, without the need to install (potentially sensitive) resource files (e.g. containing user authentication for 3rd party servers). This seems to be incompatible with that resource values can be changed independently of ODIN Rust sources. 

To reconcile the two requirements we support a general build mode for ODIN applications that takes (at build-time) resource files and generates statically linked Rust sources from them. Generating source fragments for such **embedded resources** is done by build scripts utilizing functions provided by `odin_build`. The data flow is as follows:


```diagram
      ┌────────────────┐                                                            
      │crate odin_build│                                                            
      └──────┬─────────┘                                                            
             │          ┌─────────────────────┐                                     
             │          │ crate my_crate      │        [cargo]                            
             │          │                     │      $OUT_DIR (../target/❬mode❭/build/A-../out/)                                  
             │          │   Cargo.toml (0)    │    ┌─────────────┐                  
             ╰──────────┼─► build.rs  ───(1)──┼───►│ config_data │                  
                        │   src/              │    └┬───▲──▲─────┘                  
                        │  ╭─ lib.rs ◄───(2)──┼─────╯   ╎  ╎
                        │  │  ...             │         ╎  ╎
                        │ (3) bin/            │         ╎  ╎
                        │  ╰─►  my_app.rs     │         ╎  ╎          [user]
                        │     ...             │         ╎  ╎        $ODIN_ROOT/         
                        │   configs/ ╶╶╶╶╶╶╶╶╶┼╶╶╶╶╶╶╶╶╶╯  ╰╶╶╶╶╶╶     configs/          
                        │     my_config.ron   │   internal or external    my_crate/             
                        └─────────────────────┘         resource             my_config.ron

```

This involves several steps:

### (0) declaration of embeddable resources in Cargo.toml manifest of owning crates
The first step is to specify package meta data for embeddable resource files in the crates owning them (henceforth called **resource crate**):

```toml
[[bin]]
name = "my_app"

[package.metadata.odin_configs]
my_config = { file="my_config.ron" }
...
[package.metadata.odin_assets]
my_asset = { file="my_asset.js", bins=["my_app"] }

[features]
embedded_resources = []
...
```

The `embedded_resource` feature should be transitive - if the resource crate in turn depends on other ODIN resource crates we have to pass-down the feature like so: `embedded_resources = ["❬other-odin-crate❭/embedded_resources" …]`

### (1) creation of embedded resource data
This step uses a build script of the resource crate to generate embedded resource code by calling functions from `odin_built`, showing its role as a build-time library crate:

```rs
 use odin_build;

 fn main () {
     odin_build::init_build();
     odin_build::create_config_data().expect("failed to generate config_data");
     odin_build::create_asset_data().expect("failed to generate asset_data");
 }
```

Note that using embedded resources requires the `embedded_resources` [feature](https://doc.rust-lang.org/cargo/reference/features.html) when building resource crates since it involves conditional compilation (more specifically feature-gated `import!(❬embedded-resource-fragment❭)` calls).

ODIN stores all embedded resource data in compressed format. Depending on resource file type data might be minified before compression.

### (2) declaration of resource accessor functions in resource crates
At application runtime we use two macros from `odin_build` that expand into crate-specific public `load_config(…)` and `load_asset(…)` functions mentioned above.

```rs
use odin_build::{define_load_config,define_load_asset};

define_load_config!{}
define_load_asset!{}
...
```

If the application was built with the `embedded_resources` feature the expanded `load_config(…)` and `load_asset(…)` functions conditionally import the resource code fragments. 

### (3) use of resources
Using resource values at runtime is done through calling the expanded `load_config(…)` and `load_asset(…)` functions, which only require abstract resource filenames (not their location). The application source code is fully independent of the build mode:

```rs
fn main() {
    odin_build::set_bin_context();
    ...
    let config: MyConfig = load_config("my_config.ron")?;
    ...
    let asset: &Vec<u8> = load_asset("my_asset.js")?;
}
```

## Resource Lookup
We use the same algorithm for each individual resource file lookup during build-time and application run-time. This algorithm is implemented in `odin_build::find_resource_file(…)` and based on two main directory types of ODIN:

- **root directories**
- **workspace directories**

### ODIN Root Dir
A **root-dir** is a directory that contains resource data that is kept outside of the source repository. ODIN applications are not supposed to rely on anything outside their root-dir but the user can control which root-dir to use (there can be several of them, e.g. for development and production)

We detect the root-dir to use in the following order:

1. whatever the optional environment variable `ODIN_ROOT` is set to
2. the parent of a workspace dir **iff** the current dir is (within) an ODIN workspace and this parent contains any of `cache/`,
   `data/`, `configs/` or `assets/` sub-dirs. This is to support a self-contained directory structure during development, not
   requiring any environment variables
3. a `$HOME/.odin/` otherwise - this is the normal production mode

An ODIN **root-dir** can optionally contain other sub-directories such as the ODIN **workspace-dir** mentioned below.

```diagram
.
└── ❬odin-root-dir❭/
    ├── configs/                        read-only data deserialized into config structs
    │   ├── ❬resource-crate❭/
    │   │   ├── ❬resource-file❭
    │   │   └── ...
    │   └── ❬bin-crate❭/
    │       └── ❬resource-crate❭/
    │           ├── ❬resource-file❭     bin specific override
    │           └── ...
    ├── assets/                         read-only binary data served by ODIN app
    │   ├── ❬resource-crate❭/...
    │   └── ❬bin-crate❭/...
    │
    ├── data/                           persistent runtime data for ODIN apps
    │   └── ...
    │
    ├── cache/                          transient runtime data for ODIN apps
    │   └── ...
    │
    └── ... (e.g. odin-rs/)             optional dirs (ODIN workspace-dir etc.)
```

### ODIN Workspace Dir
The **workspace-dir** is the top directory of an ODIN source repository, i.e. the directory into which the `odin-rs` Github repository was cloned. While the primary content of a **workspace-dir** are the ODIN crate sources, such crates *can* contain
configs and assets in case those should be kept within the source repository. This is typically the case for crates that serve/communicate with Javascript module assets - here we want to make sure asset and related ODIN Rust code are kept together.

The **workspace-dir** is the topmost dir that holds a Cargo.toml, starting from the current dir.

A **workspace-dir** follows the normal cargo convention but adds optional `configs/` and `assets/` sub-directories to respective workspace crates:

```diagram
.
└── ❬odin-workspace-dir❭/
    ├── Cargo.toml                     ODIN workspace definition    
    ├── ❬crate-dir❭/
    │   ├── Cargo.toml                 including odin_configs and odin_assets metadata
    │   ├── build.rs                   calling odin_build functions
    │   ├── src/...                    normal Cargo dir structure
    │   │
    │   ├── configs/                   (optional) in-repo config resources for this crate
    │   │   ├── ❬resource-file❭
    │   │   └── ...
    │   └── assets/                    (optional) in-repo asset resources for this crate
    │       ├── ❬resource-file❭
    │       └── ...
    ├── ...                            other ODIN crates
    └── target/...                     build artifacts   
```


With those directory types we can now define the resource file lookup algorithm:

### File Lookup Algorithm
For each given tuple

 - root-dir (ODIN_HOME | workspace-parent | ~/.odin)
 - (optional) workspace-dir 
 - resource type ("configs" or "assets"), 
 - resource filename, 
 - resource crate and 
 - (optional) bin name + crate
 
 check in the following order:

1. root-dir / resource-type / bin-crate / bin-name / resource-crate / filename
2. root-dir / resource-type / resource-crate / filename
3. workspace-dir / resource-type / bin-crate / bin-name / resource-crate / filename
4. workspace-dir / resource-type / resource-crate / filename

This is implemented in the `odin_build::find_resource_file(…)` function which returns an `Option<PathBuf>`.

### Runtime Resource Lookup Algorithm

At application runtime we optionally extend the above file system lookup mechanism by checking for an embedded resource within the resource-crate **iff** no file was found with the above algorithm.

By setting a runtime environment variable `ODIN_EMBEDDED_ONLY=true` we can force the lookup to only consider embedded resources (i.e. to ignore resource files in the file system).

This lookup is performed for each resource separately, i.e. it is not just possible but even usual to have resources to reside in different locations (root dir and workspace dir). Typically only configs with user settings or credentials are kept outside the repository whereas assets are kept within. The main exception would be development/test environments.

## ODIN Environment Variables

At runtime, ODIN applications use the following optional environment variables:

- `ODIN_HOME` - the ODIN root directory to use
- `ODIN_EMBEDDED_ONLY` - use only embedded configs, no file system lookup
- `ODIN_BIN_SUFFIX` - optional suffix for binary name (can be used to differentiate multiple concurrent 
   `ODIN_BIN_NAME`/`CARGO_BIN_NAME` processes)
- `ODIN_RELOAD_ASSETS` - if set asset lookup is not cached (useful for debugging javascript modules)

Note that if you use `ODIN_HOME` to run applications outside of a workspace-dir (i.e. outside of a clones repository directory) you 
have to make sure your application does not rely on a config or asset that is normally kept in the repository - all configs and assets
have to be copied into `ODIN_HOME` in this case. 

At build-time, ODIN uses the following environment variables to provide build script input

- `ODIN_BIN_CRATE` - set manually or by ODIN build tool
- `ODIN_BIN_NAME` - set manually or by ODIN build tool
- `ODIN_EMBED_RESOURCES` - set manually or by ODIN build tool
- `OUT_DIR` - automatically set by cargo
- `CARGO_PKG_NAME` - automatically set by cargo
- `CARGO_BIN_NAME` - automatically set by cargo for bin target


## ODIN build tools

To further simplify building applications with embedded resources `odin_build` includes a tool that automates setting required environment variables, calling cargo and reporting embedded files:

```manpage
bob [--embed] [--root ❬dir❭] [❬cargo-opts❭...] ❬bin-name❭
  --embed      : build binary with embedded resources
  --root ❬dir❭ : set ODIN root dir to embed resources from
```

Using this tool is optional. ODIN applications can be built/run through normal cargo invocation but in this case resources are not embedded without manually setting the above `ODIN_..` build-time environment variables and the `embedded_resources` feature.

Although provided by the `odin_common` crate the `duplicate_dir` command line tool can be used to duplicate nested `ODIN_ROOT` directory trees. Use
the `--link-files` option to create root dirs that only override some config/asset files and otherwise link to an existing root dir:

```manpage
duplicate_dir [FLAGS] [OPTIONS] <source-dir> <target-dir>

FLAGS:
    -h, --help          Prints help information
    -l, --link-files    only use symbolic (soft) links for files
    -V, --version       Prints version information

OPTIONS:
    -e, --exclude <exclude>...    exclude file or directory matching glob

ARGS:
    <source-dir>    root directory to duplicate
    <target-dir>    directory to duplicate to (will be created/overwritten)
```