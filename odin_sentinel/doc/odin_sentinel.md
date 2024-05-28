# odin_sentinel

The `odin_sentinel` crate is a user (data) crate for retrieval, realtime import and processing of [Delphire Sentinel](https://delphiretech.com/products/) fire sensor data. Sentinel sensor devices are smart in-field devices that use on-board AI to detect fire and/or smoke from a mix of
sensor data such as visual/infrared cameras and chemical sensors. The goal is to minimize latency for fire detection *and* in-field
communication related power consumption. 

At this point the `odin_sentinel` crate accesses respective Sentinel data through an external Delphire server, it does not directly communicate with in-field devices. This external server provides two communication channels:

- an http query api to retrieve device capabilities and sensor record data
- a websocket based push notification for new record data availability

The specification of Sentinel data records with respective http access APIs can be found on [Delphire's Documentation Server](http://38.99.249.67:2361/api/). Access of realtime Sentinel data is protected and requires an authentication token from Delphire that can be stored/retrieved in `odin_sentinel` applications via the [`odin_config`] crate.

