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

    // field of view display (dataView "fov" checkboxes)
    fov: {
      color: Cesium.Color.fromCssColorString('#00ffff').withAlpha(0.1),
      outlineColor: Cesium.Color.fromCssColorString('#00ffff'),
      dc: new Cesium.DistanceDisplayCondition(0, 50000),
    },

    imageWidth: 630,
    imageHeight: 450,
    maxHistory: 20, // max data points we keep
    zoomHeight: 20000,

    // viewpoint tracking in image windows (bearing line and terrain intersection range). Note that camera height above
    // ground and tilt offset are configured per camera in the server side AlertCaConfig
    viewTrack: {
      color: Cesium.Color.CYAN,             // bearing line and labels
      width: 2,
      rangeColor: Cesium.Color.ORANGERED,   // terrain intersection range and hit point
      rangeWidth: 6,
        hitSize: 8,                           // [pixel] terrain intersection point
      hitColor: Cesium.Color.RED,
      hitOutlineColor: Cesium.Color.YELLOW,
      hitOutlineWidth: 2,
      serverSide: true,                     // compute terrain intersections on the server (requires AlertCaConfig.dem),
                                            // otherwise (or if the server has no DEM) in the browser from Cesium terrain
      maxDist: 40000,                       // [m] length of bearing line and max distance for terrain intersection
      pitchUncertainty: 0.25,               // [deg] pitch error for which we show the intersection range
      delay: 300,                           // [ms] mouse rest time before we compute terrain intersections
      angleOffset: new Cesium.Cartesian2( 10, -8), // pixel offset of bearing/pitch label from camera position
    },

    // viewshed display (polygons computed offline by compute_vsh)
    vsh: {
      color: Cesium.Color.fromCssColorString('#ffff00'),
      alpha: 0.25,
    },
  };
