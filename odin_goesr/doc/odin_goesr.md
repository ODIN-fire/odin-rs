# odin_goesr

This is a user domain crate that supports download and processing of NASA/NOAAs Geostational Operational Environmental Satellite (GOES)
data. At the time of this writing (05/2024) there are two operational satellites: GOES-16 (aka GOES-east) and GOES-18 (aka GOES-west)
observing North and South America. The primary instrument used is the [Advanced Baseline Imager (ABI)](https://www.ncei.noaa.gov/access/metadata/landing-page/bin/iso?id=gov.noaa.ncdc:C01520). The [Product User Guide (PUG) vol. 5](https://www.goes-r.gov/products/docs/PUG-L2+-vol5.pdf) contains information about
available data products and formats.

ODIN currently supports the L2 FDCC (Fire/Hotspot Characerization) data product, with future plans for additional data sets such as geo-color
images and lightining detection. Details about FDCC data can be found in the PUG (pg. 472pp), other available data products are listed
[here](https://github.com/awslabs/open-data-docs/blob/main/docs/noaa/noaa-goes16/README.md) (note that GOES-16 (East) is now replaced by GOES-19
and GOES-18 (West) supports the same data products).

Data is downloaded from the following AWS S3 buckets:

- [GOES-19](https://noaa-goes19.s3.amazonaws.com/index.html)
- [GOES-18](https://noaa-goes18.s3.amazonaws.com/index.html)

which are updated in a 5min interval (data becomes available with +/- 20sec).

The main functions (and general progressions) of this user domain data crate are:

1. timely (minimal latency) data retrieval
2. translation of external data format ([NetCDF](https://www.unidata.ucar.edu/software/netcdf/)) into internal data model
3. async import/notification with import actor
4. web (micro) service for browser based visualization
5. archive replay (TBD)


## modules

- the main `lib` module contains the common data model and general (non-application specific) functions to download and translate
  respective AWS data sets
- the `geo` module holds functions to compute geodetic coordinates from GOES-R scan angles
- `live_importer` does the download schedule computation and realtime data import from AWS S3. It also contains definition of
  respective configuration data
- `actor` holds the import actor definition that makes the internal data model available in an actor context that provides three
  action points (see [odin_action])
  - init (taking the initial data as action input)
  - update (for each new data set)
  - on-demand snapshot (requested per message, taking the whole current data as action input)
- `errors` has the error type definition for the `odin_goesr` crate

## tool executables

- `download_goesr_data` bin - this is both for testing the download schedule and for retrieving raw data during production
- `read_goesr_hotspots` bin - this is a test and production tool to translate single downloaded files into the internal data format

## example executables

- `goesr_actor` example - this shows how to instantiate and connect a [`GoesRHotspotImportActor`]
