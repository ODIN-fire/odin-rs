
export const config = {
    layer: {
        name: "/background/imagery",
        description: "static imagery",
        show: true,
    },
    
    sources: [
        { pathName:'globe/natgeo',
            info:'ArcGIS NatGeo Terrain',
            provider: Cesium.ArcGisMapServerImageryProvider.fromUrl('proxy/globe-natgeo'),
            exclusive:['globe'],
            show:true,
            render:{ brightness:0.6 }
        },
        { pathName:'globe/openstreetmap',
            info:'OpenStreetMap',
            provider: new Cesium.OpenStreetMapImageryProvider({ url : 'proxy/globe-osm' }),
            //provider: new Cesium.OpenStreetMapImageryProvider({ url : 'https://tile.openstreetmap.org' }),
            exclusive:['globe'],
            render:{ brightness:0.6 }
        },
        { pathName:'globe/opentopomap',
            info:'OpenTopoMap',
            provider: new Cesium.OpenStreetMapImageryProvider({ url : 'proxy/globe-otm' }),
            //provider: new Cesium.OpenStreetMapImageryProvider({ url : 'https://tile.opentopomap.org' }),
            exclusive:['globe'],
            render:{ brightness:0.6 }
        },
        { pathName:'globe/bing-aerial',
            info:'Bing aerial default',
            style: Cesium.IonWorldImageryStyle.AERIAL_WITH_LABELS,
            exclusive:['globe'],
            render:{ brightness:1.0, contrast:1.0, hue:0.0 }
        }
    ],

    render: { alpha:1.0, brightness:1.0, contrast:1.0, hue:0.0, saturation:1.0, gamma:1.0 }
};