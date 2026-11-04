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

    barrierWidths: [5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 60, 90, 120, 150],

    // those are in ft
    handLineWidth: 8,
    dozerLineWidth: 12,
    seatDropWidth: 60,
    latDropWidth: 90,
    vlatDropWidth: 130,

    // air drop agents - names have to match ElmfireConfig.drop_agents (odin_elmfire/configs/elmfire.ron)
    dropAgents: [
        "LTR",  // long time retardant
        "water"
    ],
    defaultDropAgents: { 'SEAT-drop': 'LTR', 'LAT-drop': 'LTR', 'VLAT-drop': 'LTR', 'bucket-T3': 'water', 'bucket-T2': 'water', 'bucket-T1': 'water' },

    // helicopter bucket drop patterns (ellipses) in ft. These are estimates for water at a coverage level of ~1 gal/100sqft,
    // based on the Griffith 100 gal bucket tests (USDA FS MTDC 0057-2829: 70 gal water produce a 157ft line at CL 1, i.e.
    // ~7000 sqft), scaled with sqrt(volume)
    bucketPatterns: {
        'bucket-T3': { length: 190, width: 70 },   // type 3 helicopter, 100 gal
        'bucket-T2': { length: 375, width: 135 },  // type 2 helicopter, 400 gal
        'bucket-T1': { length: 590, width: 215 },  // type 1 helicopter, 1000 gal
    },
    maxDropStep: 24, // [h] max time step (hours after full hour before first ignition) for air drops

    pointSize: 4,
    pointColor: Cesium.Color.RED,
    pointOutlineColor: Cesium.Color.YELLOW,
    pointOutlineWidth: 1,
    pointDC: new Cesium.DistanceDisplayCondition( 70000, Number.MAX_VALUE),

    ignPointSize: 4,
    ignPointColor: Cesium.Color.RED,
    ignPointOutlineColor: Cesium.Color.YELLOW,
    ignPointOutlineWidth: 1,
    ignPointDC: new Cesium.DistanceDisplayCondition( 0, 10000),
    ignLineColor: Cesium.Color.RED,
    ignLineWidth: 3, // [pixel]
    ignitionDelays: [0, 10, 15, 20, 30, 45, 60, 90, 120, 180], // [min] after dtg for new ignitions

    labelColor: Cesium.Color.YELLOW,
    labelFont: '14px sans-serif',
    smallLabelFont: '11px sans-serif',
    labelBackground: Cesium.Color.fromCssColorString('#00000080'),
    labelOffset: new Cesium.Cartesian2( 8, 0),
    labelDC: new Cesium.DistanceDisplayCondition(0, 50000),

    polyDC: new Cesium.DistanceDisplayCondition(0, 20000),

    billboardDC: new Cesium.DistanceDisplayCondition(0, 50000),
    billboardColor: Cesium.Color.YELLOW,

    barrierDC: new Cesium.DistanceDisplayCondition(0, 5000), // for both labels and geometries

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
            widthBySpeed: true,       // scale line width with wind speed
            maxWidthFactor: 2.0,      // max line width factor (reached at widthSpeedMax)
            widthSpeedMax: 30,        // [mph] wind speed for max line width
            edgeDarkening: 0.3,       // [0..1] darkening of line edges (only for wide lines)
            color: Cesium.Color.fromCssColorString('yellow')
        },

        vector: {
            pointSize: 4.0
        },

        contour: {
            alpha: 0.6
        }
    },

    // fire specific rendering options (can be interactively modified per fire entry)
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
        fl: { color: Cesium.Color.DEEPPINK },
    },

    // this is separate since we also need to show barrier input before a new fire is created
    barrierRender: {
        colors: { // per barrier type
            'hand-line': Cesium.Color.fromCssColorString('#00ff91'),
            'dozer-line': Cesium.Color.fromCssColorString('#00ff91'),
            'SEAT-drop': Cesium.Color.fromCssColorString('#00ffff'),
            'LAT-drop': Cesium.Color.fromCssColorString('#00ffff'),
            'VLAT-drop': Cesium.Color.fromCssColorString('#00ffff'),
            'bucket-T3': Cesium.Color.fromCssColorString('#00ffff'),
            'bucket-T2': Cesium.Color.fromCssColorString('#00ffff'),
            'bucket-T1': Cesium.Color.fromCssColorString('#00ffff')
        },
        lineWidths: { // in pixels
            'hand-line': 2,
            'dozer-line': 3,
            'SEAT-drop': 4,
            'LAT-drop': 6,
            'VLAT-drop': 8,
            'bucket-T3': 2,
            'bucket-T2': 3,
            'bucket-T1': 4
        },
        defaultLineWidth: 5,
        defaultColor: Cesium.Color.fromCssColorString('#ff00ff'),
    },
};
