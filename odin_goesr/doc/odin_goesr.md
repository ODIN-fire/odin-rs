# odin_goesr

This is a user domain crate that supports download and processing of NASA/NOAAs Geostational Operational Environmental Satellite (GOES)
data. At the time of this writing (05/2024) there are two operational satellites: GOES-16 (aka GOES-east) and GOES-18 (aka GOES-west)
observing North and South America. The primary instrument used is the [Advanced Baseline Imager (ABI)](https://www.ncei.noaa.gov/access/metadata/landing-page/bin/iso?id=gov.noaa.ncdc:C01520). The [Product User Guide (PUG) vol. 5](https://www.goes-r.gov/products/docs/PUG-L2+-vol5.pdf) contains information about
available data products and formats.

ODIN currently supports the L2 FDCC (Fire/Hotspot Characerization) data product, with future plans for additional data sets such as geo-color
images and lightining detection. Details about FDCC data can be found in the PUG (pg. 472pp).

Data is downloaded from the following AWS S3 buckets:

- [GOES-16](https://noaa-goes16.s3.amazonaws.com/index.html)
- [GOES-18](https://noaa-goes18.s3.amazonaws.com/index.html)

which are updated in a 5min interval (data becomes available with +/- 20sec).

The main functions (and general progressions) of this user domain data crate are:

1. timely (minimal latency) data retrieval
2. translation of external data format ([NetCDF](https://www.unidata.ucar.edu/software/netcdf/)) into internal data model
3. async import/notification with import actor
4. web (micro) service for browser based visualization (TBD)