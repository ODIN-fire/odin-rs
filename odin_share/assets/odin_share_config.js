// example odin_share.js configuration asset

export const config = {
    layer: {
        name: "/system/sharedItems",
        description: "local/global shared items and sync",
        show: true,
    },

    maxMessages: 50,

    render: { // rendering options
        color: Cesium.Color.AQUA,
        labelPath: false, // show shared var path in label (otherwise only name is shown)
        labelStats: true, // show label stats (area, distance etc.)
        labelFont: '16px sans-serif',
        labelBackground: Cesium.Color.BLACK,
        labelOffset: new Cesium.Cartesian2( 8, 0),
        labelDC: new Cesium.DistanceDisplayCondition( 0, Number.MAX_VALUE),
        pointSize: 5,
        lineWidth: 2,
        fill: true, 
        fillAlpha: 0.3,
        pointDC: new Cesium.DistanceDisplayCondition( 0, Number.MAX_VALUE),
    }
}
