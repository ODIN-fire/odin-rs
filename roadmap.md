# ODIN Development Roadmap

This file contains a list of tentatively planned future `odin-rs` components and work items. The order might
change.

workaround for Safari JS module initialization problem (for dependencies on async modules - which is mostly 
odin_cesium.js). As of Safari 18.6.1 

ODIN server side fire detection from images (e.g. in combination with odin_alertca and odin_sentinel). This
is mainly intended to reduce the number of false positives from field based sensors and ultimately should
support continuous model refinement per device.

Port of the RACE historical fire perimeter layer.

User authentication for odin-rs SpaServer documents. This can make use of the Rust tower-http middleware.

Import and visualization of the GOES-R Geostationary Lightning Mapper (GLM), possibly combined with 
ground precipitation to detect potential dry lightining storms (https://www.goes-r.gov/spacesegment/glm.html)

Sentinel-2 data import to detect and map fires.

Port of the RACE ReplayActor functionality, including the RACE 
[Tagged Archive format](https://nasarace.github.io/race/design/archive-replay.html). This will not be
a direct port as ODIN uses import actor internal "connector tasks" to abstract the data source. This
also needs to verify consistent use of a simulation time (there are a number of ODIN components that
use wall clock although the basic sim clock infrastructure is there).
