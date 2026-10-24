// example windninja_service.js module config

const windSpeedColors = [ // in 5mph increments
    Cesium.Color.fromCssColorString("#d9cfff"), // < 5mph
    Cesium.Color.fromCssColorString("#20d4f8"),   // < 10mph
    Cesium.Color.fromCssColorString("#0bd25e"), // < 15mph
    Cesium.Color.fromCssColorString("#ddff00"), // < 20mph
    Cesium.Color.fromCssColorString("#ffc400"), // < 25mph
    Cesium.Color.fromCssColorString("#FF4500"),   // < 30mph
    Cesium.Color.fromCssColorString("#d000d0")    // > 30mph
];

export const config = {
    layer: {
        name: "/weather/wind",
        description: "wind forecasts",
        show: true,
    },

    backHours: 2, // max duration for past forecasts we keep

    zoomMargin: 0.03, // in degrees lon/lat when zooming in on region bbox

    regionRender: {
        color: Cesium.Color.fromCssColorString('yellow'),
        lineWidth: 1.5,
        //fill: {
        //    color: Cesium.Color.fromCssColorString('yellow'),
        //    alpha: 0.3
        //}
    },

    //--- wind field render options (see odin_wind/windfield.js WindFieldRenderControls)
    windRender: {
        colors: windSpeedColors, // in 5mph steps - used by all display types
        lineWidth: 1.9,          // [pixel] particle trails, vector lines and contour outlines

        anim: {
            particlesTextureSize: 64, // sqrt of number of particles if the view is completely covered by the wind field
            speedFactor: 0.12,
            particleHeight: 0.0,
            fadeOpacity: 0.99,
            dropRate: 0.002,
            dropRateBump: 0.01,
            colorBySpeed: true,       // use colors, otherwise single color
            color: Cesium.Color.fromCssColorString('yellow')
        },

        vector: {
            pointSize: 5.0
        },

        contour: {
            alpha: 0.9
        }
    }
};
