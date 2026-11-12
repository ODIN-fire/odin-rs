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

## Features

None of the features are enabled by default, i.e. a plain `cargo build` creates a server that uses only public data
sources and does not authenticate users. Features can be combined (e.g. `cargo build --features n5,adsb,auth-totp`).

| feature        | category       | description |
|----------------|----------------|-------------|
| `sentinel`     | data source    | [Delphire](https://delphiretech.com/) Sentinel fire sensors ([`odin_sentinel`](../odin_sentinel/odin_sentinel.md)), requires a Delphire access token in `sentinel.ron` |
| `n5`           | data source    | [N5Sensor](https://n5sensors.com/) fire sensors ([`odin_n5`](../odin_n5/odin_n5.md)), requires an N5 API key in `n5.ron` |
| `adsb`         | data source    | ADS-B aircraft tracks from a connected SBS device ([`odin_adsb`](../odin_adsb/odin_adsb.md)), configured in `adsb.ron` |
| `auth-passkey` | authentication | login with passkeys (requires `localhost` or HTTPS with a DNS name) |
| `auth-totp`    | authentication | login with authenticator app codes (works on IP-only LANs) |
| `auth-pw`      | authentication | login with passwords (bootstrapping / fallback) |

The authentication features enable `odin_server/auth` and are described in the [User Authentication](#user-authentication)
section below.

## User Authentication

Per default `odin_live` does not authenticate users (public access). Authentication methods can be enabled with
the `auth-passkey`, `auth-totp` and `auth-pw` features (see [Authentication and Authorization](../odin_server/auth.md)):

```sh
cargo build --features auth-passkey,auth-totp
```

The selected features determine the `methods` of the authentication config, all other settings (roles, `rp_id`,
`session_ttl` etc.) are taken from the `auth` section of `spa_server.ron` if present. Without such a section `odin_live`
uses default settings with the roles `viewer`, `editor` and `admin`. If `odin_live` is built without any `auth-..` feature
an `auth` section in `spa_server.ron` is ignored.

Users are managed with the `auth_admin` tool, e.g. `auth_admin --add-user alice --roles editor --app live`. Since
`odin_live` combines most of the ODIN services it doubles as an integration test for production servers with
authentication.
