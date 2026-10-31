// landfire layers are only shown if the camera is below this height [m]. Landfire data has a 30m resolution and is
// computed on demand from the original GeoTIFFs - displaying large areas would be expensive and not useful
const MAX_CAMERA_HEIGHT = 150000;

// the CONUS extent of the landfire.gov data sets. We specify it explicitly so that Cesium does not request images
// outside of it. Note this has to be in sync with the layer bounds of the WMS capabilities (landfire/wms?request=GetCapabilities)
const CONUS_RECT = Cesium.Rectangle.fromDegrees( -128.39, 22.42, -64.05, 52.49);

// the most detailed (geographic tiling scheme) level we request. Level 12 is about 2x the 30m data resolution, Cesium
// magnifies images above that level
const WMS_MAX_LEVEL = 12;

// landfire data is mostly categorical - interpolation would create colors that don't correspond to any category. Since
// this preserves the data cell boundaries we also use it for continuous (topo) data
const CATEGORICAL_LAYER = {
    magnificationFilter: Cesium.TextureMagnificationFilter.NEAREST,
    minificationFilter: Cesium.TextureMinificationFilter.NEAREST
};

// the color (#RRGGBBAA) for nodata pixels of all layers (ocean, outside of CONUS or missing data) so that valid data
// boundaries can be recognized. Set to undefined to render nodata pixels transparent
const NODATA_COLOR = "#ff00ff60";

// colors and value ranges for continuous value layers rendered with the "alpha" WMS style. These are rendered with a
// single color and the value mapped to alpha (values <= min are transparent, values >= max are opaque, i.e. zero values
// are transparent). Ranges are optional (server config otherwise). Note the canopy data sets use scaled integer values
const ALPHA = {
    cc:        { color: "#00a000", range: [0, 95] },    // canopy cover [%]
    ch:        { color: "#006060", range: [0, 510] },   // canopy height [m*10]
    cbh:       { color: "#c06000", range: [0, 100] },   // canopy base height [m*10]
    cbd:       { color: "#8000c0", range: [0, 45] },    // canopy bulk density [kg/m3*100]
    slope:     { color: "#ff4000", range: [0, 60] },    // degrees
    aspect:    { color: "#0060ff", range: [0, 359] },   // degrees (-1 is flat)
    elevation: { color: "#a000ff", range: [0, 4400] },  // meters
};

// note that NDCOLOR, COLOR and VRANGE are vendor specific WMS parameters
function wmsProvider (layer, parameters={}) {
    let params = { format: "image/png", transparent: true, ...parameters };
    if (NODATA_COLOR) params.ndcolor = NODATA_COLOR.replace("#","");

    return new Cesium.WebMapServiceImageryProvider({
        url: "landfire/wms",
        layers: layer,
        parameters: params,
        rectangle: CONUS_RECT,
        maximumLevel: WMS_MAX_LEVEL,
        enablePickFeatures: false // we don't support GetFeatureInfo (yet)
    });
}

// layers rendered with the WMS "alpha" style
function alphaProvider (layer, opts) {
    let parameters = { styles: "alpha", color: opts.color.replace("#","") };
    if (opts.range) parameters.vrange = opts.range.join(",");
    return wmsProvider( layer, parameters);
}

