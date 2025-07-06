# Conventions

This page describes naming and structure conventions used througout the whole `odin-rs` project.


### crate names

There are two main categories of crates within `odin-rs`:

- system- and application domain crates - those should be named with a `odin_` prefix (e.g. `odin_actor`)
- common tools - those are not necessarily restricted to `odin-rs` and hence should be named after
  their purpose. In most cases they contain just a single executable and hence should be named after it
  (e.g. `mkslides` for the slide deck generator tool)


### Rust guidelines

We follow the conventions as laid out in the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/naming.html)
and exemplified in [The Rust Programming Language](https://doc.rust-lang.org/book/).

For starters this means:

- upper *CamelCase* names for types, enum variants and traits
- lower *snake_case* names for functions and modules


### function size

Although we don't impose hard limits to function size those should be kept small so that they can be viewed without
scrolling (< 50 loc as a ballpark number). Larger functions should be broken down if it enhances readability. An
exception to this rule are flat functions without much structure but with a high cohesion (e.g. large match expressions).


### code sections

Although module sources should not exceed manageable size (as a rough guideline < 1000 loc>) we encourage
to group module code into semantic sections (e.g. "auxiliary functions"):

```.rs
...
/* #region auxiliary functions ***********************************************************/
...
/* #endregion auxiliary functions */
...
```

This not only visibly groups items but can also be used by IDEs to fold/unfold respective sections (e.g. in
[VS Code](https://code.visualstudio.com/) with the [maptz.regionfolder](https://marketplace.visualstudio.com/items?itemName=maptz.regionfolder)
extension)


### order of top level items

This pertains to the order of type-, trait- and function- definitions inside of modules, which should follow these
rules:

- outside-in - exported top level items should be at the top, in the order of importance (main concepts first)
- call order - function definitions should follow the order in which they call each other
- keep related toplevel items together - the primary intention of this rule is to mimimize the need to scroll/jump around
  within source files but it also reflects the indirect Rust principle of locality 
- types before functions - since functions operate on types those should be defined first


### data file name pattern

Many `odin-rs` crates generate files that are stored in the `ODIN_ROOT/data/` or `ODIN_ROOT/cache/` directory trees
and might be shared between different modules/crates. Such filenames should be composed of components that reflect
associated meta information. 

The general rules apply:

- primary use of structured filenames is to support programmatic lookup 
- filename length is of secondary concern
- filenames should be compatible with native filesystems (windows, Linux, macOS)
- filenames should be readable/compatible with external programs
- naming convention should support region as primary and date as secondary lexicographic order
- whitespace characters and punctuation within components are replaced with a single underscore (`_`) 
- components are separated with double underscore (`__`)
- name components can use CamelCase
- only one '.' marking the file format (e.g. `.csv`)

Filename components should follow the following order (note that not all component 
categories might apply)

- area name - if this refers to a [shared item](odin_share/odin_share.md) key name path separators ('/') should
  be replaced with '-' to avoid conflicts with native filesystems
- related date/time in UTC following an abbreviated `YYYY-MM-DD[THHMM[SS]]Z` format (note that date can refer to
  a forecast- or snapshot time and there is no need to encode creation time of the file). Time does not need
  to be present and does not have to include seconds. This follow the [ISO 8601](https://en.wikipedia.org/wiki/ISO_8601)
  specification
- semantic meta information components:
   + forecast step
   + spatial reference system
   + region coordinates
   + data specifiers

Example: `geometry-rect-SantaCruzMtns__2025-06-22T2200Z__2__epsg32610__532097_4087019_620341_4166466__huvw__vec.csv` :

- `geometry-rect-SantaCruzMtns` represents the [shared item](odin_share/odin_share.md) `geometry/rect/SantaCruzMtns`
- `2025-06-22T2200Z` represents the UTC time 06/22/2025 22:00:00
- `2` represents the forecast age ()
- `epsg32610` represents a [UTM](https://en.wikipedia.org/wiki/Universal_Transverse_Mercator_coordinate_system) spatial reference system (zone 10)
- `532097_4087019_620341_4166466` represents region coordinates (rectangle with UTM easting/northing coordinates in this case)
- `huww` represents the dataset composition (height, u,v,w velocities)
- `vec` represents a vector field
- `.csv` means the file format is [comma separated values](https://en.wikipedia.org/wiki/Comma-separated_values)

The preferred way to specify spatial reference systems is [EPSG](https://epsg.io/)