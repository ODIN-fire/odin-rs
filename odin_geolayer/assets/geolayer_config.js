export const config = {
    layer: {
        name:"/overlay/annotation",
        description:"static map overlays with symbolic data",
        show: true,
    },
    
    sources: [
        { pathName: "utilities/powerlines/ca",
            file: './hifld/Electric_Power_Transmission_Lines-CA-100122.geojson',
            info: '<a target: \'_blank\' href: \'https://hifld-geoplatform.opendata.arcgis.com/datasets/electric-power-transmission-lines/explore?location: 37.235258%2C-120.490264%2C6.86\'>HIFLD Electric Power Transmission Lines in CA 10/01/2022</a>',
            date: '10/01/2022',
            render: { strokeWidth: 1.5, stroke: "#48D1CC", fill: "#48D1CC" }
        },
        { pathName: 'utilities/substations/ca',
            file: './hifld/Electric_Substations-CA-100122.geojson',
            info: 'HIFLD electric substations in CA 10/01/2022',
            date: '10/01/2022',
            render: { markerSymbol: 's' }
        },
        { pathName: 'comm/cell_towers/ca',
            file: './hifld/CellularTowers-CA100122.geojson',
            info: 'HIFLD cell towers in CA 10/01/2022',
            date: '10/01/2022',
            render: { markerSymbol: 'c' }
        },
        { pathName: 'comm/radio_towers/ca',
            file: './hifld/FM__Transmission__Towers-CA-100122.geojson',
            info: 'HIFLD FM radio towers in CA 10/01/2022',
            date: '10/01/2022',
            render: { markerSymbol: 'r' }
        },
        { pathName: 'emergency/fire_stations/ca',
            file: './hifld/Fire_Stations-CA-100122.geojson.gz',
            info: 'HIFLD fire stations in CA 10/01/2022',
            date: '10/01/2022',
            render: { markerSymbol: './asset/odin_geolayer/firestation.png', markerColor: 'red' } // requires extsym.js module
        },
        { pathName: 'community/buildings',
            file: './ah/ah-buildings.geojson',
            info: 'sample Aldercroft Heights Buildings 10/16/2022',
            date: '10/16/2022',
            render: { markerSymbol: 'i', markerColor: 'yellow', stroke: 'yellow', fill: 'orange' }
        },
        { pathName: 'community/roads',
            file: './ah/ah-roads.geojson',
            info: 'sample Aldercroft Heights access/escape routes 10/16/2022',
            date: '10/16/2022',
            render: { markerSymbol: './asset/odin_geolayer/warning.png', markerColor: 'red', stroke: 'red', module: './road.js' }
        },
        { pathName: 'boundaries/counties/CA',
            file: './hifld/CA_County_Boundaries.geojson',
            info: 'California county boundaries',
            date: '10/01/2022',
            render: { stroke: 'red', strokeWidth: 3, fill: '#ff000000', module: './county_boundaries.js' }
        }
    ],

    render: { // default render parameters
            stroke: '#48D1CC',
            stroke:  '#48D1CC',
            strokeWidth: 2,
            fill: '#48D1CC',
            //markerColor: '#48D1CC'
            markerColor: 'cyan',
            markerSize: 32,
            module: './extsym.js'
    }
};