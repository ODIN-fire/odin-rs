# Introduction

ODIN is a software framework to efficiently create servers that support disaster management. 

<img class="mono right" src="../img/info-fragmentation.svg" width="35%"/>

More specifically it is a framework to build servers that import and process an open number of external data sources for information such as weather, ground-/aerial- and space-based sensors, threat assessment, simulation, vehicle/crew tracking and many more. The over-arching goal is to improve situational awareness of stakeholders by making more - and more timely - information available in stakeholder-specific applications. The main challenge for this is not the availability of data, it is how this data can be integrated in extensible and customizable applications. 

We want to mitigate the **information fragmentation- and compartmentalization problem**. No more hopping between dozens of browser tabs. No more manual refreshes to stay up-to-date. No more printouts to communicate. No more take-it-or-leave-it systems that can't be extended.

ODINs goal is *not* to create yet another website that is supposed to replace all the ones that already exist. We want to enable stakeholder organizations to assemble *their* server applictions showing the information *they* need, with the capability to run those servers/applications on *their* machines (even out in the field if need be). We also want to do this in a way that makes it easy to integrate new data sources and functions as they become available from government, research organizations and commercial vendors. We want ODIN ro be extensible, scalable, portable - and last not least - accessible.

To that end ODIN is open sourced under [Apache v2 license](http://www.apache.org/licenses/LICENSE-2.0). It is a library you can use and extend in your projects. 

## Stakeholders

<img class="mono left" src="../img/stakeholders.svg" width="50%"/>

Our vision for ODIN goes beyond a single stakeholder. We want it to be an open (freely available) platform for both users and developers. The ODIN maintainers are just one part of the puzzle, developing and maintaining the core framework other developers can build on. We only see our role in creating generic components that implement a consistent, extensible and scalable architecture. 

User stakeholders are more than just responder organizations (of which there are many). We also envision local communities who want to improve their level of preparedness / disaster planning. Another example would be utility providers monitoring critical infrastructure. The common theme for such user stakeholders is to enhance their situational awareness but what information that entails depends on the specific stakeholder and location. 

What holds for most user stakeholder organizations is that they lack the resources to develop respective systems from scratch. The stakeholders who do have development capacity often find themselves reinventing the wheel. The stakeholders who subscribe to commercial services have no way to tailor or extend such services.

There is no single organization that could develop all service components on its own. Commercial vendors come up with new sensors. Research organizations develop new forecast models and simulators. The common theme for all such provider stakeholders is that they want to focus on their specific expertise. They don't want to duplicate existing functions just to make their products available. Moreover, if they do it just increases the information fragmentation problem we started with.


## ODIN Application Types

In general there are two main types of ODIN applications:

- user servers
- edge servers

Both are built from the same ODIN components and follow the same architectural design.

### User Servers
support a limited number (<1000) of stakeholder users with the need for collaboration (synchronized views)
and low data latency (tracking, realtime intel). The main application model for user servers is a [Single Page Application](https://en.wikipedia.org/wiki/Single-page_application), the main user interface is a web browser

```
   <SPA diagram>
```

### Edge Servers
provide data for other servers. They are not just brokers/proxies for external resources but can be used to add complex functions and reduce downstream data volume. Assume for instance micro-grid (location/terrain- aware) wind forecast for a given incident area, such as provided by [WindNinja](https://weather.firelab.org/windninja/). This not only requires to run a high computational load (the WindNinja executable itself) but also needs a lot of bandwidth/connectivity to obtain the WindNinja input data (weather forecasts and station reports, high resolution digital elevation models etc.). The user-facing results are relatively small and simple data files containing a wind vector grid in the area of interest. This functionality should run in the cloud on high performance machinery with reliable high speed internet connection. It should not be crammed into a field deployed user server.
Edge servers are the means to make ODIN applications scalable.

```
   <data consolidation diagram>
```

## Examples

To get an idea of what ODIN servers might look like on end user machines we refer to two of our TFRSAC talks:

  * [spring 2023](https://www.youtube.com/watch?v=b9DfMBYCe-s&t=4950s)
  * [fall 2022](https://www.youtube.com/watch?v=gCBXOaybDLA)

