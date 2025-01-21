# Design Principles

To keep a complex and multi-disciplinary framework such as odin-rs consistent we have to adhere to a set of
general design principles. The dominant ones for odin-rs are listed below.


## Use existing libraries

The use of odin-rs generally falls into the cross section of several application domains such as

- (web) server/client development
- serialization/deserialization
- geo-spatial processing
- physical computation
- data visualization and user interfaces
- asynchronous programming

The [Rust ecosystem](https://crates.io) contains substantial libraries for all these domains. Wherever
these libraries are stable, maintained, widely adopted and license compatible `odin-rs` should use them
to avoid not-invented-here syndrome. Not doing so means to dramatically increase the size of `odin-rs`
with functions that probably won't be based on the same domain expertise and won't be as well tested.

Using 3rd party libaries does come with caveats, namely dependency management and interface/type consistency.

To avoid [dependency/version hell](https://en.wikipedia.org/wiki/Dependency_hell) we have to ensure that 

(1) we use Rust crates instead of native libraries wherever possible so that we can rely on the Rust build
system to manage versions and features. This also means we can statically compile/link those dependencies
which greatly reduces the risk of version hell.

(2) we try to keep the number of 3rd party dependencies low by using only established crates.

To mitigate the interface/type consistency problem that comes with using partly overlapping 3rd party libraries
we use Rust language features, namely [traits](https://doc.rust-lang.org/book/ch10-02-traits.html) and the 
[*NewType* pattern](https://rust-unofficial.github.io/patterns/patterns/behavioural/newtype.html). The goal
is to use Rust's "zero cost abstraction" features to add adapters that imply minimal (if any) runtime costs.
The caveat here is to be aware where this might involve copying of aggregates and collections.

As of this time the strategic 3rd party crates used by `odin-rs` are:

- server/client development: [Axum](https://docs.rs/axum/latest/axum/) and [Reqwest](https://docs.rs/reqwest/latest/reqwest/)
- serialization/deserialization: [serde](https://serde.rs/)
- geo-spatial processing: [GeoRust](https://georust.org/) - esp. [geo](https://docs.rs/geo/latest/geo/) and [gdal](https://docs.rs/gdal/latest/gdal/,
  [nalgebra](https://nalgebra.org/))
- physical computation: [uom](https://docs.rs/uom/latest/uom/)
- asynchronous programming: [Tokio](https://tokio.rs/)

These are defined as workspace dependencies (in the `odin-rs` `Cargo.toml`) to make sure versions are compatible 
across all `odin-rs` sub crates.


The client code (browser scripts/modules) in `odin-rs` does not strictly follow the rule of using existing libraries. This code runs in many 
different environments (browsers, operating systems, hardware) and has to be loaded over the network so we have to minimize
the amount of required code by adhering to what we strictly need. This also means to limit the client code to user interface
related functions and performing as much data processing as possible on the server side.

Where a separation is not entirely possible (e.g. to serve client library specific data/code) respective `odin-rs` sub-crates have to
be very limited in scope and purpose, and are not allowed to be a dependency for non-client dependent ones (see [`odin_cesium`] example).

That said there are (a) readily available standard browser APIs we have to use in order to be platform/browser independent, and (b) complex
geospatial display libraries that cannot be re-implemented in `odin-rs`. The former is the [Document Object Model (DOM)](https://developer.mozilla.org/en-US/docs/Web/API/Document_Object_Model/Introduction) that is supported by contemporary browsers. The latter is the virtual globe display for which we use [CesiumJS](https://cesium.com/platform/cesiumjs/). This is a serious 3rd party dependency and hence extra care has to be
taken to not let it proliferate into the server. This is achieved with the following principle.


## Separate server- and client- side code

The primary purpose of the server is to import and process external data, and then serve it in a timely manner to connected clients.
The client code should only be concerned about visualization and user interface.

To that end communication between the two is using standard protocols and data formats, namely [HTTP](https://developer.mozilla.org/en-US/docs/Web/HTTP/Overview) and [JSON](https://www.json.org/json-en.html) over [websockets](https://en.wikipedia.org/wiki/WebSocket). The ideal
is to be able to re-implement each side without affecting the other.


## Use the Rust type system to enforce correct semantics

Many domain-specific 3rd party Rust libraries do abstract the memory type of variables (e.g. `f64`) but do little to enforce
compatible units of measure (e.g. SI vs. Imperial). As a simple example, the correct use of angles entails

- memory type (e.g. `f64`)
- units (degrees or radians)
- semantics (e.g. use as latitude or longitude)

Again we can use the Rust type system to our advantage. By means of using [uom](https://docs.rs/uom/latest/uom/) types (such as
`Length` based on SI and `f64`), and/or by using the [*NewType* pattern](https://rust-unofficial.github.io/patterns/patterns/behavioural/newtype.html) and overloadable Rust [`std::ops`](https://doc.rust-lang.org/std/ops/index.html) traits we can add specific types that catch most potential errors at compile time without introducing runtime overhead.

