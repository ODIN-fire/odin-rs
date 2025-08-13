export const config = {
    layer: {
      name: "/fire/detection/N5Shield",
      description: "stationary N5Shield fire sensors",
      show: true,
    },
    color: Cesium.Color.fromCssColorString('chartreuse'),
    alertColor: Cesium.Color.fromCssColorString('deeppink'),
    labelFont: '16px sans-serif',
    labelBackground: Cesium.Color.fromCssColorString('black'),
    labelOffset: new Cesium.Cartesian2( 8, 0),
    labelDC: new Cesium.DistanceDisplayCondition( 0, 200000),
    pointSize: 5,
    pointOutlineColor: Cesium.Color.fromCssColorString('black'),
    pointOutlineWidth: 1,
    pointDC: new Cesium.DistanceDisplayCondition( 5000, Number.MAX_VALUE),
    infoFont: '14px monospace',
    infoOffset:  new Cesium.Cartesian2( 8, 16),
    infoDC: new Cesium.DistanceDisplayCondition( 0, 10000),
    billboardDC: new Cesium.DistanceDisplayCondition( 0, 5000),
    maxHistory: 10,
    zoomHeight: 20000,
    inactiveMinutes: 60
  };