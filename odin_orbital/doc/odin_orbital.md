# odin_orbital

This is a user domain crate that supports orbit calculation, download, and processing of polar orbiting satellite data products, including the Visible Infrared Imaging Radiometer Suite ([VIIRS](https://ladsweb.modaps.eosdis.nasa.gov/missions-and-measurements/viirs/)) hotspot detection instrument aboard the NOAA Joint Polar Satellite System ([JPSS](https://www.nesdis.noaa.gov/our-satellites/currently-flying/joint-polar-satellite-system)). Other currently supported instruments for hotspot detection include the Moderate Resolution Imaging Spectroradiometer [MODIS](https://www.earthdata.nasa.gov/data/instruments/modis) aboard the Terra and Aqua systems. Since the availability of new data is dependent the next overpass calculated from orbital trajectories, the crate provides tools to identify overpasses over a specified region and download data accordingly. 

While the current ODIN support is specialized for hotspot detection, components of `odin_orbital`, including orbit trajectory calculation, are generealized for use with any polar orbiting satellite. The main functions and general steps of this user domain crate are:

1. overpass identification for a large region through orbit propogation
2. async overpass import/notification with orbit actor
3. timely (minimal latency) data retrieval according to overpass notifications
4. translation of external data format (e.g., CSV) into internal data model
5. async data import/notification with import actor
6. web (micro) service for browser based visualization
7. archive replay (TBD)

The structure of the system is shown in the figure below. First an `OrbitActor` is instantiated, which calculates overpasses for a large region using an implementation of the `OrbitCalculator` trait. In this trait, a task is started to regularly query TLEs from an external server, calculate orbital trajectories, and filter the trajectories into discrete overpasses for a specified large region. Once the first calcution is performed, the `OrbitActor` sends the `OrbitSatImporterActor` an `OrbitsReady` message. Note that the `OrbitActor` is inteded to be used to calculate lists of overpasses of a large region, then filter those lists for overpasses of smaller regions of interest defined in the importer(s).

Now that a list of initial overpasses is available, the `OrbitSatImporterActor` activates and implementation of the `OrbitSatImporter` trait and queries an external data server (e.g., FIRMS) for data of interest (e.g., hotspots). The initial overpass list, as well as additional configuration values (e.g., request delay) is used to build a schedule for future data queries. The `OrbitSatImporter` starts a task to request the data according to the future schedule, as well as request new overpasses once the old ones expire. Processed data and overpasses are stored in the actor, then sent to the `WebServerActor` to be displayed in a browser via definitions in the `OrbitalService` and the main actor system definition (see show_jpss)

```
                     ┌──────────────────────────────────────────────────────────────────┐                    
                     │                                                                  │                    
                     │  ┌────────────────────────┐                                      │                    
                     │  │      OrbitActor        │                                      │                    
                     │  │ ┌────────────────────┐ │                                      │                    
                     │  │ │   OverpassList     │ │                                      │                    
                     │  │ └─────────▲──────────┘ │                                      │                    
                     │  │           │            │                                      │                    
┌───────────┐        │  │           │            │                                      │                    
│           ├─┐      │  │┌──────────┼───────────┐│                                      │                    
│ TLE Server│ ◄───http──┼┼─                     ││                                      │                    
│           │ │         ││   OrbitCalculator    ││                                      │                    
│           │ ┼─response┼┼►                     ││                                      │                    
└┬──────────┘ │      │  │└─────┬────────────────┘│                                      │                    
 └────────────┘      │  └──────┼────────────▲──┬─┘                                      │                    
                     │         │            │  │                                        │                    
                     │   Orbits│   Query<AskOverpassRequest,                            │                    
                     │   ready │    UpdateOverpassList>                                 │                    
                     │         │            │  │                                        │                    
                     │  ┌──────▼────────────┴──▼─┐               ┌────────────────────┐ │                    
┌───────────┐        │  │  OrbitSatImportActor   │               │ WebServerActor     │ │                    
│           ├─┐      │  │                        ┼──Overpasses───► ┌────────────────┐ │ │                    
│Data Server│ │      │  │  ┌──────────────────┐  │               │ │                │ │ │                    
│(FIRMS,etc.) ◄── http──┼──┼                  │  ┼───DataSets────► │ OrbitalService ├─┼─┼───────► Web Browser
│           │ │         │  │OrbitalSatImporter┼─┐│               │ │                │ │ │                    
└┬──────────┘ ┼response─┼──┼►                 │ ││               │ │                │ │ │                    
 └────────────┘      │  │  └────────┬─────────┘ ││               │ └────────────────┘ │ │                    
                     │  │           │           ││               │                    │ │                    
                     │  │   ┌───────▼─────────┐ ││               └────────────────────┘ │                    
                     │  │   │   DataStore     │ ││                                      │                    
                     │  │   └─────────────────┘ ││                                      │                    
                     │  │   ┌─────────────────┐ ││                                      │                    
                     │  │   │   OverpassList  ◄─┘│                                      │                    
                     │  │   └─────────────────┘  │                                      │                    
                     │  └────────────────────────┘                                      │                    
                     │                                                                  │                    
                     └──────────────────────────────────────────────────────────────────┘                    
```

## modules

- the main `lib` module contains the common hotspot data model and functions to download and translate respective datasets from NASA [FIRMS](https://firms.modaps.eosdis.nasa.gov/)
- the `overpass` module contains the general (non-application specific) data model and functions for orbit trajectory calculation, including TLE acquisition from [CelesTrak](https://celestrak.org/) or [SpaceTrack](https://www.space-track.org/auth/login), orbit propogation via SGP4, and orbit formatting.
- the `orbital_geo` module holds functions to compute ground track points from orbits
- `live_importer` does the orbit calculation schedule computation, data download schedule computation, and realtime data import from FIRMs. It also contains definitions of respective configuration data. 
- `actor` holds the import actor definition that makes the internal orbit and hotspot data models available in an actor context that provides three
  action points (see [odin_action])
  - init (taking the initial data as action input)
  - update (for each new hotspot and overpass data set)
  - on-demand snapshot (requested per message, taking the whole current hotspot store and overpass list as action input)
- `orbital_service` provides the web-based microservice for browser visualization
- `errors` has the error type definition for the `odin_jpss` crate

## configuration
Several .ron configuration files are needed for building an orbital actor system for a given satellite, including:
- `LiveOrbitalSatConfig`: configuration for a live actor system, includes satellite id, name, data history length, clean up interval, and max age of data stored
- `OrbitalSatOrbitCalculatorConfig`: configuration for orbit calculator, includes the region to calculate overpasses for and the time between calculating overpasses
- `OrbitalSatImporterConfig`: configuration for data importer, including server URL, API map key, region of interest, and request delays for quering URT and NRT data. The free map key has to be obtained from <https://firms.modaps.eosdis.nasa.gov/api/map_key/>
- `OrbitalSatelliteInfo`: configuration for webserver, including satellite name and description for display

## tool executables
- `get_tles` bin - this is a test and production tool to download and read in Two-Line Elements used in orbit propogation
- `get_orbit` bin - this is a test and production tool to calculate and save overpasses from a specified satellite over a given region
- `get_jpss` bin - this is a test and production to download and translate JPSS VIIRS hotspots from FIRMS
- `read_jpss` bin - this is a test and production tool to read and translate an existing CSV file of JPSS VIIRS hotspots into the internal data model
- `show_jpss` bin - this is a test and production tool to implement the end-to-end microservice for the JPSS NOAA-20 satellite including orbit actors, import actors, and a webserver


## example execuables 
- `jpss_actors` example - this shows how to instantiate and connect an [`OrbitActor`] and [`OrbitalSatImportActor`], which calculate overpasses and import data respectively