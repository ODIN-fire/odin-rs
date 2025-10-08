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

    vectorRender: { 
        pointSize: 5.0, 
        strokeWidth: 1.5, 
        colors: windSpeedColors
    },

    animRender: { 
        particlesTextureSize: 64, 
        maxParticles: 4096, 
        lineWidth: 1.5, 
        color: Cesium.Color.fromCssColorString('yellow'), 
        speedFactor: 0.12, 
        particleHeight: 0.0, 
        fadeOpacity: 0.99, 
        dropRate: 0.002, 
        dropRateBump: 0.01
    },

    contourRender: { 
        strokeWidth: 1.5,
        alpha: 0.9,
        colors: windSpeedColors
    }
};