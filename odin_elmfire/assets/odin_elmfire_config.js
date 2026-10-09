export const config = {
    layer: {
        name: "/overlay/elmfire",
        description: "Elmfire fire behavior simulator",
        show: true,
    },
    maxEntries: 20,

    //--- fixed wx input defaults
    fixedWs: [5, 10, 15, 20, 30, 40, 50],
    defaultWs: 2,

    fixedWd: [0, 45, 90, 135, 180, 225, 270, 315],
    defaultWd: 7,

    fixedM1: [0, 2, 5, 7, 10, 20],
    defaultM1: 1,

    fixedM10: [0, 5, 10, 15, 20],
    defaultM10: 1,

    fixedM100: [0, 10, 20, 30],
    defaultM100: 1,

    lfmMap: new Map([ // live fuel moisture categories (herbaceous, woody) in %
        ["low", [25, 55]],
        ["med", [30, 60]],
        ["high", [35, 65]]
    ]),
    defaultLfm: 1,

    fixedT2: [0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],  // temperature at 2m
    defaultT2: 7,

    fixedTcc: [0, 30, 70, 100], // pervent total cloud coverage
    defaultTcc: 0,

    pointSize: 4,
    pointColor: Cesium.Color.RED,
    pointOutlineColor: Cesium.Color.YELLOW,
    pointOutlineWidth: 1,
    pointDC: new Cesium.DistanceDisplayCondition( 70000, Number.MAX_VALUE),

    ignPointSize: 4,
    ignPointColor: Cesium.Color.RED,
    ignPointOutlineColor: Cesium.Color.YELLOW,
    ignPointOutlineWidth: 1,
    ignPointDC: new Cesium.DistanceDisplayCondition( 0, 50000),

    labelColor: Cesium.Color.YELLOW,
    labelFont: '14px sans-serif',
    labelBackground: Cesium.Color.fromCssColorString('#00000080'),
    labelOffset: new Cesium.Cartesian2( 8, 0),
    labelDC: new Cesium.DistanceDisplayCondition(0, 70000),

    billboardDC: new Cesium.DistanceDisplayCondition(0, 70000),
    billboardColor: Cesium.Color.YELLOW,

    zoomHeight: 10000,

    imgOutput: [ "flin", "vs", "cf", "fl" ],

    render: {
        toa: {
            stroke: Cesium.Color.YELLOW,
            fill: Cesium.Color.RED.withAlpha(0.25),
            strokeWidth: 1.2,
            dimFactor: 0.7
        },
        flin: { color: Cesium.Color.RED },
        vs: { color: Cesium.Color.MAGENTA },
        cf: { color: Cesium.Color.GREENYELLOW },
        fl: { color: Cesium.Color.DEEPPINK }
    }
};
