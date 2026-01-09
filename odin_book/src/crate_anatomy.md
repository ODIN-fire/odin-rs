# Application Domain Crate Anatomy

Most application domain crates of ODIN provide the following generic functions to integrate external data into
ODIN applications:

1. periodic or scheduled retrieval of external data (satellite, ground sensor etc.)
2. translation of external data formats into internal data model (e.g. NetCDF into Rust objects)
3. async import & data availability notification with ["actors"](odin_actor/actor_basics.md) (message based internal data integration)
4. web (micro) service for browser based visualization

As a consequence the module structure crates follows a general pattern with the following main modules:

- library module (`lib.rs`)
- data import module (`live_connector.rs`)
- import actor module (`actor.rs`)
- microservice module (`service.rs`)

## File System Structure

The directory structure of ODIN crates is a standard [`Cargo`](https://doc.rust-lang.org/cargo/guide/project-layout.html)
package layout with three optional extensions:

```
.
└── odin-rs/
    └── <crate>/                     odin crate (system, domain or tool)
        ├── Cargo.toml               Cargo crate configuration
        ├── src/                     Rust source tree
        │   ├── lib.rs               crate module (data definitions, common functions)
        │   ├── actor.rs             data import actor
        │   ├── live_connector.rs    external data retrieval
        │   ├── service.rs           data layer microservice
        │   ├── errors.rs            crate error type & mapping
        │   └── bin/                 executable sources of this crate
        │       └── show_*.rs        minimal single page application for data layer
        ├── examples/                example sources of this crate
        │   └── *.rs
        ├── tests/                   test sources
        ├── resources/               test data
        ├── doc/                     crate documentation (also linked from odin_book)
        │   └── <crate>.md
        ├── configs/                 odin-rs specific module configuration sources (shared)
        │   └── <crate>.ron          shared module config (Rust Object Notation)
        └── assets/                  served assets of this crate (shared)
            ├── <crate>.js           data layer Javascript (browser) module
            ├── <crate>_config.js    shared data layer Javascript config module (rendering etc.)
            └── <shared assets> ...  icons, images etc.
```

The optional `configs/` directory holds [Rust Object Notation](https://docs.rs/ron/latest/ron/) files that are
used to initialize crate modules with configurable parameters (e.g. stable, public external URLs).

The optional `assets/` directory contains crate specific static files served by the microservice (`service.rs`). This
almost always includes a Javascript module (`<crate>.js`) as the browser-side counter part of the Rust microservice.
In case the data layer has complex rendering this is usually accompanied by a `<crate>_config.js` Javascript module
that contains configured parameters that control the rendering in the browser.

The optional `resources/` directory is less common and is mostly for static test data.

Since they are part of the source repository `configs/` and `assets/` should **only** hold configurations and assets that
do **not** contain any private information such as user credentials or non-public URLs. In case those are needed respective
files have to be kept outside the source repository in the global `$ODIN_ROOT/configs/<crate>/` and `$ODIN_ROOT/assets/<crate>/`
directories. See [`odin_build`](odin_build/odin_build.md) for details.

```
$ODIN_ROOT/
├── configs/
│   └── <crate>/
│       └── <crate>.ron             private crate config (Rust Object Notation)
├── cache/
│   └── <crate>/
│       └── <temp-data>...          transient (downloaded) crate data
├── assets/
│   └── <crate>/
│       └── <crate>_config.js       private data layer config (rendering, app keys etc.)
└── data/
    └── <crate>/
        └── <perm-data>...          permanent crate data (models etc.)
```

The repository external filesystem can also contain an optional `$ODIN_ROOT/data/<crate>/` directory for large persistent
files that are used by the crate (such as AI models or DEM tile images).

Lastly, the crate can also make use of an optional `$ODIN_ROOT/cache/<crate>/` directory to store transient files, e.g. for
imported external data with a limited lifespan.

## Crate Modules

Following the conventions described above there are four main modules for application domain ODIN crates:

- library module (`lib.rs`)
- data import module (`live_connector.rs`)
- import actor module (`actor.rs`)
- microservice module (`service.rs`)

We look at each of these module types separately.

### Library Module

The `lib.rs` module is a standard library crate module that defines which other modules are part of this crate (see
[Packages, Crates and Modules](https://doc.rust-lang.org/book/ch07-00-managing-growing-projects-with-packages-crates-and-modules.html)). The main responsibility of `lib.rs` is to define the ODIN internal data model for the imported data, which usually
involves two levels:

- **item level** - many external data sources break down into related sets of individual items such as
  hotspots or device sensor records. This data level is typically represented by a Rust `struct` or `enum`
  that also serves as the serialization/deserialization units, which uses the external [Serde](https://serde.rs/)
  crate for declarative (struct/field attribute macro based) parsing where appropriate. The ODIN data item types
  are the interface to the external data representation.
  In case the external data is in binary form (e.g. [`grib2`](https://www.nco.ncep.noaa.gov/pmb/docs/grib2/grib2_doc/) or
  [`NetCDF`](https://www.unidata.ucar.edu/software/netcdf)) there is usually just one item struct
  without a `Deserialize` derive macro. If the external format is text based ([JSON](https://www.json.org/json-en.html) or
  [CSV](https://en.wikipedia.org/wiki/Comma-separated_values)) there might be a raw item type for the sole purpose of
  deserialization/parsing, and an internal ODIN type that uses high level field types (e.g. [`uom`](https://crates.io/crates/uom/0.37.0)
  based types to capture units of measurement for length, area and similar physical quantities). For highly selective
  and performance critical parsing of CSV or (less likely) JSON input we have support functions and macros in `odin_common::u8extractor`.
- **item store level** - it is common that data items imported into ODIN have to capture a certain (configured) amount
  of history but since servers might be long running have to store that history in constant space. To that end we employ
  the [std::collections::VecDeque](https://doc.rust-lang.org/std/collections/struct.VecDeque.html) from Rust's standard
  library to implement time sorted ringbuffers, which is further supported by some utility functions in `odin_common::collections`.

The item store level is usually the one that is also transmitted to web clients, for which we prefer to use the
[JSON](https://www.json.org/json-en.html) format produced by the [`serde_json`](https://crates.io/crates/serde_json) crate.
Sometimes attribute based serialization is not suitable due to procedural components (translation, conditional omission etc.).
For such cases we provide more control over serialization by means of `odin_common::json_writer::JsonWriter` and the
associated `odin_common::json_writer::JsonWritable` trait.

The `lib` module also holds functions that are used throught other modules of this crate, such as low level network data
retrieval.

### Data Import Module

The `live_connector.rs` module is the async task that does the - usually periodic or scheduled - retrieval of external data and then parses it into ODIN item types. Availability of data sets is then announced through a [`odin_actor` message](odin_actor/odin_actor.md)
to an abstract `odin_actor::ActorHandle<M>` that is passed into the respective live connector constructor from the component
using the connector (e.g. an import actor).

Live connectors usually implement importer traits, i.e. they are referenced from the outside through abstract trait
interfaces that allow different implementations. The main reason for this is that the component using the connector
should not be aware of if this is live or recorded data. While ODIN as of 01/2026 does not yet support the RACE [data replay
infrastructure](https://nasarace.github.io/race/design/archive-replay.html) we plan to implement such support in the future and
therefore hide concrete connectors behind abstract interfaces.

### Import Actor Module

The `actor.rs` module has at least one [`odin_actor`](odin_actor/odin_actor.md) based [_Actor_](https://odin-fire.org/book/odin_actor/actor_basics.html) implementation that encapsulates a concrete connector / data acquisition task within a reusable, application agnostic actor that uses async messages to announce availability of data sets.

This module holds the specific actor state struct, which usually includes the following fields:

- an item store object as the internal data base (see `lib.rs` above)
- an async importer task which runs the connector that is passed into the actor constructor (see `live_connector.rs` above)
- [`odin_action`](odin_action/odin_action.md) fields for initialization and update callbacks. Those _actions_
  make the actor reusable (see [actor communication](https://odin-fire.org/book/odin_actor/actor_communication.html))

The module also defines the message interface of the actor, which usually includes the following messages:

- the `_Start_` system message to initialize and kick off the importer task
- an internal `Initialize` message sent from the importer when initial data is available. In response
  the actor initializes its store and invokes its init action
- an internal `Update` message sent from the importer when item updates become available. In response
  the actor updates the store and invokes the update action. The update action is usually set in the
  application main function that creates the actor system and broadcasts the serialized update items to all
  connected clients of a micro service
- an external `ExecSnapshotAction` message sent by other actors or clients (such as a micro service) to
  obtain the current snaphot of the actor store. Such actions are typically used by
  [`odin_server::spa::SpaService` micro services](https://odin-fire.org/book/odin_server/odin_server.html)
  implementations when a new user connects to the server and requires a serialized snapshot of the current data
  that is only sent to this new connection
- a `_Terminate_` system message for a controlled shutdown of the importer task

Since actors are components of long running systems they also have to guarantee the system is running in bounded space.
This might require periodic background tasks to clean up the store or the file system.

### Web App Microservice Module

The `service.rs` module contains a [`odin_server`](odin_server/odin_server.md) `SpaService` implementation that integrates the
import actor into web applications. Its main functions are:

- defining dependencies on other micro services (`add_dependencies(..)`)
- defining client (browser) side assets to serve (`add_components(..)`)
- defining the data initialization action (`data_available(..)`)
  this is the (internal) reaction to the initial data availability notification from the associated import actor
- defining the new connection request action (`init_connection(..)`)
  this is the reaction to incoming (external) server requests

The last two actions reflect a subtle data race - we might get the internal data availability notification before there
are users connected to the server, or we might get new user connections before the import actor is ready. In the first case
we can serve data right away when user requests come in. In the latter case we have to postpone serving that data until
we get the `data_available` notification. This requires to keep track of data initialization state and established connections
which is implemented in the `odin_server::spa::SpaServer`.

### Error Support Module

The `errors.rs` module is a simple support module to define a crate specific error type using the 3rd party [`thiserror`](https://docs.rs/thiserror/latest/thiserror/) crate. Since Rust does use [`Result`](https://doc.rust-lang.org/std/result/) return values for error
handling it is common practice to use a crate specific enum as a single stand-in type for all errors that could be encountered by
clients.

Since these are common Rust library crates we use a standard [Rust crate directory structure](https://doc.rust-lang.org/cargo/guide/project-layout.html) with three ODIN specific additions (`assets/`, `configs/`, `resources/`):

## Object (Runtime) Structure

The following diagram shows the runtime structure of the objects defined in these modules:

```
                                               odin ┆ external
$ODIN_ROOT/                                         ┆
  config/             ┏━━━━━━━━━━━━━━━━━━━━┓        ┆
    <crate>/          ┃ <data>Actor        ┃        ┆
      <config>.ron ───►                    ┃        ┆  ┌───────────┐
                      ┃  ┌──────────────────┐       ┆  │  external │
$ODIN_ROOT/      ┌───────┼ importer task  ◄─┼─────────►│   data    │
  cache/         │    ┃  └──────────────────┘       ┆  │  server   │
    <crate>/     │    ┃  ┌──────────────┐  ┃        ┆  └───────────┘
      <data>     └──────►│ <data>Store  │  ┃        ┆
                      ┃  └──────────────┘  ┃        ┆
                      ┃    update_action   ┃
                      ┗━━▲━━━━━━━━━━━━━━━━━┛
                         │      │
           exec_snapshot │      │
       ┌─────────────────┼──────┼──┐
       │ SpaServerActor  │      │  │
       │      ┏━━━━━━━━━━━━━━┓  │  │
       │      ┃ <data>Service┃  │  │
       │      ┗━━━━━━━━━━▲━━━┛  │  │
       └─────────────────┼──────┼──┘
                         │   wss│               server
    ╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌│╌╌╌╌╌╌│╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌
                         │      │               client
 ┌──────────────────┐   ┌┴──────▼────┐
 │ <crate>_config.js┼──►│ <crate>.js │
 └──────────────────┘   └────────────┘
```
