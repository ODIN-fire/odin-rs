# odin_wx

The `odin_wx` crate is a small system level crate to abstract weather forecast systems. The main purpose is to allow weather clients such as [`odin_wind`](../odin_wind/odin_wind.md) to operate regardless of location and/or with ensembles of different weather forecast systems.

## Background

Weather models require substantial HPC infrastructure and other resources. As a consequence they are developed and operated by government agencies around the world, each continent/region providing their own. Some of these models are global, other only cover specific regions such as the conterminous US. Some of the related services provide direct access of files with model specific formats through ftp or AWS S3, most of them can also be accessed through the [Open-Meteo](https://open-meteo.com/) servers with a unified API.

The challenge is to allow access to weather data from clients (such as `odin_wind`) that have to operate world wide, i.e. hide the concrete (configured) weather model access behind an abstract interface.

## Implementation

`odin_wx` provides the following constructs:

- `WxService` - a trait that is the abstract interface object used in clients
- `WxDataSetRequest` - a struct that contains all the information for a weather forecast query
- `WxFileAvailable` - a struct that is used by the respective service to announce availability of files containing results for the (included) query
- `AddDataSet` and `RemoveDataSet` messages to subscribe to/unsubscribe from a weather service, using a reference to the `WxDataSetRequest` to specify the region of interest

The `WxService` trait and its implementations have to be object safe so that clients can store references without having to specify the concrete implementation type.

The outbound message interface from client actors are `AddDataSet` and `RemoveDataSet`, the inbound (async response) message is `WxFileAvailable`. To prevent unneccessary duplication and memory allocation the invariant request objects are wrapped as `Arc<WxDataSetRequest>`, which also allows efficient matching (`WxDataSetRequest` implements `std::hash::Hash` which allows to use them as `HashMap` or `HashSet` keys).
