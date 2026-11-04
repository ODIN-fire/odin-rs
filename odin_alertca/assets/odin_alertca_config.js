export const config = {
    layer: {
      name: "/fire/detection/AlertCA",
      description: "AlertCalifornia webcams",
      show: false,
    },
    maxAlertAge: 15 * 60, // [secs] cameras that have not been updated for 15min are flagged
    color: Cesium.Color.fromCssColorString('#00ffff'),
    alertColor: Cesium.Color.fromCssColorString('deeppink'),
    labelFont: '15px sans-serif',
    labelBackground: Cesium.Color.fromCssColorString('#00000040'),
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
    fovColor: Cesium.Color.fromCssColorString('#00ffff').withAlpha(0.1),
    fovOutlineColor: Cesium.Color.fromCssColorString('#00ffff'),
    fovDC: new Cesium.DistanceDisplayCondition(0, 50000),
    imageWidth: 630,
    imageHeight: 450,
    maxHistory: 20, // max data points we keep
    zoomHeight: 20000,

    // viewpoint tracking in image windows (bearing line and terrain intersection range)
    viewTrackColor: Cesium.Color.YELLOW,
    viewTrackWidth: 2,
    viewTrackRangeColor: Cesium.Color.ORANGERED,
    viewTrackRangeWidth: 6,
    viewTrackMaxDist: 40000,         // [m] length of bearing line and max distance for terrain intersection
    viewTrackPitchUncertainty: 0.25, // [deg] pitch error for which we show the intersection range
    viewTrackDelay: 300,             // [ms] mouse rest time before we compute terrain intersections
    cameraHeightOffset: 10,          // [m] camera height above the reported camera position (e.g. mast height)
  };
