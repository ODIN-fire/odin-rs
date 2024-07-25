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
  the Rust standard libraries  
- [odin_gdal](odin_gdal/odin_gdal.md)

## architectural crates

- [odin_actor](odin_actor/odin_actor.md)
- [odin_action](odin_action/odin_action.md)
- [odin_job](odin_job/odin_job.md)
- [odin_server](odin_server/odin_server.md)