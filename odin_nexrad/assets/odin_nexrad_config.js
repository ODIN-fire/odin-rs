/**
 * Copyright 2026 Peter Mehlitz
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://apache.org
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

// static config for the odin_nexrad module

export const config = {
    layer: {
        name: "/weather/nexrad",
        description: "NEXRAD Level II radar products",
        show: false,
    },

    // product tabs (names have to match the server products)
    products: [
        { name: "bref", label: "base", title: "Base Reflectivity" },
        { name: "cref", label: "composite", title: "Composite Reflectivity" },
    ],
    product: "bref",   // initially shown product

    maximumLevel: 9,   // most detailed (geographic tiling scheme) level we request (~150m pixels, products have 250m cells)
    opacity: 0.7,      // initial layer opacity
    showLatest: true,  // initially show (and follow) the latest time step of the shown stations
    maxAge: 15,        // [min] initial max age of displayed time steps at the time cursor (display panel)
    nearestOnTop: true, // initially put the station closest to the view center on top where stations overlap (display panel)
    overview: true,    // initially show server mosaics of all stations instead of station layers (avoids loading all WMS layers)
    overviewUrl: "nexrad/mosaic", // where we get the mosaic GeoJSON files from
    // overview band fill colors as [dBZ, css-color] pairs (ascending). A band gets the color of the highest entry that is
    // <= its lower bound (server mosaic `levels`), bands below the first entry use the server default (station layer) color
    overviewColors: [
        [10, "#87ceeb"],
        [20, "#00ff00"],
        [30, "#228b22"],
        [40, "#eedc82"],
        [50, "#ff3030"],
    ],

    // station symbols
    station: {
        color: Cesium.Color.fromCssColorString("#ff0000"),
        noDataColor: Cesium.Color.fromCssColorString("#808080"),
        outlineColor: Cesium.Color.YELLOW,
        pointSize: 8,
        labelFont: "14px sans-serif",
        labelOffset: new Cesium.Cartesian2(8, 0),
        labelColor: Cesium.Color.YELLOW,
        labelBackground: Cesium.Color.fromCssColorString("#00000060"),
        labelDC: new Cesium.DistanceDisplayCondition( 0, 3000000),
        infoFont: "12px monospace", // station info (name, displayed time step and age, latest time step)
        infoOffset: new Cesium.Cartesian2(8, 12),
        infoDC: new Cesium.DistanceDisplayCondition( 0, 200000),
    },
}
