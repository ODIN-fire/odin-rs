// this should be in ODIN_HOME/assets/odin_cesium as it contains the cesium access token

export const config = {
    accessToken: 'your-cesium-access-token', // REPLACE WITH YOUR TOKEN
    terrainProvider: Cesium.createWorldTerrainAsync(),
    requestRenderMode: true,
    targetFrameRate: -1,
    cameraPositions: [
      {name: "Bay Area", lat: 38.15910, lon: -122.67800, alt: 779589},
      {name: "Peninsula", lat: 37.23020, lon: -122.19930, alt: 58887},
      {name: "Big Sur North", lat: 36.29400, lon: -121.77800, alt: 90000},
      {name: "Los Angeles", lat: 34.04000, lon: -118.02000, alt: 120000},
      {name: "conus west", lat: 40.98100, lon: -120.38130, alt: 2388500},
      {name: "space", lat: 37.32540, lon: -127.71080, alt: 11229395}
    ],
    localTimeZone: 'America/Los_Angeles',
    color: Cesium.Color.fromCssColorString('red'),
    outlineColor: Cesium.Color.fromCssColorString('yellow'),
    font: '16px sans-serif',
    labelBackground: Cesium.Color.fromCssColorString('#00000060'),
    pointSize: 3,
  };