export const config = {
    layer: {
        name: "/background/landfire",
        description: "landfire imagery",
        show: true,
    },

    sources: [
        {
            pathName: "LF2025/EVT",
            info: "existing vegetation type",
            provider: Cesium.TileMapServiceImageryProvider.fromUrl( "landfire/tms/LF2025_EVT_CONUS"),
            exclusive: ["landfire"],
            colorMapUrl: "./landfire/data/LF2025_EVT.json",
            render:{ brightness:1.0, contrast:1.0, hue:0.0, alphaColor: "white" }
        }
        ,{
            pathName: "LF2025/FBFM40",
            info: "Scott Burgan fire behavior fuel model",
            provider: Cesium.TileMapServiceImageryProvider.fromUrl( "landfire/tms/LF2025_EVT_CONUS"),
            exclusive: ["landfire"],
            colorMapUrl: "./landfire/data/LF2025_FBFM40.json",
            render:{ brightness:1.0, contrast:1.0, hue:0.0, alphaColor: "white" }
        }
        ,{
            pathName: "LF2025/CC",
            info: "tree canopy cover",
            provider: Cesium.TileMapServiceImageryProvider.fromUrl( "landfire/tms/LF2025_CC_CONUS"),
            exclusive: ["landfire"],
            colorMapUrl: "./landfire/data/LF2025_CC.json",
            render:{ brightness:1.0, contrast:1.0, hue:0.0 }
        }
        ,{
            pathName: "LF2025/CH",
            info: "tree canopy height",
            provider: Cesium.TileMapServiceImageryProvider.fromUrl( "landfire/tms/LF2025_CH_CONUS"),
            exclusive: ["landfire"],
            colorMapUrl: "./landfire/data/LF2025_CH.json",
            render:{ brightness:1.0, contrast:1.0, hue:0.0 }
        }
        //... and more to follow
    ],

    render: { alpha:1.0, brightness:1.0, contrast:1.0, hue:0.0, saturation:1.0, gamma:1.0 }
}
