# Documentation

The `odin-rs` project follows the single source principle, i.e. the repository contains all the sources of its
respective documentation and all required tools to render content that are not part of the standard Rust toolchain.

The primary tool to generate documentation is [mdBook](https://rust-lang.github.io/mdBook/), the main source
formats are [markdown](https://rust-lang.github.io/mdBook/format/markdown.html) for textual and 
[SVG](https://en.wikipedia.org/wiki/SVG) for graphical content.

Each crate has its own `doc/` subdirectory that holds at least the toplevel markdown file for this crate (e.g. 
`odin_himawari/doc/odin_himawari.md`). If applicable this file should also be usable for [`rustdoc`](https://doc.rust-lang.org/rustdoc/what-is-rustdoc.html) generated source documentation.

The `odin_book/` directory integrates all these crate doc files into a standalone document that is created with 
[mdBook](https://rust-lang.github.io/mdBook/). The book configuration file can be found in `odin_book/book.toml`. Each
documented crate is represented by its own `src/⟨crate⟩` subdirectory (e.g. `odin_book/src/odin_himawari`) which holds
at least a link to the crate markdown file but can also contain additional material/pages that are not included in
`rustdoc` content.

We do provide a ODIN specific CSS file (`odin_book/src/odin.css`) which mostly holds image style classes.

We also use our own `odin_book/theme/index.hbs` page template file to add custom book title rendering including a logo. This might change
in the future since `mdbook` versions can have different template files (e.g. breaking compatibility between 0.4.x and 0.5.x)
that require to retrieve and modify the template file for the chosen `mdbook` version. The current template is compatible
with `mdbook` v0.5.2.
