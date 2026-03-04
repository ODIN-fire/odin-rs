export const config = {
    layer: {
        name: "/overlay/bushfires",
        description: "near realtime bushfire data",
        show: true,
    },
    maxEntries: 20,

    pointSize: 4,
    pointColor: Cesium.Color.MAGENTA,
    pointOutlineColor: Cesium.Color.WHITE,
    pointOutlineWidth: 1,
    pointDC: new Cesium.DistanceDisplayCondition( 70000, Number.MAX_VALUE),

    labelColor: Cesium.Color.YELLOW,
    labelFont: '14px sans-serif',
    labelBackground: Cesium.Color.fromCssColorString('#00000060'),
    labelOffset: new Cesium.Cartesian2( 8, 0),
    labelDC: new Cesium.DistanceDisplayCondition(0, 70000),

    billboardDC: new Cesium.DistanceDisplayCondition(0, 70000),
    billboardColor: Cesium.Color.YELLOW,

    zoomHeight: 70000,

    perimeterRender: {
        strokeColor: Cesium.Color.YELLOW,
        strokeWidth: 1.2,
        fillColor: Cesium.Color.fromCssColorString("#f00000"),
        fillOpacity: 0.5,
        dimFactor: 0.9,
    },

};
