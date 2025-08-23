export const config = {
    layer: {
        name: "/tracking/ads-b",
        description: "ADS-B aircraft tracking",
        show: false,
    },

    colors: new Map([                  // source->color map
        ['KNUQ',Cesium.Color.CYAN],
        [undefined, Cesium.Color.YELLOW] // the default
    ]), 
  
    models: new Map([ // track.type -> model URI
        [undefined, './asset/odin_adsb/generic-airplane.glb'] // the default
    ]),

    labelFont: '16px sans-serif',
    labelOffset: new Cesium.Cartesian2( 12, 10),
    //labelBackground: Cesium.Color.fromCssColorString('black'),
    labelDC: new Cesium.DistanceDisplayCondition( 0, 200000),

    pointSize: 5,
    pointOutlineColor: Cesium.Color.fromCssColorString('black'),
    pointOutlineWidth: 1,
    pointDC: new Cesium.DistanceDisplayCondition( 120000, Number.MAX_VALUE),

  // TODO - configure models/colors (per channel?)

    modelSize: 20,
    modelDC: new Cesium.DistanceDisplayCondition( 0, 120000),
    modelOutlineColor: 'black',
    modelOutlineWidth: '2.0',
    modelOutlineAlpha: '1.0',

    infoFont: '14px monospace',
    infoOffset:  new Cesium.Cartesian2( 12, 26),
    infoDC: new Cesium.DistanceDisplayCondition( 0, 80000),

    pathLength: 0,
    pathDC: new Cesium.DistanceDisplayCondition( 0, 1000000),
    pathColor: Cesium.Color.fromCssColorString('yellow'),
    pathWidth: 1,
    path2dWidth: 3,

    maxTraceLength: 200,
};