export const config = {
    layer: {
      name: "/fire/detection/AlertCA",
      description: "AlertCalifornia webcams",
      show: false,
    },
    maxAlertAge: 60000 * 60, // 1h
    color: Cesium.Color.fromCssColorString('aquamarine'),
    alertColor: Cesium.Color.fromCssColorString('deeppink'),
    labelFont: '15px sans-serif',
    labelBackground: Cesium.Color.fromCssColorString('black'),
    labelOffset: new Cesium.Cartesian2( 10, 0),  // not handled correctly in Firefox
    labelDC: new Cesium.DistanceDisplayCondition( 0, 80000),
    pointSize: 5,
    pointOutlineColor: Cesium.Color.fromCssColorString('black'),
    pointOutlineWidth: 1,
    pointDC: new Cesium.DistanceDisplayCondition( 30000, Number.MAX_VALUE),
    infoFont: '14px monospace',
    infoOffset:  new Cesium.Cartesian2( 8, 16),
    infoDC: new Cesium.DistanceDisplayCondition( 0, 8000),
    billboardDC: new Cesium.DistanceDisplayCondition( 0, 30000),
    fovColor: Cesium.Color.fromCssColorString('aquamarine').withAlpha(0.1),
    fovOutlineColor: Cesium.Color.fromCssColorString('aquamarine'),
    fovDC: new Cesium.DistanceDisplayCondition(0, 50000),
    imageWidth: 630,
    imageHeight: 450,
    maxHistory: 20, // max data points we keep
    zoomHeight: 20000,
    inactiveDuration: 60000 * 20 // 20 min
  };