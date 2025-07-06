# Overall Project Structure

As the name implies `odin-rs` is a [Rust](https://www.rust-lang.org/) project that uses [Cargo](https://doc.rust-lang.org/cargo/)
as its primary build tool. This mostly defines the directory structure.

The `odin-rs` project is structured as a [Cargo workspace](https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html) project
that contains 

- system- and application domain crates (e.g. `odin_actor`)
- tool crates (e.g. `mkslides`)
- the ODIN book (`odin_book`)

The primary purpose of the workspace is to ensure all crates use consistent versions of the key external dependencies
(e.g. libraries such as [tokio](https://docs.rs/tokio/latest/tokio/)). Since `odin-rs` has a large number of 3rd party
dependencies it is crucial to define them in one place (the toplevel `Cargo.toml`).

## Toplevel Directory Structure

```
.
└── odin-rs/
    ├── Cargo.toml               Cargo workspace configuration
    ├── odin_<name>/             odin-rs crates (see below)
    ├── ...
    └── odin_book/               sources of this documentation
        ├── book.toml            mdbook configuration
        ├── src/                 book source tree
        │   ├── <crate-doc>/     chapters (linking to crate/doc/)
        │   ├── *.md
        │   └── img/             diagrams
        └── theme/               book theme 
```

## Crate Directory Structure

```
.
└── odin-rs/
    └── odin_<crate>/            odin crate (system, domain or tool)
        ├── Cargo.toml           Cargo crate configuration
        ├── src/                 Rust source tree
        │   ├── *.rs
        │   └── bin/             executable sources of this crate
        │       └── *.rs
        ├── examples/            example sources of this crate
        │   └── *.rs
        ├── tests/               test sources
        ├── resources/           test data
        ├── doc/                 crate documentation (also linked from odin_book)
        │   └── *.md
        ├── configs/             odin-rs specific module configuration sources (shared)
        │   └── *.ron              in Rust Object Notation
        └── assets/              served assets of this crate (shared) 
            └── *.js, *.svg, ..
```