export const config = {
    layer: {
        name: "/background/landfire",
        description: "landfire imagery",
        show: true,
    },

    maxCameraHeight: MAX_CAMERA_HEIGHT, // default for all sources (can be overridden per source)

    sources: [
        {
            pathName: "LF2025/EVT",
            info: "existing vegetation type",
            layerOpts: CATEGORICAL_LAYER,
            provider: wmsProvider( "LF2025_EVT"),
            exclusive: ["landfire"],
            colorMapUrl: "./landfire/cmap/LF2025_EVT",
            render:{ brightness:1.0, contrast:1.0, hue:0.0 }
        }
        ,{
            pathName: "LF2025/FBFM40",
            info: "Scott Burgan fire behavior fuel model",
            layerOpts: CATEGORICAL_LAYER,
            provider: wmsProvider( "LF2025_FBFM40"),
            exclusive: ["landfire"],
            colorMapUrl: "./landfire/cmap/LF2025_FBFM40",
            render:{ brightness:1.0, contrast:1.0, hue:0.0 }
        }

        //--- canopy layers with value mapped to alpha (can be shown on top of one of the above)
        ,{
            pathName: "LF2025/CC",
            info: "forest canopy cover [%]",
            layerOpts: CATEGORICAL_LAYER,
            provider: alphaProvider( "LF2025_CC", ALPHA.cc),
            exclusive: ["landfire"],
            render:{ brightness:0.6, contrast:1.3, hue:0.0 }
        }
        ,{
            pathName: "LF2025/CH",
            info: "forest canopy height [m*10]",
            layerOpts: CATEGORICAL_LAYER,
            provider: alphaProvider( "LF2025_CH", ALPHA.ch),
            exclusive: ["landfire"],
            render:{ brightness:0.8, contrast:1.5, hue:0.0 }
        }
        ,{
            pathName: "LF2025/CBH",
            info: "forest canopy base height [m*10]",
            layerOpts: CATEGORICAL_LAYER,
            provider: alphaProvider( "LF2025_CBH", ALPHA.cbh),
            exclusive: ["landfire"],
            render:{ brightness:0.8, contrast:1.3, hue:0.0 }
        }
        ,{
            pathName: "LF2025/CBD",
            info: "forest canopy bulk density [kg/m3*100]",
            layerOpts: CATEGORICAL_LAYER,
            provider: alphaProvider( "LF2025_CBD", ALPHA.cbd),
            exclusive: ["landfire"],
            render:{ brightness:0.8, contrast:1.3, hue:0.0 }
        }

        ,{
            pathName: "LF2025/cmap/CC",
            info: "forest canopy cover",
            layerOpts: CATEGORICAL_LAYER,
            provider: wmsProvider( "LF2025_CC"),
            exclusive: ["landfire"],
            colorMapUrl: "./landfire/cmap/LF2025_CC",
            render:{ brightness:1.0, contrast:1.0, hue:0.0 }
        }
        ,{
            pathName: "LF2025/cmap/CH",
            info: "forest canopy height",
            layerOpts: CATEGORICAL_LAYER,
            provider: wmsProvider( "LF2025_CH"),
            exclusive: ["landfire"],
            colorMapUrl: "./landfire/cmap/LF2025_CH",
            render:{ brightness:1.0, contrast:1.0, hue:0.0 }
        }
        ,{
            pathName: "LF2025/cmap/CBH",
            info: "forest canopy base height",
            layerOpts: CATEGORICAL_LAYER,
            provider: wmsProvider( "LF2025_CBH"),
            exclusive: ["landfire"],
            colorMapUrl: "./landfire/cmap/LF2025_CBH",
            render:{ brightness:1.0, contrast:1.0, hue:0.0 }
        }
        ,{
            pathName: "LF2025/cmap/CBD",
            info: "forest canopy bulk density",
            layerOpts: CATEGORICAL_LAYER,
            provider: wmsProvider( "LF2025_CBD"),
            exclusive: ["landfire"],
            colorMapUrl: "./landfire/cmap/LF2025_CBD",
            render:{ brightness:1.0, contrast:1.0, hue:0.0 }
        }

        //--- topo layers (can be shown on top of one of the above)
        ,{
            pathName: "topo/SlpD",
            info: "slope [deg]",
            layerOpts: CATEGORICAL_LAYER,
            provider: alphaProvider( "LF2020_SlpD", ALPHA.slope),
            exclusive: ["landfire"],
            render:{ brightness:1.0, contrast:1.0, hue:0.0 }
        }
        ,{
            pathName: "topo/Asp",
            info: "aspect [deg]",
            layerOpts: CATEGORICAL_LAYER,
            provider: alphaProvider( "LF2020_Asp", ALPHA.aspect),
            exclusive: ["landfire"],
            render:{ brightness:1.0, contrast:1.0, hue:0.0 }
        }
        ,{
            pathName: "topo/Elev",
            info: "elevation [m]",
            layerOpts: CATEGORICAL_LAYER,
            provider: alphaProvider( "LF2020_Elev", ALPHA.elevation),
            exclusive: ["landfire"],
            render:{ brightness:1.0, contrast:1.0, hue:0.0 }
        }
    ],

    render: { alpha:1.0, brightness:1.0, contrast:1.0, hue:0.0, saturation:1.0, gamma:1.0 }
}
