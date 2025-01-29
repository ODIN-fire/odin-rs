// this should be in ODIN_HOME/assets/odin_cesium as it contains the private cesium access token

// this has to be set *before* calling any Cesium functions
Cesium.Ion.defaultAccessToken = null; // replace with your Cesium Ion access token from https://ion.cesium.com/tokens?page=1

export const config = {
    //terrainProvider: Cesium.createWorldTerrainAsync(),
    showTerrain: true,
    requestRenderMode: true,
    targetFrameRate: -1,
    defaultViews: [
      // default views (will be locally shared if not yet set)
      {key: "view/globe/US",               default: { lat: 38.4705, lon: -97.2921, alt: 10370000} },
      {key: "view/region/CONUS West",      default: { lat: 40.98100, lon: -120.38130, alt: 2388500}, home: true },
      {key: "view/state/CA/SF Bay Area",      default: { lat: 38.15910, lon: -122.67800, alt: 779589} },
      {key: "view/state/CA/SF Peninsula",     default: { lat: 37.23020, lon: -122.19930, alt: 58887} },
      {key: "view/state/CA/Big Sur", default: { lat: 36.29400, lon: -121.77800, alt: 90000} },
      {key: "view/state/CA/Los Angeles",   default: { lat: 34.04000, lon: -118.02000, alt: 120000} }
    ],
    localTimeZone: 'America/Los_Angeles',
    color: Cesium.Color.fromCssColorString('red'),
    outlineColor: Cesium.Color.fromCssColorString('yellow'),
    font: '16px sans-serif',
    labelBackground: Cesium.Color.fromCssColorString('#00000060'),
    pointSize: 3,
  };
