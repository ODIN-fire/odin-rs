# odin_build

`odin_build` is a library crate that is used in a dual role both for utility functions called by ODIN crate specific 
`build.rs` scripts and at application runtime to locate resources and global dirs.

```
      ┌────────────────┐                                                            
      │crate odin_build│                                                            
      └──────┬─────────┘                                                            
             │          ┌─────────────────────┐                                     
             │          │ crate my_crate      │        [cargo]                            
             │          │                     │       $OUT_DIR (../target/❬mode❭/build/A-.../out/)                                  
             │          │   Cargo.toml (0)    │    ┌─────────────┐                  
             ╰──────────┼─► build.rs  ───(1)──┼───►│ config_data │                  
                        │   src/              │    └┬───▲──▲─────┘                  
                        │  ╭─ lib.rs ◄───(2)──┼─────╯   ╎  ╎
                        │  │  ...             │         ╎  ╎
                        │ (3) bin/            │         ╎  ╎
                        │  ╰─►  my_app.rs     │         ╎  ╎          [user]
                        │     ...             │         ╎  ╎        $ODIN_ROOT/         
                        │   config/ ╶╶╶╶╶╶╶╶╶╶┼╶╶╶╶╶╶╶╶╶╯  ╰╶╶╶╶╶╶     config/          
                        │     my_config.ron   │   internal or external    my_crate/             
                        └─────────────────────┘         resource             my_config.ron

```


### (0) declaration of embeddable resources in Cargo.toml
```toml
[[bin]]
name = "my_app"

[package.metadata.odin_configs]
my_config = { file="my_config.ron" }
...
[package.metadata.odin_assets]
my_asset = { file="my_asset.js", bins=["my_app"] }
```

### (1) creation of embedded resource data
```rs
 use odin_build;

 fn main () {
     odin_build::init_build();
     odin_build::create_config_data().expect("failed to generate config_data");
     odin_build::create_asset_data().expect("failed to generate asset_data");
 }
```

### (2) declaration of resource accessor
```rs
use odin_build::{define_load_config,define_load_asset};

define_load_config!{}
define_load_asset!{}
...
```

### (3) use of resources
```rs
...
fn main() {
    odin_build::set_bin_context();
    ...
    let config: MyConfig = load_config("my_config.ron")?;
    ...
    let asset: &Vec<u8> = load_asset("my_asset.js)?;
}
```


## Background

At build-time it is mostly used to generate source fragments for inlined resources. At runtime its main function
is the lookup/instantiation of resources. The algorithm for resource lookup is shared by both phases.

The `odin_build` crate is based on the four global types of data of an ODIN application:

1. configs - runtime invariant config data which can be spread over a number of locations, including in-memory
2. assets - runtime invariant data that is served/used by ODIN. Same locations as configs
3. data - global persistent data for ODIN applications. Requires a single runtime-invariant ODIN root-dir
4. cache - global transient data for ODIN applications. Also requires a single runtime-invariant ODIN root dir

`odin_build` uses two main directory types:

## ODIN Root Dir
An **root-dir** is a directory that contains runtime data kept outside of the source repository
This is a directory that potentially contains all of the aforementioned data types. Both `data/` and `cache/`
can only reside under a ODIN root-dir.
`odin_build` detects the root-dir to use in the following order:

1. whatever the environment variable ODIN_ROOT is set to
2. if `$ODIN_ROOT` is not set the source repository parent - if it contains any of `cache/`, `data/`, `configs/` 
   or `assets/` sub-dirs
3. a global `$HOME/.odin/` otherwise

An ODIN *root-dir* can optionally contain other sub-directories such as an ODIN *workspace-dir*.
This is to allow a self-contained directory structure during development (workspace root) and a global, configurable
directory structure in production (ODIN_ROOT or ~/.odin).

```
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
    └── ...                             optional dirs (e.g. ODIN workspace-dir)
```

## ODIN Workspace Dir
The **workspace-dir** is the top directory of the ODIN source repository, i.e. the directory into
which the Github repository was cloned.
While the primary contents of a *workspace-dir* are the ODIN crate sources, such crates can contain
configs and - more typically - assets in case those should be kept within the source repository.

```
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


## Data and Cache Directories

ODIN applications can write to both data/ and cache/ dirs (in whatever sub-dirs they choose). Applications have
full control over respective directory contents.

## Config and Asset Resources

Configs and assets are read-only *resources* and supposed to exist before starting an ODIN application.
They are looked up in the following order 

1. the ODIN root-dir, 
2. respective `configs/` or `assets/` subdirs of their crate (either resource-crate or bin-crate)
3. memory (compiled into bin)

Resources are identified by their defining crates and their filenames, to abstract their physical location.

In-memory (compiled in) resources are used to build stand-alone binaries that do not require any additional
files to start up. To enable in-memory resources for a crate its `Cargo.toml` manifest has to include 
metadata of the form

```toml
...
[package.metadata.odin_configs]
my_app = { file="my_app.ron" }
...
[package.metadata.odin_assets]
my_es_module = { file="my_es_module.js" }
...
```

Config resources are  different from assets in that they normally reside outside the source repository as they
are supposed to be modified without the need to rebuild applications and might contain information that should 
not be distributed with sources (e.g. user credentials).

The typical use of config data is to deserialize it into dedicated config structs at runtime. The crate that 
defines the struct is called the *config-crate*. Config data is stored in files and uses the
[Rust Object Notation](https://docs.rs/ron/latest/ron/).

The primary key for a config file is its filename. There are several alternative locations for each
config file which are checked in a priority order (specific overrides general).

Config files can be shared or bin specific. In the first case they are associated with the crate of the
called `load_config(..)`, i.e. `❬config-crate❭/❬filename❭`. If they are bin specific they are primarily associated 
with the crate of the bin, i.e. lookup uses `❬bin-crate❭/❬bin-name❭/❬config-crate❭/❬filename❭` for lookup. 
Bin specific configs override shared ones.

There are three root locations for external config lookup. Each location is first searched for 
a bin specific config and then for a shared config

1.  $ODIN_HOME/configs/ ( ❬bin-crate❭/❬bin-name❭/ )? ❬config-crate❭/❬filename❭
2.  ❬workspace-parent❭/configs/ ( ❬bin-crate❭/❬bin-name❭/ )? ❬config-crate❭/❬filename❭
3.  $HOME/.odin/configs/ ( ❬bin-crate❭/❬bin-name❭/ )? ❬config-crate❭/❬filename❭

Internal (in-repo) config files are located in `.../configs/` directories of their respective crates: 

1.  `❬workspace-dir❭/❬bin-crate❭/configs/❬bin-name❭/❬config-crate❭/❬filename❭` for bin specific configs
2.  `❬workspace-dir❭/❬confg-crate❭/configs/❬filename❭` for shared configs

External configs have preference over internal ones.
