# odin_sentinel

The `odin_sentinel` crate is a user (data) crate for retrieval, realtime import and processing of [Delphire
Sentinel](https://delphiretech.com/products/) fire sensor data. Sentinel sensor devices are smart in-field devices that
use on-board AI to detect fire and/or smoke from a mix of sensor data such as visual/infrared cameras and chemical
sensors. The goal is to minimize latency for fire detection *and* in-field communication related power consumption. 

The `odin_sentinel` crate accesses respective Sentinel data through an external Delphire server, it does not directly communicate with in-field devices. This external server provides two communication channels:

- an http query api to retrieve device capabilities and sensor record data
- a websocket based push notification for new record data availability

The primary component running in the ODIN server is a `SentinelActor`, which is a realtime database of a configurable
number of most recent sentinel sensor records. The `SentinelActor` does not directly communicate with the outside world - it
uses a `SentinelConnector` trait object to do external IO. The primary impl for this trait is the `LiveSentinelConnector`,
which retrieves the list of available Sentinel devices, queries each devices sensors and retrieves initial sensor records
(all using HTTP GET requests). It then opens a websocket and listens for incoming sensor record availability notifications
(as JSON messages). Once such a notification was received the connector uses HTTP GET to retrieve the record itself and
informs client actors about the update. 

```
                                                                                        [sentinel_alarm.ron] config
                                                               ┌─────────────────────────────────────╎─────┐                  
                                                               │ ODIN Server           ┌─────────────╎───┐ │                  
                                                               │                       │ AlarmActor  ╎   │ │                  
                                                 - devices     │                ┌─────▶︎│  ┌──────────▼┐  │ │                  
                                                 - sensors     │                │      │  │Alarm      ├─┐│ │                  
                            ┌─────────────────┐  - sensor-     │ ┌──────────────┴──┐   │  │ Messenger │ │────────▶︎ phone      
┌────────┐                  │ Delphire Server │     records    │ │ SentinelActor   │   │  └┬──────────┘ ││ │                
|        ├─┐                │                 │                │ │ ┌─────────────┐ │   │   └────────────┘│ │                  
│Sentinel│ │─ satellite ───▶︎│  ┌───────────┐  ├───── http ────────▶︎│             │ │   └─────────────────┘ │                  
│        │ │      or        │  │  record   │  │                │ │ │LiveConnector│ │                       │                  
└┬───────┘ │── cellular ───▶︎│  │ database  │  ├─── websocket ─────▶︎│             │ │   ┌─────────────────┐ │                  
 └─────────┘                │  └───────────┘  │                │ │ └─▲───────────┘ │   │ WebServerActor  │ │                  
                            │                 │ - notification │ └───╎──────────┬──┘   │ ┌────────────┐  │ │                  
                            └─────────────────┘                │     ╎          │      │ │  Sentinel  ├─┐│ │                  
                                                               │     ╎          └─────▶︎│ │   Service  │ │────────▶︎ web        
                                                               │     ╎                 │ └┬──────▲────┘ ││ │     browser      
                                            [sentinel.ron]╶╶╶╶╶╶╶╶╶╶╶┘                 │  └──────╎──────┘│ │                  
                                              config           │                       └─────────╎───────┘ │                  
                                                               └─────────────────────────────────╎─────────┘ 
                                                                                          [odin_sentinel.json] asset                 
```

Although `SentinelActor` can be connected to any client actor using `odin_action` for message interactions the `odin_sentinel` crate
includes two primary clients / client-components: `SentinelAlarmActor` and `SentinelSpaService`.

The `SentinelAlarmActor` is used to listen for incoming updates about fire and smoke alarms that have not been reported yet, retrieves respective
evidence (images), and then uses a configurabe set of `AlarmMessenger` trait objects to report new alarms to the outside world. The primary
choice for production messengers is the `SlackAlarmMessenger` that pushes notifications to configurable slack channels.

The `SentinelSpaService` implements a `odin_server::SpaService` to add a sentinel channel to a single page web application.

The specification of Sentinel data records with respective http access APIs can be found on [Delphire's Documentation Server](http://38.99.249.67:2361/api/). Access of realtime Sentinel data is protected and requires an authentication token from Delphire that can be stored/retrieved in `odin_sentinel` applications via the [`odin_config`] crate.
