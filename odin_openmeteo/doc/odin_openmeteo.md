# odin_openmeteo

The `odin_openmeteo` crate is an application domain crate to download and publish weather forecast data
obtained from [Open-Meteo](https://open-meteo.com/). This site provides a generic API to access various
regional and global weather model forecasts such as

- NOAA HRRR (CONUS)
- GFS (global)
- ECMWF (global, both ifs and aifs)
- ICON (global and Germany)
- GEM (Canada)

and more.

The main `odin_openmeteo` components are the `OpenMeteoActor` and the `OpenMeteoService`.

`OpenMeteoActor` is the [`odin_actor`](../odin_actor/odin_actor.md) based actor implementation that 
performs the actual downloads from [https://open-meteo.com](https://open-meteo.com) according to its
[API documentation](https://open-meteo.com/en/docs).

As its counterpart in [odin_hrrr](../odin_hrrr/odin_hrrr.md) the actor implements a subscription model based
on [`odin_wx::WxDataSetRequest`](../odin_wx/odin_wx.md) objects sent from client actors via `odin_wx::AddDataSet`
and `odin_wx::RemoveDataSet` messages. Different to `odin_hrrr` the update schedule is not computed or configured
in ODIN but retrieved from Open-Meteo through its 
[`https://api.open-meteo.com/data/<model-name>/meta.json`](https://open-meteo.com/en/docs/model-updates) endpoint.
The response data is a JSON object that contains all the Unix epoch timestamps to determine if a new dataset
is available and when to expect the next one:

```json
{
  "chunk_time_length": 504,
  "crs_wkt": "GEOGCRS[\"Reduced Gaussian Grid\" ... BBOX[-90,-180.0,90,180]]]",
  "data_end_time": 1774443600,
  "last_run_availability_time": 1773167124,
  "last_run_initialisation_time": 1773144000,
  "last_run_modification_time": 1773166529,
  "temporal_resolution_seconds": 3600,
  "update_interval_seconds": 21600
}
```

Since the schedule for a given dataset is infrequent (1h/3h/6h, depending on model) we use the common [`odin_job`](../odin_job/odin_job.md) mechanism to inform the actor of when to check for updates. 

Once a new dataset is retrieved it is published through a [`odin_action`](../odin_action/odin_action.md) `DataAction<WxFileAvailable>`.
The action argument is a [`odin_wx::WxFileAvailable`](../odin_wx/odin_wx.md) object that encapsulates the respective 
`odin_wx::WxDataSetRequest` and the filename of the downloaded weather forecast data.

`odin_openmeteo::OpenMeteoService` is the `odin_wx::WxService` implementation that abstracts concrete weather service
actors in clients (such as the [`odin_wind::WindActor`](../odin_wind/odin_wind.md)).

The Open-Meteo data model differs from direct weather model specific downloads (such as done by `odin_hrrr`). While Open-Meteo
supports bounding boxes for query regions it really is a point set query, i.e. it returns forecast data as a set of discrete 
location objects that include timestep arrays for each requested variable. Depending on the requested model these locations might
not be on an equidistant grid - ECMWF for instance is based on a 
[Gaussian grid](https://www.ecmwf.int/sites/default/files/elibrary/2016/17262-new-grid-ifs.pdf) that does have to be re-gridded
by means of [`odin_gdal::grid::create_grid_ds(..)`](../odin_gdal/odin_gdal.md).

While `odin_openmeteo` does not automatically perform such conversions it has a `odin_openmeteo::convert` module that provides
convenience functions which can be used by clients to post-process the JSON data obtained from Open-Meteo.
