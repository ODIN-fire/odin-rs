export const config = {
    layer: {
        name: "/fire/detection/polar satellite",
        description: "active fire detection using polar orbiting satellites",
        show: true,
    },
    history: 3,//${hotspotMaxAge.toHours/24},
    timeSteps: [{ "hours": 6, "color": "#ff0000" },
            { "hours": 12, "color": "#c0000080" },
            { "hours": 24, "color": "#80202080" },
            { "hours": 48, "color": "#80404080" }] ,//${StringUtils.mkString(timeSteps,"[\n    ", ",\n    ", "  ]")(_.toConfigString())},
    bright: 200,// ${brightThreshold.toConfigString()},
    frp: 10,// ${frpThreshold.toConfigString()},
    pixelSize: 3,
    outlineWidth: 1,
    resolution: 0.0,
    swathColor: Cesium.Color.fromCssColorString("#ff000040"),
    trackColor: Cesium.Color.fromCssColorString("#ff0000ff"),
    labelColor: Cesium.Color.fromCssColorString("#ffff00ff"),
    regionColor:Cesium.Color.fromCssColorString("#00ffffff"),
    font: "bold 14px monospace",
    swathDC: new Cesium.DistanceDisplayCondition(  150000, Number.MAX_VALUE)
  };