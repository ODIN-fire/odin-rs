export const config = {
    layer: {
        name: "/overlay/elmfire",
        description: "Elmfire fire behavior simulator",
        show: true,
    },
    maxEntries: 20,

    //--- fixed wx input defaults
    fixedSpd: [5, 10, 15, 20, 30, 40, 50],
    defaultSpd: 2,
    fixedDir: [0, 45, 90, 135, 180, 225, 270, 315],
    defaultDir: 7,
    dfmMap: new Map([ // dead fuel moisture categories (1,10,100h) in %
        ["low", [2, 5, 5]],
        ["med", [5, 10, 10]],
        ["high", [10, 15, 20]]
    ]),
    defaultDfm: 0,
    lfmMap: new Map([ // live fuel moisture categories (herbaceous, woody) in %
        ["low", [25, 55]],
        ["med", [30, 60]],
        ["high", [35, 65]]
    ]),
    defaultLfm: 1,

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

    zoomHeight: 70000,

    toaRender: {
        stroke: Cesium.Color.YELLOW,
        fill: Cesium.Color.RED.withAlpha(0.25),
        strokeWidth: 1.2
    },
    flinRender: { color: Cesium.Color.RED },
    vsRender: { color: Cesium.Color.MAGENTA },
    cfRender: { color: Cesium.Color.GREENYELLOW },
    flRender: { color: Cesium.Color.DEEPPINK },

};
