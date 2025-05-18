export const config = {
    layer: {
        name: "/fire/detection/polar satellite",
        description: "active fire detection using polar orbiting satellites",
        show: true,
    },
    maxAgeInDays: 10, // maximum age (in days) after which we drop completed overpasses
    timeSteps: [
        { "hours":  6, "color": Cesium.Color.fromCssColorString("#ff0000") },
        { "hours": 12, "color": Cesium.Color.fromCssColorString("#c0000080") },
        { "hours": 24, "color": Cesium.Color.fromCssColorString("#80202080") },
        { "hours": 48, "color": Cesium.Color.fromCssColorString("#80404080") }
    ],
    bright: {
        value: 310, 
        color: Cesium.Color.fromCssColorString('#ffff00')
    },
    frp: {
        value: 10, 
        color: Cesium.Color.fromCssColorString('#000000')
    },
    pixelSize: 4,
    outlineWidth: 1,
    resolution: 0.0,
    swathColor: Cesium.Color.fromCssColorString("#ff000040"),
    trackColor: Cesium.Color.fromCssColorString("#ff0000ff"),
    labelColor: Cesium.Color.fromCssColorString("#ffff00ff"),
    regionColor:Cesium.Color.fromCssColorString("#00ffffff"),
    font: "bold 14px monospace",
    swathDC: new Cesium.DistanceDisplayCondition(  150000, Number.MAX_VALUE),
    smallFootprintLength: 100, // minimal footprint width/height (in meters) for which we always display points  
    smallFootprintDist: 15000, // below this we don't show points for small footprints
    footprintDist: 100000 // above this we only show points, no areas
  };