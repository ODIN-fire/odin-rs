export const config = {
    layer: {
      name: "/fire/detection/FEMS",
      description: "FEMS weather stations",
      show: true,
    },
    maxAlertAge: 60000 * 60, // 1h
    color: Cesium.Color.fromCssColorString('yellow'),
    alertColor: Cesium.Color.fromCssColorString('deeppink'),
    labelFont: '16px sans-serif',
    labelBackground: Cesium.Color.fromCssColorString('black'),
    labelOffset: new Cesium.Cartesian2( 8, 0),
    labelDC: new Cesium.DistanceDisplayCondition( 0, 200000),
    pointSize: 5,
    pointOutlineColor: Cesium.Color.fromCssColorString('black'),
    pointOutlineWidth: 1,
    pointDC: new Cesium.DistanceDisplayCondition( 20000, Number.MAX_VALUE),
    infoFont: '14px monospace',
    infoOffset:  new Cesium.Cartesian2( 8, 16),
    infoDC: new Cesium.DistanceDisplayCondition( 0, 18000),
    billboardDC: new Cesium.DistanceDisplayCondition(0, 20000),
    windDC: new Cesium.DistanceDisplayCondition(0, 150000),
    maxHistory: 30, // max data points we keep
    zoomHeight: 18000,
    inactiveDuration: 60000 * 20 // 20 min
};
