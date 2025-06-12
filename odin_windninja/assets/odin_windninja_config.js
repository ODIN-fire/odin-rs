// example windninja_service.js module config

export const config = {
    back_hours: 1, // number of past forecast hours we keep

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
        pointSize: 4.0, 
        strokeWidth: 1.5, 
        color: Cesium.Color.fromCssColorString('blue')
    },

    animRender: { 
        particlesTextureSize: 64, 
        maxParticles: 4096, 
        lineWidth: 1.5, 
        color: Cesium.Color.fromCssColorString('yellow'), 
        speedFactor: 0.2, 
        particleHeight: 0.0, 
        fadeOpacity: 0.99, 
        dropRate: 0.002, 
        dropRateBump: 0.01
    },
    
    contourRender: { 
        strokeWidth: 2.0, 
        strokeColor: Cesium.Color.fromCssColorString('hotpink'), 
        fillColors:[
            Cesium.Color.fromCssColorString('#f0000000'),
            Cesium.Color.fromCssColorString('#f0000040'),
            Cesium.Color.fromCssColorString('#f0000060'),
            Cesium.Color.fromCssColorString('#f0000080'),
            Cesium.Color.fromCssColorString('#f00000a0')
        ]
    }
};