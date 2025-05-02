# odin_orbital

This is an application domain crate to support data acquisition, processing and display for satellites that revolve around the earth. 
Since those satellites move with respect to terrestrical reference systems ([geodetic](https://en.wikipedia.org/wiki/Geodetic_coordinates) or 
[ECEF](https://en.wikipedia.org/wiki/Earth-centered,_Earth-fixed_coordinate_system)) it is considerably more complex than support for
[geostationary satellites](https://en.wikipedia.org/wiki/Geostationary_orbit) (e.g. [`odin_goesr`](../odin_goesr/odin_goesr.md)). 
Its functions can broken down into:

1. orbit propagation
2. overpass ground-track/-swath computation for a given macro-area (e.g. CONUS)
3. acquisition and post-processing of satellite data products for macro-region overpasses
4. micro-service implementations for such data products (e.g. active fire hotspots)

This chain does involve several external data sources for:

- ephemeris data (input for orbit calculation)
  we currently obtain these through the external [satkit](https://docs.rs/satkit/latest/satkit/) crate from various sources 
  (not requiring authentication but periodic updates)
- orbit parameters (e.g. in form of [TLE](https://en.wikipedia.org/wiki/Two-line_element_set))
  the authorative source is [space-track.org](https://www.space-track.org/) which requires a (free) user account and updates
  for each satellite every 6-12h
- satellite/instrument specific data products (e.g. [VIIRS active fire product](https://www.earthdata.nasa.gov/data/instruments/viirs/viirs-i-band-375-m-active-fire-data)).
  We obtain [VIIRS](https://ladsweb.modaps.eosdis.nasa.gov/missions-and-measurements/viirs/) near realtime hotspot data from
  the excellent [FIRMS](https://firms.modaps.eosdis.nasa.gov/) server (this requires a (free) [map key](https://firms.modaps.eosdis.nasa.gov/usfs/api/area/)). This involves knowing the satellite ground station data processing with respective downlink/availability schedules.

The end goal is to present automatically updated data (e.g. hotspots) for user-selected regions/incident areas, broken down into
past and upcoming overpasses for that area. The user should not be concerned about obtaining and updating the input from above sources.

Although the main orbit type is a low [eccentricity](https://en.wikipedia.org/wiki/Orbital_eccentricity), high 
[inclination](https://en.wikipedia.org/wiki/Orbital_inclination) [Sun Synchonous Orbit (SSO)](https://en.wikipedia.org/wiki/Sun-synchronous_orbit)
(with typical altitude of ~800km and orbital periods around 100 min) the `odin_orbital` crate strives to be generic with respect to supported
orbits to accommodate inclusion of future commercial satellite systems.

To propagate (fly out) orbit trajectories `odin_orbital` uses the 3rd party [satkit](https://docs.rs/satkit/latest/satkit/) crate, which
in turn uses the [SGP4](https://en.wikipedia.org/wiki/Simplified_perturbations_models) perturbation model to calculate trajectory points.
While this model has only about 1km accuracy for up-to-date TLEs this is enough for our purposes, which does not require exact positions at given time points but only [ground tracks](https://oer.pressbooks.pub/lynnanegeorge/chapter/chapter-9-ground-tracks/) with a spatial 
resolution ≪ swath width and a temporal resolution ≪ overpass duration. A typical SSO satellite moves at about 7500m/sec (0.13 sec per 1km), 
which results in overpasses over full CONUS in < 10min. SGP4 is efficient enough to calculate several days of orbits in < 10sec on
commodity hardware.

Apart from orbital parameters we also have to consider the satellite *instrument* in use, which we abstract in terms of a *maximum scan angle*
of the instrument that defines the [*swath*](https://natural-resources.canada.ca/maps-tools-publications/satellite-elevation-air-photos/satellite-characteristics-orbits-swaths) (field-of-vision) of a satellite. This can be anything from a 3000km wide swath for a 
["whisk broom"](https://www.nv5geospatialsoftware.com/Learn/Blogs/Blog-Details/push-broom-and-whisk-broom-sensors) sensor (e.g. 
[JPSS VIIRS](https://ladsweb.modaps.eosdis.nasa.gov/missions-and-measurements/viirs/)) down to a narrow 180km swath for a "push broom" sensor 
(e.g. [Landsat OLI](https://landsat.gsfc.nasa.gov/satellites/landsat-8/spacecraft-instruments/operational-land-imager/)).

