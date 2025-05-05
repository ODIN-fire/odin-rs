# odin_orbital

## Introduction

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


## Main Constructs and Algorithms

The primary functions provided by `odin_orbital`  can be partitioned into 

1. overpass-computation
2. payload data acquisition 

Apart from that (1) does provide input for (2) in form of overpass times both steps are independent. The `odin_orbital` crate
tries to be generic in terms of payload data types. Active fire products providing "hotspots" are only one example of such payload data.

Since they are normally used to determine when to obtain payload data the types associated with overpasses do not typically show in 
applications. The main underlying data type is `Overpass` which captures the ground track and start-/end-times of a satellite trajectory
over a configured macro-region (e.g. CONUS). Instances are computed by an `OverpassCalculator`, which in turn uses a `TleStore` implementation
(of which the main impl currently is the `SpaceTrackTleStore` retrieving [TLEs](https://en.wikipedia.org/wiki/Two-line_element_set) from [space-track.org](https://www.space-track.org/)) to obtain basic orbital parameters for a given satellite.

The `OverpassCalculator` first obtains TLEs based on configured `OrbitalSatelliteInfos`, computes 

- `OrbitInfos` with actual orbit data such as orbital period, perigee/apogee times, average height and 
  [orbital nodes](https://en.wikipedia.org/wiki/Orbital_node) for those TLEs, and
- `OverpassConstraints` derived from the configured macro-region and `OrbitalSatelliteInfos`
  
and finally uses `TLEs`,`OverpassConstraints` and `OrbitInfos` to compute the `Overpass` objects by propagating orbits with the external 
[`satkit`](https://docs.rs/satkit/latest/satkit/) crate by means of its [SGP4](https://en.wikipedia.org/wiki/Simplified_perturbations_models) implementation. Both `OrbitInfo` and `OverpassConstraints` are internal objects that are only used for efficient overpass computation.

<img src="../img/macro-region-alg.svg" class="mono right" width="30%">

The basic algorithm to detect relevant overpasses is to check if the ground point or any of the swath end points of a trajectory time step 
are within the open polyhedron that is formed by the earth center and the planes that are defined by the macro-region vertices (which therefore
have to form a concave spherical polygon). This is an efficient operation using cartesian coordinates (ECEF) and precalculated polyhedron normal
vectors, which is crucial for being able to obtain overpasses over large areas (such as CONUS) and several days. Should the region of interest be small with respect to the average swath width then additional test points along the (ground track orthogonal) scan line can be added to prevent
that we miss overpasses due to small regions being entirely within one side of the swath.

Once we have the observation times of the computed `Overpass` objects we can obtain and post-process the payload data we are ultimately
interested in by retrieving respective data products for the given satellite/instrument combinations (e.g. NOAA-21/VIIRS). This step is
independent of orbit/overpass calculation - it merely uses the computed overpass end times and knowledge about the satellite specific 
downlink/data processing to schedule retrieval of such data products and then translates the raw data into our internal formats 
(e.g. `Hotspot`).

The first class of payload data that is supported by `odin_orbital` are active fire products with so called "hotspots" - geographic
areas for which data post processing yields a s significant risk of active fires. The size of such hotspot "footprints" varies depending
on satellite height, instrument and distance from ground track. For the JPSS/[VIIRS](https://ladsweb.modaps.eosdis.nasa.gov/missions-and-measurements/viirs/) combination the footprint is a rectangle with ~400m side length. For Landsat/OLI the spatial resolution
is about 30m (due to a much more narrow swath). For larger footprints such as VIIRS it is also of interest to show the orientation of
the rectangle as an indicator of fire fronts (represented by consecutive hotspots along and between scan lines). 

Footprint orientation can be calculated based on the ground track trajectories stored in `Overpass` objects by first computing the
nearest ground track point for a given hotspot, and then using the [law of haversines](https://en.wikipedia.org/wiki/Haversine_formula) to compute the bearing from the hotspot to the ground track point.

The main data type for active fire detection is `Hotspot`, which in addition to the geographic position also stores quantifications
such as *brightness*, *fire radiative power* (representing rate of outgoing thermal radiative energy) and the footprint area mentioned above.

The main types that are used in applications - and tie together all the objects listed above - are

- `OrbitalHotspotActor` - the [actor](../odin_actor/odin_actor.md) type producing overpass and hotspot data
- `OrbitalHotspotService` - the `SpaService` that uses `OrbitalHotspotActor` instances to push their data to clients through a
  [`SpaServer`](../odin_server/odin_server.md)

The `OrbitalHotspotActor` provides collections of `Overpass` and `HotspotList` objects. It is connected to other actors such
as the [`odin_server::SpaServer`](../odin_server/odin_server.md) through three [action slots](../../odin_action/odin_action.md):

- init action - to announce initial availability of (past) overpasses and hotspot data sets
- overpass action - to distribute new (upcoming) overpasses
- hotspot action - to distribute new (past) hotspot data sets

In addition to these outgoing connection points the `OrbitalHotspotActor` also processes incoming `ExecSnapshotAction` messages
by executing their [DynDataRefAction](../odin_action/odin_action.md) with (immutable) references to computed overpasses and hotspots.

```
                       ┌───────────────────────────────────────────────────────┐
  ./configs/           │               OrbitalHotspotActor<T,I>                │
    noaa-21_viirs.ron ─┼► sat_info                                             │
            conus.ron ─┼► region                                               │
                       │                                                       │
                       │ ┌───────────────────────┐                             │
                       │ │OverpassCalculator     │                             │
                       │ │                       │                             │
 ODIN_ROOT/config/...  │ │   T:TleStore          ┼──── Overpass ──┐            │
       spacetrack.ron ─┼─┼─► SpaceTrackTleStore  │                │            │
                       │ │                       │                │            │
                       │ │   OverpassConstraints │       ┌────────▼─────────┐  │
                       │ │                       │       │HotspotActorData  │  │
                       │ │   OrbitInfo[]         │       │                  │  │
                       │ └───────────┬───────────┘       │  upcoming[]      │  │
                       │             │overpass-end       │  completed[]     │  │
                       │             │                   └────────▲──────┬──┘  │
                       │ ┌───────────▼───────────┐                │      │     │
ODIN_ROOT/configs/...  │ │I:HotspotImporter      │                │      │     │
            firms.ron ─┼─► ViirsHotspotImporter  ┼──── Hotspot[] ─┘      │     │
                       │ └───────────────────────┘                       │     │
                       │                                                 │     │
                       │        ┌─────────────────┬──────────────────────┤     │
                       │  ┌─────▼─────┐   ┌───────▼───────┐   ┌──────────▼───┐ │
                       │  │init_action│   │overpass_action│   │hotspot_action│ │
                       │  └───────────┘   └───────────────┘   └──────────────┘ │
                       └───────────────────────────────────────────────────────┘
```

This actor is autonomous in that it knows when to compute new overpasses and - for completed overpasses - to retrieve payload data so
that it can compute respective hotspot sets.

There is one `OrbitalHotspotActor` instance for each satellite.

The `OrbitalHotspotService` is a fairly common `odin_server::SpaService` micro-service that links a number of `OrbitalHotspotActors` to a
single [`SpaServer`](../odin_server/odin_server.md), which then serves both the overpass- and hotspot- data (as JSON) plus the associated [JS module](../odin_server/client.md) assets to process and display this data.

Since hotspot data size depends on the fire activity it can get large. Consequently, both overpasses and hotspots are directly stored
as files in the `ODIN_ROOT/cache/odin_orbital/` directory (see [odin_build](../odin_build/odin_build.md)) and only annonced on the websocket.
It is up to the JS module to fetch these data files when the user wants to display available data. Apart from such on-demand retrieval
(using JS [`Promises`](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Promise)) the JS module
also supports interactive selection/entry of incident areas (as geographic rectangles) which are then used to filter overpasses
that cover them. This is done (on the client) by computing overpasses for which at least one area vertex is within the swath (has a 
distance to the closest groung track point < swath-width/2).