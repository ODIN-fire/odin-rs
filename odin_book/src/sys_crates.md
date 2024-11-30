# System Crates

As the name implies ODIN *system crates* provide functionality that is not directly associated to a specific application domain such as weather or even the general topic of disaster management. Most of them can be used for non-ODIN applications.

ODIN system crates can be divided into 3 categories:

## ODIN development

- [odin_build](odin_build/odin_build.md) - this crate is a build- and runtime dependency for other ODIN crates. It
  provides the mechanism to build stand-alone applications that do not rely on separate resource files
- [odin_macro](odin_macro/odin_macro.md) - this is collection of macros that implement domain specific languages
  used especially by `odin_actor` 

## cross-cutting functions

- [odin_common](odin_common/odin_common.md) - this is primarily a collection of cross-cutting functions that extend 
  the Rust standard libraries and provide some basic capabilities such as admin notification
- [odin_gdal](odin_gdal/odin_gdal.md) - a crate that wraps and extends the [GDAL](https://gdal.org) library for geo-spatial
  data sets and images
- [odin_dem](odin_dem/odin_dem.md) - a simple digital elevation model based on [GDAL VRT](https://gdal.org/en/latest/drivers/raster/vrt.html)

## architectural crates

- [odin_actor](odin_actor/odin_actor.md) - this crate implements a full actor system and is the architectural
  basis for most ODIN app crates
- [odin_action](odin_action/odin_action.md) - a crate that provides generic callbacks (used primarily to make
  actors inter-operable)
- [odin_job](odin_job/odin_job.md) - general system-global scheduling
- [odin_server](odin_server/odin_server.md) - this crate provides the building blocks to construct web server actors
- [odin_share](odin_share/odin_share.md) - crate that provides infrastructure to share data between services and users