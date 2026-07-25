export const config = {
    layer: {
        name: "/overlay/elmfire",
        description: "Elmfire fire behavior simulator",
        show: true,
    },
    maxEntries: 20,

    pointSize: 4,
    pointColor: Cesium.Color.RED,
    pointOutlineColor: Cesium.Color.YELLOW,
    pointOutlineWidth: 1,
    pointDC: new Cesium.DistanceDisplayCondition( 70000, Number.MAX_VALUE),

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
