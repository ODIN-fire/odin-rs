# Known Problems

### Safari / VPN / Cesium Ion CORS problem (09/09/2025)
When using Safari while on VPN access to both Bing imagery and Cesium.cesiumWorldTerrainAsync() runs into CORS
rejections by the browser and hence can lead to a blocked JS module initialization (odin_cesium.js awaits a terrainProvider)

#### Workaround:
 
- comment out Bing imagery in ODIN_ROOT/assets/odin_cesium/imglayer_config.js
- set 'fakeTerrain' in ODIN_ROOT/assets/odin_cesium/odin_cesium_config.js
  Note this means height queries will not work and height that is received from the ODIN server might render inconsistently

#### Planned Action:
create own Cesium terrain provider in ODIN