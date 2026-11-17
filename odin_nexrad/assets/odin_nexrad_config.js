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
        show: true,
    },

    // product tabs (names have to match the server products)
    products: [
        { name: "bref", label: "base", title: "Base Reflectivity" },
        { name: "cref", label: "composite", title: "Composite Reflectivity" },
    ],
    product: "bref",   // initially shown product

    maximumLevel: 9,   // most detailed (geographic tiling scheme) level we request (products have 0.005deg cells)
    opacity: 0.7,      // initial layer opacity
    showLatest: true,  // initially show (and follow) the latest time step of all stations

    // station symbols
    station: {
        color: Cesium.Color.fromCssColorString("#00ffff"),
        noDataColor: Cesium.Color.fromCssColorString("#808080"),
        outlineColor: Cesium.Color.BLACK,
        pointSize: 8,
        labelFont: "14px sans-serif",
        labelOffset: new Cesium.Cartesian2( 8, 0),
        labelBackground: Cesium.Color.fromCssColorString("#00000060"),
        labelDC: new Cesium.DistanceDisplayCondition( 0, 3000000),
    },
}
