/*
 * Copyright © 2024, United States Government, as represented by the Administrator of 
 * the National Aeronautics and Space Administration. All rights reserved.
 *
 * The “ODIN” software is licensed under the Apache License, Version 2.0 (the "License"); 
 * you may not use this file except in compliance with the License. You may obtain a copy 
 * of the License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software distributed under
 * the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,
 * either express or implied. See the License for the specific language governing permissions
 * and limitations under the License.
 */

console.log("geolayer module loaded: " +  new URL(import.meta.url).pathname.split("/").pop());

const defaultGeometryDC  = new Cesium.DistanceDisplayCondition(0, 350000);
const defaultBillboardDC = new Cesium.DistanceDisplayCondition(0, 200000);
const defaultStrokeWidth = 2;
const defaultStroke = Cesium.Color.MAGENTA;


export function render (entityCollection, opts) {
    for (const e of entityCollection.values) {
        let props = e.properties;

        if (e.polygon) {
            let name = getPropValue(props,'NAMELSAD');
            let lat = getPropValue(props,'INTPTLAT');
            let lon = getPropValue(props,'INTPTLON');

            if (name && lat && lon) {
                e.position = Cesium.Cartesian3.fromDegrees(lon, lat);

                e.label = {
                    text: name,
                    scale: 0.6,
                    fillColor: opts.stroke,
                    distanceDisplayCondition: (opts.billboardDC ? opts.billboardDC : defaultBillboardDC),
                };
            }

            // since clamp-to-ground polygons in Cesium do not support outlines we have to turn the polygon into a polyline
            e.polyline = {
                positions: e.polygon.hierarchy._value.positions,
                material: (opts.stroke ? opts.stroke : defaultStroke),
                width: (opts.strokeWidth ? opts.strokeWidth : defaultStrokeWidth),
                clampToGround: true,
                distanceDisplayCondition: defaultGeometryDC
            };
            e.addProperty('polyline');
            e.polygon = undefined; // TODO - maybe we keep it to allow fill
            e.removeProperty('polygon');
        }
    }
}

function getPropValue(props,key) {
    let p = props[key];
    return p ? p._value : undefined;
}