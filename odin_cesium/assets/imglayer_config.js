
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
            provider: Cesium.ArcGisMapServerImageryProvider.fromUrl('proxy/globe-natgeo'),
            exclusive:['globe'],
            show:true,
            render:{ brightness:0.6 }
        },
        { 
            pathName:'globe/openstreetmap',
            info:'OpenStreetMap',
            provider: new Cesium.OpenStreetMapImageryProvider({ url : 'proxy/globe-osm' }),
            //provider: new Cesium.OpenStreetMapImageryProvider({ url : 'https://tile.openstreetmap.org' }),
            exclusive:['globe'],
            render:{ brightness:0.6 }
        },
        { 
            pathName:'globe/opentopomap',
            info:'OpenTopoMap',
            provider: new Cesium.OpenStreetMapImageryProvider({ url : 'proxy/globe-otm' }),
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
            pathName: "fuel/cover",
            info: "landfire fuel vegetation cover (FVC 220)",
            provider: new Cesium.WebMapServiceImageryProvider({
              url: "https://edcintl.cr.usgs.gov/geoserver/landfire/us_220/ows",
              layers: "LC22_FVC_220",
              parameters: "format=image/png"
            }),
            exclusive:["fuel"],
            render:{ brightness:1.0, contrast:1.0, hue:0.0, alphaColor: "white" }
        },
        {
            pathName: "fuel/type",
            info: "landfire fuel vegetation type (FVT 220)",
            provider: new Cesium.WebMapServiceImageryProvider({
                url: "https://edcintl.cr.usgs.gov/geoserver/landfire/us_220/ows",
                layers: "LC22_FVT_220",
                parameters: "format=image/png"
            }),
            exclusive: ["fuel"],
            colorMap: "./asset/odin_cesium/landfire/LF20_FVT_220.json",
            render:{ brightness:1.0, contrast:1.0, hue:0.0, alphaColor: "white" }
        },
        {
            pathName: "fuel/VERM",
            info: "Vegetation Ember Relative Mass index 2023",
            provider: await Cesium.TileMapServiceImageryProvider.fromUrl("./tms/verm"),
            exclusive: ["fuel"],
            render:{ brightness:1.0, contrast:1.0, hue:0.0 }
        }
    ],

    render: { alpha:1.0, brightness:1.0, contrast:1.0, hue:0.0, saturation:1.0, gamma:1.0 }
};