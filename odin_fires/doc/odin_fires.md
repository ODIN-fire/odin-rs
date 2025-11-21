# odin_fires

The `odin_fires` application domain crate provides a data model and `SpaService` to display operational fire data
such as name, incident identifiers, location, perimeters and more. 

This was started as an attempt to provide visualization for the "Fire Progression" database from <https://data-nifc.opendata.arcgis.com> 
(which is succeeded by the "Historical Operational Data" database). This data was not directly displayable as it contains 
[GeoJSON](https://geojson.org/) `FeatureCollections` with all perimeter `Polygons` in one file. Our intention is to extend the
functionality to a full data model for wildfire incidents that can be dynamically updated, i.e. that is applicable to both historical
and ongoing incidents. This model should

* be extensible
* be backed by a hierarchical file system in which each incident has its own subdirectory
* use standard external file formats such as JSON and GeoJSON
* use a summary (JSON) file in each incident directory that contains links to all available incident data such as perimeters

Since incident data is mainly produced either manually or by external systems and applications `odin_fires` does not have a classic
import actor and operates solely on its file system in `ODIN_ROOT/data/odin_fires`. This file system is currently analyzed during
initialization of the `FireService` but eventually will be monitored continuously for changes that are to be distributed as summary
updates to connected clients.

Before such content is sent to clients `odin_fires` has to make sure the data conforms to our internal data model, i.e. all data
has to be parsed into our model before sending it out so that our `odin_fires.js` Javascript module will be able to process the data.

Consequently the main construct of the `odin_fires` crate is the `FireSummary` data model in its `lib.rs`. The crate also provides
a `FireService` that is a simple `odin_server::spa::SpaService` implementation.

The crate also contains tools and generators for related incident files such as the `opendata_splitter` command line executable.