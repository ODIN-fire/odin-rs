# odin_live

`odin_live` is a bin (application) crate that demonstrates the use of ODIN as a web server in the conterminous US (CONUS),
including commercial data sources that require application keys.

Per default `odin_live` builds and runs with public data sources. Additional (proprietary) data sources / services
have to be enabled with features, i.e. to build the full system you have to use the following command:

```sh
cargo build --features n5,sentinel,adsb
```

More data sources will be added to `odin_live` as they become available. The currently supported ones are:

- basic [`odin_cesium`](../odin_cesium/odin_cesium.md) services (clock, view and imagery)
- [`odin_share`](../odin_share/odin_share.md) service (both for shared data and messaging)
- [`odin_geolayer`](../odin_geolayer/odin_geolayer.md) to show configured GeoJSON data sets
- [`odin_fires`](../odin_fires/odin_fires.md) historical operational fire data
- [`odin_goesr](../odin_goesr/odin_goesr.md) geostationary satellite hotspots
- [`odin_orbital](../odin_orbital/odin_orbital.md) various orbiting satellites
- [`odin_wind`](../odin_wind/odin_wind.md) micro grid wind simulation and display with WindNinja
- [`odin_alertca`](../odin_alertca/odin_alertca.md) [AlertCalifornia](https://alertcalifornia.org/) web cameras
- [`odin_n5`](../odin_n5/odin_n5.md) [N5Sensor](https://n5sensors.com/) fire sensors (requires to build with `n5` feature)
- [`odin_sentinel`](../odin_sentinel/odin_sentinel.md) [Delphire](https://delphiretech.com/) Sentinel fire sensors (requires to build with `sentinel` feature)
- [`odin_adsb`](../odin_adsb/odin_adsb.md) ADS-B import from connected SBS device (requires `adsb` feature)
