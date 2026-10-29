const windSpeedColors = [ // in 5mph increments
    // TODO - adjust to make higher windspeed stand out on darker background
    Cesium.Color.fromCssColorString("#d9cfff"), // < 5mph
    Cesium.Color.fromCssColorString("#20d4f8"), // < 10mph
    Cesium.Color.fromCssColorString("#0bd25e"), // < 15mph
    Cesium.Color.fromCssColorString("#ddff00"), // < 20mph
    Cesium.Color.fromCssColorString("#ffc400"), // < 25mph
    Cesium.Color.fromCssColorString("#FF4500"), // < 30mph
    Cesium.Color.fromCssColorString("#d000d0")  // > 30mph
];

export const config = {
    layer: {
        name: "/overlay/elmfire",
        description: "Elmfire fire behavior simulator",
        show: true,
    },
    maxEntries: 20,

    contourThreshold: 15, // timestep polygons with less vertices are ignored when displayed
    labelCandidates: 24,  // number of evenly spaced outline vertices considered for isochrone timestep label positions
    labelMaxNeighborPts: 400, // max number of neighbor isochrone vertices used to compute label position gaps

    //--- fixed wx input defaults
    fixedWs: [5, 10, 15, 20, 25, 30, 35, 40, 50, 60],
    defaultWs: 2,

    fixedWd: [0, 45, 90, 135, 180, 225, 270, 315],
    defaultWd: 7,

    fixedM1: [0, 2, 5, 7, 10, 20],
    defaultM1: 1,

    fixedM10: [0, 5, 10, 15, 20],
    defaultM10: 1,

    fixedM100: [0, 10, 20, 30],
    defaultM100: 1,

    fixedLh: [25, 30, 35],
    defaultLh: 1,

    fixedLw: [55, 60, 65],
    defaultLw: 1,

    fixedT2: [0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],  // temperature at 2m
    defaultT2: 8,

    fixedTcc: [0, 30, 70, 100], // pervent total cloud coverage
    defaultTcc: 0,

    barrierWidths: [5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 60, 90],
    barrierRender: {
        lineWidth: 3, // [pixel]
        colors: { // per barrier type
            'SEAT-drop': Cesium.Color.fromCssColorString('#ff8c00'),
            'LAT-drop': Cesium.Color.fromCssColorString('#ff3030'),
            'VLAT-drop': Cesium.Color.fromCssColorString('#c00000'),
            'dozer-line': Cesium.Color.fromCssColorString('#cd853f'),
            'hand-line': Cesium.Color.fromCssColorString('#f0e68c'),
        },
        defaultColor: Cesium.Color.fromCssColorString('#ff00ff'),
    },
    handLineWidth: 8,
    dozerLineWidth: 12,
    seatDropWidth: 60,
    latDropWidth: 90,
    vlatDropWidth: 130,

    pointSize: 4,
    pointColor: Cesium.Color.RED,
    pointOutlineColor: Cesium.Color.YELLOW,
    pointOutlineWidth: 1,
    pointDC: new Cesium.DistanceDisplayCondition( 70000, Number.MAX_VALUE),

    ignPointSize: 4,
    ignPointColor: Cesium.Color.RED,
    ignPointOutlineColor: Cesium.Color.YELLOW,
    ignPointOutlineWidth: 1,
    ignPointDC: new Cesium.DistanceDisplayCondition( 0, 30000),
    ignLineColor: Cesium.Color.RED,
    ignLineWidth: 3, // [pixel]
    ignitionDelays: [0, 10, 15, 20, 30, 45, 60, 90, 120, 180], // [min] after dtg for new ignitions

    labelColor: Cesium.Color.YELLOW,
    labelFont: '14px sans-serif',
    smallLabelFont: '11px sans-serif',
    labelBackground: Cesium.Color.fromCssColorString('#00000080'),
    labelOffset: new Cesium.Cartesian2( 8, 0),
    labelDC: new Cesium.DistanceDisplayCondition(0, 50000),

    billboardDC: new Cesium.DistanceDisplayCondition(0, 50000),
    billboardColor: Cesium.Color.YELLOW,

    zoomHeight: 10000,

    imgOutput: [ "flin", "vs", "cf", "fl" ],

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
            color: Cesium.Color.fromCssColorString('yellow')
        },

        vector: {
            pointSize: 4.0
        },

        contour: {
            alpha: 0.6
        }
    },

    render: {
        toa: {
            stroke: Cesium.Color.YELLOW,
            strokeWidth: 1.2,
            fill: Cesium.Color.RED,
            alpha: 0.2
        },
        flin: { color: Cesium.Color.RED },
        vs: { color: Cesium.Color.MAGENTA },
        cf: { color: Cesium.Color.GREENYELLOW },
        fl: { color: Cesium.Color.DEEPPINK }
    }
};
