
//--- support objects to instantiate times WMTS (weather)
//var now = Cesium.JulianDate.now();
//
//const clock = new Cesium.Clock({
//   startTime : now,
//   currentTime : now,
//   stopTime : Cesium.JulianDate.fromIso8601("2026-12-31"),
//   clockRange : Cesium.ClockRange.LOOP_STOP,
//   clockStep : Cesium.ClockStep.SYSTEM_CLOCK_MULTIPLIER
//});
//
//const times = Cesium.TimeIntervalCollection.fromIso8601({
//    iso8601: '2025-06-13/2026-12-31/P1D',
//    dataCallback: function dataCallback(interval, index) {
//        return {
//            Time: Cesium.JulianDate.toIso8601(interval.start)
//        };
//    }
//});

export const config = {
    layer: {
        name: "/background/imagery",
        description: "static imagery",
        show: true,
    },
    
    sources: [
        { 
            pathName:'globe/natgeo',
            info:'ArcGIS NatGeo Terrain',
            provider: Cesium.ArcGisMapServerImageryProvider.fromUrl( './proxy/globe-natgeo'),
            exclusive:['globe'],
            show:true,
            render:{ brightness:0.6 }
        },
        { 
            pathName:'globe/openstreetmap',
            info:'OpenStreetMap',
            provider: new Cesium.OpenStreetMapImageryProvider({ url : './proxy/globe-osm' }),
            //provider: new Cesium.OpenStreetMapImageryProvider({ url : 'https://tile.openstreetmap.org' }),
            exclusive:['globe'],
            render:{ brightness:0.6 }
        },
        { 
            pathName:'globe/opentopomap',
            info:'OpenTopoMap',
            provider: new Cesium.OpenStreetMapImageryProvider({ url : './proxy/globe-otm' }),
            //provider: new Cesium.OpenStreetMapImageryProvider({ url : 'https://tile.opentopomap.org' }),
            exclusive:['globe'],
            render:{ brightness:0.6 }
        },
        { 
            pathName:'globe/bing-aerial',
            info:'Bing aerial default',
            style: Cesium.IonWorldImageryStyle.AERIAL_WITH_LABELS,
            exclusive:['globe'],
            render:{ brightness:1.0, contrast:1.0, hue:0.0 }
        },

        //{ 
        //    pathName: 'weather/amsr2',
        //    info: 'NASA AMSR2 snow water equivalent',
        //    provider: new Cesium.WebMapTileServiceImageryProvider({
        //        url : 'https://gibs.earthdata.nasa.gov/wmts/epsg4326/best/AMSR2_Snow_Water_Equivalent/default/{Time}/{TileMatrixSet}/{TileMatrix}/{TileRow}/{TileCol}.png',
        //        layer : 'AMSR2_Snow_Water_Equivalent',
        //        style : 'default',
        //        tileMatrixSetID : '2km',
        //        maximumLevel : 5,
        //        format : 'image/png',
        //        clock: clock,  // <<<<<
        //        //times: times,  // <<<<<
        //        credit : new Cesium.Credit('NASA Global Imagery Browse Services for EOSDIS')
        //    }),
        //    exclusive: [],
        //    render:{ brightness:1.0, contrast:1.0, hue:0.0 }
        //},
        { 
            pathName: "fuel/landfire/cover",
            info: "landfire fuel vegetation cover (FVC 230)",
            provider: new Cesium.WebMapServiceImageryProvider({
              url: "https://edcintl.cr.usgs.gov/geoserver/landfire/us_230/ows",
              layers: "LC22_FVC_230",
              parameters: "format=image/png"
            }),
            exclusive:["fuel"],
            render:{ brightness:1.0, contrast:1.0, hue:0.0, alphaColor: "white" }
        },
        {
            pathName: "fuel/landfire/type",
            info: "landfire fuel vegetation type (FVT 230)",
            provider: new Cesium.WebMapServiceImageryProvider({
                url: "https://edcintl.cr.usgs.gov/geoserver/landfire/us_230/ows",
                layers: "LC22_FVT_230",
                parameters: "format=image/png"
            }),
            exclusive: ["fuel"],
            colorMap: "./asset/odin_cesium/landfire/LF20_FVT_220.json",
            render:{ brightness:1.0, contrast:1.0, hue:0.0, alphaColor: "white" }
        },
        //{  // not everybody has the VERM data
        //    pathName: "fuel/VERM",
        //    info: "Vegetation Ember Relative Mass index 2023",
        //    provider: Cesium.TileMapServiceImageryProvider.fromUrl( "./tms/verm"),
        //    exclusive: ["fuel"],
        //    render:{ brightness:1.0, contrast:1.0, hue:0.0 }
        //},

        {
            pathName: "mtbs/burn_severity/2018",
            info: "burn severity 2018 from MTBS",
            provider: new Cesium.WebMapServiceImageryProvider({
                url: "https://apps.fs.usda.gov/arcx/services/RDW_Wildfire/MTBS_CONUS/MapServer/WMSServer",
                layers: "34"
            }),
            exclusive:[],
            colorMap: "./asset/odin_cesium/mtbs/burn-severity-conus-2020.json",
            render: { alphaColor: "white", alphaColorThreshold: 0.1 }
        },
        {
            pathName: "mtbs/burn_severity/2020",
            info: "burn severity 2020 from MTBS",
            provider: new Cesium.WebMapServiceImageryProvider({
                url: "https://apps.fs.usda.gov/arcx/services/RDW_Wildfire/MTBS_CONUS/MapServer/WMSServer",
                layers: "36"
            }),
            colorMap: "./asset/odin_cesium/mtbs/burn-severity-conus-2020.json",
            render: { alphaColor: "white", alphaColorThreshold: 0.1 }
        },
        {
            pathName: "mtbs/burn_severity/2021",
            info: "burn severity 2021 from MTBS",
            provider: new Cesium.WebMapServiceImageryProvider({
                url: "https://apps.fs.usda.gov/arcx/services/RDW_Wildfire/MTBS_CONUS/MapServer/WMSServer",
                layers: "37"
            }),
            colorMap: "./asset/odin_cesium/mtbs/burn-severity-conus-2020.json",
            render: { alphaColor: "white", alphaColorThreshold: 0.1 }
        },
        {
            pathName: "mtbs/burn_severity/2022",
            info: "burn severity 2021 from MTBS",
            provider: new Cesium.WebMapServiceImageryProvider({
                url: "https://apps.fs.usda.gov/arcx/services/RDW_Wildfire/MTBS_CONUS/MapServer/WMSServer",
                layers: "38"
            }),
            colorMap: "./asset/odin_cesium/mtbs/burn-severity-conus-2020.json",
            render: { alphaColor: "white", alphaColorThreshold: 0.1 }
        },
        {
            pathName: "mtbs/burn_severity/2023",
            info: "burn severity 2023 from MTBS",
            provider: new Cesium.WebMapServiceImageryProvider({
                url: "https://apps.fs.usda.gov/arcx/services/RDW_Wildfire/MTBS_CONUS/MapServer/WMSServer",
                layers: "39"
            }),
            colorMap: "./asset/odin_cesium/mtbs/burn-severity-conus-2020.json",
            render: { alphaColor: "white", alphaColorThreshold: 0.1 }
        },
        {
            pathName: "mtbs/burn_severity/2024",
            info: "burn severity 2024 from MTBS",
            provider: new Cesium.WebMapServiceImageryProvider({
                url: "https://apps.fs.usda.gov/arcx/services/RDW_Wildfire/MTBS_CONUS/MapServer/WMSServer",
                layers: "40"
            }),
            colorMap: "./asset/odin_cesium/mtbs/burn-severity-conus-2020.json",
            render: { alphaColor: "white", alphaColorThreshold: 0.1 }
        },

        {
            pathName: "mtbs/fire_boundaries/2018",
            info: "fire boundaries 2018 from MTBS",
            provider: new Cesium.WebMapServiceImageryProvider({
                url: "https://apps.fs.usda.gov/arcx/services/EDW/EDW_MTBS_01/MapServer/WMSServer",
                layers: "76"
            }),
            render: { hue: 218, saturation: 1.5, alphaColor: "white", alphaColorThreshold: 0.1 }
        },        
        {
            pathName: "mtbs/fire_boundaries/2019",
            info: "fire boundaries 2019 from MTBS",
            provider: new Cesium.WebMapServiceImageryProvider({
                url: "https://apps.fs.usda.gov/arcx/services/EDW/EDW_MTBS_01/MapServer/WMSServer",
                layers: "77"
            }),
            render: { hue: 218, saturation: 1.5, alphaColor: "white", alphaColorThreshold: 0.1 }
        },
        {
            pathName: "mtbs/fire_boundaries/2020",
            info: "fire boundaries 2020 from MTBS",
            provider: new Cesium.WebMapServiceImageryProvider({
                url: "https://apps.fs.usda.gov/arcx/services/EDW/EDW_MTBS_01/MapServer/WMSServer",
                layers: "78"
            }),
            render: { hue: 218, saturation: 1.5, alphaColor: "white", alphaColorThreshold: 0.1 }
        },        
        {
            pathName: "mtbs/fire_boundaries/2021",
            info: "fire boundaries 2021 from MTBS",
            provider: new Cesium.WebMapServiceImageryProvider({
                url: "https://apps.fs.usda.gov/arcx/services/EDW/EDW_MTBS_01/MapServer/WMSServer",
                layers: "79"
            }),
            render: { hue: 218, saturation: 1.5, alphaColor: "white", alphaColorThreshold: 0.1 }
        },
        {
            pathName: "mtbs/fire_boundaries/2022",
            info: "fire boundaries 2022 from MTBS",
            provider: new Cesium.WebMapServiceImageryProvider({
                url: "https://apps.fs.usda.gov/arcx/services/EDW/EDW_MTBS_01/MapServer/WMSServer",
                layers: "80"
            }),
            render: { hue: 218, saturation: 1.5, alphaColor: "white", alphaColorThreshold: 0.1 }
        },        
        {
            pathName: "mtbs/fire_boundaries/2023",
            info: "fire boundaries 2023 from MTBS",
            provider: new Cesium.WebMapServiceImageryProvider({
                url: "https://apps.fs.usda.gov/arcx/services/EDW/EDW_MTBS_01/MapServer/WMSServer",
                layers: "81"
            }),
            render: { hue: 218, saturation: 1.5, alphaColor: "white", alphaColorThreshold: 0.1 }
        },
        {
            pathName: "mtbs/fire_boundaries/2024",
            info: "fire boundaries 2024 from MTBS",
            provider: new Cesium.WebMapServiceImageryProvider({
                url: "https://apps.fs.usda.gov/arcx/services/EDW/EDW_MTBS_01/MapServer/WMSServer",
                layers: "82"
            }),
            render: { hue: 218, saturation: 1.5, alphaColor: "white", alphaColorThreshold: 0.1 }
        }
    ],

    render: { alpha:1.0, brightness:1.0, contrast:1.0, hue:0.0, saturation:1.0, gamma:1.0 }
};