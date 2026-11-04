// example windninja_service.js module config

const windSpeedColors = [ // in 5mph increments. Higher speeds use darker, more saturated colors so that they stand out
    Cesium.Color.fromCssColorString("#c8c8ff"), // < 5mph
    Cesium.Color.fromCssColorString("#00b4ff"), // < 10mph
    Cesium.Color.fromCssColorString("#00c853"), // < 15mph
    Cesium.Color.fromCssColorString("#ffd600"), // < 20mph
    Cesium.Color.fromCssColorString("#ff6d00"), // < 25mph
    Cesium.Color.fromCssColorString("#e00000"), // < 30mph
    Cesium.Color.fromCssColorString("#c7006a")  // > 30mph
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
        lineWidth: 1.5,          // [pixel] particle trails, vector lines and contour outlines

        anim: {
            particlesTextureSize: 50, // sqrt of number of particles if the view is completely covered by the wind field
            speedFactor: 0.12,
            particleHeight: 0.0,
            fadeOpacity: 0.99,
            dropRate: 0.002,
            dropRateBump: 0.01,
            colorBySpeed: true,       // use colors, otherwise single color
            widthBySpeed: true,       // scale line width with wind speed
            maxWidthFactor: 2.0,      // max line width factor (reached at widthSpeedMax)
            widthSpeedMax: 30,        // [mph] wind speed for max line width
            edgeDarkening: 0.3,       // [0..1] darkening of line edges (only for wide lines)
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
