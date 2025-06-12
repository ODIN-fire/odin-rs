/*
 * Copyright (c) 2025, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The RACE - Runtime for Airspace Concept Evaluation platform is licensed
 * under the Apache License, Version 2.0 (the "License"); you may not use
 * this file except in compliance with the License. You may obtain a copy
 * of the License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

import * as ui from "../odin_server/ui.js";
import * as util from "../odin_server/ui_util.js";
import * as odinCesium from "../odin_cesium/odin_cesium.js";

import * as windUtils from "./wind-particles/windUtils.js";
import { ParticleSystem } from "./wind-particles/particleSystem.js";

//--- constants
const csvGridPrefixLine = /^# *nx: *(\d+), *x0: *(.+) *, *dx: *(.+) *, *ny: *(\d+), *y0: *(.+) *, *dy: *(.+)$/;
const csvVectorPrefixLine = /^# *length: *(\d+)$/;
const polyLineColorAppearance = new Cesium.PolylineColorAppearance();

const globeBoundingSphere = new Cesium.BoundingSphere(Cesium.Cartesian3.ZERO, 0.99 * 6378137.0);

export const DisplayType = {
    DISPLAY_ANIM: "anim",
    DISPLAY_VECTOR: "vector",
    DISPLAY_CONTOUR: "contour"
};

/// the status of WindFieldVisualization objects as exposed to our owning forecast object
export const WindFieldStatus = {
    REMOTE  : "" ,
    LOADING : "…",
    LOADED  : "○",
    SHOWING : "●"
};

var viewerParameters = {
    lonRange: new Cesium.Cartesian2(),
    latRange: new Cesium.Cartesian2(),
    pixelSize: 0.0
};

export function updateViewerParameters() {
    let viewer = odinCesium.viewer;

    var viewRectangle = viewer.camera.computeViewRectangle(viewer.scene.globe.ellipsoid);
    var lonLatRange = windUtils.viewRectangleToLonLatRange(viewRectangle);

    viewerParameters.lonRange.x = lonLatRange.lon.min;
    viewerParameters.lonRange.y = lonLatRange.lon.max;
    viewerParameters.latRange.x = lonLatRange.lat.min;
    viewerParameters.latRange.y = lonLatRange.lat.max;

    var pixelSize = viewer.camera.getPixelSize(
        globeBoundingSphere,
        viewer.scene.drawingBufferWidth,
        viewer.scene.drawingBufferHeight
    );

    if (pixelSize > 0) {
        viewerParameters.pixelSize = pixelSize;
    }
}

//--- types

/// the common base for all wind field visualization types
export class WindFieldVisualization {
    constructor ( displayType, urlBase, defaultRenderOpts, statusChangeCallback=null) {
        this.displayType = displayType;
        this.statusChangeCallback = statusChangeCallback;
        this.urlBase = urlBase;
        this.status = WindFieldStatus.REMOTE;
        this.show = false;
        this.renderOpts = {...defaultRenderOpts};
    }

    setStatus (newStatus) {
        this.status = newStatus;
        if (this.statusCallback) this.statusChangeCallback( this);
    }

    isShowing () {
        return Object.is( this.status, WindFieldStatus.SHOWING);
    }

    //--- those are called by our owner and have to be overridden
    displayType() {}
    setVisible (showIt) {}
    renderChanged() {}
    updateDisplayPanel() {}
    startViewChange() {}
    endViewChange() {}
}

/// a wind field with a custom particle system visualization
export class AnimField extends WindFieldVisualization {

    constructor (urlBase, defaultRenderOpts, statusChangeCallback=null) {
        super( DisplayType.DISPLAY_ANIM, urlBase, defaultRenderOpts, statusChangeCallback);

        this.particleSystem = undefined; // only lives while we show the grid animation, owns a number of CustomPrimitives
    }

    static animShowing = 0;

    setVisible (showIt) {
        if (showIt != this.show) {
            if (showIt) {
                AnimField.animShowing++;
                if (!this.particleSystem) {
                    this.loadParticleSystemFromUrl(); // async
                } else {
                    this.particleSystem.forEachPrimitive( p=> p.show = true);
                    odinCesium.setRequestRenderMode(false);

                }
                this.setStatus( WindFieldStatus.SHOWING);

            } else {
                AnimField.animShowing--;
                if (this.particleSystem) {
                    // TODO - do we have to stop rendering first ?
                    this.particleSystem.forEachPrimitive( p=> p.show = false);
                    if (AnimField.animShowing == 0) {
                        odinCesium.setRequestRenderMode(true);
                        odinCesium.requestRender();
                    }
                    this.setStatus( WindFieldStatus.LOADED);
                }
            }
            this.show = showIt;
        }
    }

    startViewChange() {
        if (this.isShowing()) {
            this.particleSystem.forEachPrimitive( p=> p.show = false);
            odinCesium.requestRender();
        }
    }

    endViewChange() {
        if (this.isShowing()) {
            this.particleSystem.applyViewerParameters(viewerParameters);
            this.particleSystem.forEachPrimitive( p=> p.show = true);
            odinCesium.requestRender();
        }
    }

    async loadParticleSystemFromUrl() {

        let nx, x0, dx, ny, y0, dy; // grid bounds and cell size
        let hs, us, vs, ws; // the data arrays
        let hMin = 1e6, uMin = 1e6, vMin = 1e6, wMin = 1e6;
        let hMax = -1e6, uMax = -1e6, vMax = -1e6, wMax = -1e6;
    
        let i = 0;
    
        function procLine (line) {
            if (i > 1) { // grid data line
                let values = util.parseCsvValues(line);
                if (values.length == 5) {
                    const h = values[0];
                    const u = values[1];
                    const v = values[2];
                    const w = values[3];
    
                    if (h < hMin) hMin = h;
                    if (h > hMax) hMax = h;
                    if (u < uMin) uMin = u;
                    if (u > uMax) uMax = u;
                    if (v < vMin) vMin = v;
                    if (v > vMax) vMax = v;
                    if (w < wMin) wMin = w;
                    if (w > wMax) wMax = w;
    
                    const j = i-2;
                    hs[j] = h;
                    us[j] = u;
                    vs[j] = v;
                    ws[j] = w;
                }
            } else if (i > 0) { // ignore header line
            } else { // prefix comment line with grid bounds
                let m = line.match(csvGridPrefixLine);
                if (m && m.length == 7) {
                    nx = parseInt(m[1]);
                    x0 = Number(m[2]);
                    dx = Number(m[3]);
                    ny = parseInt(m[4]);
                    y0 = Number(m[5]);
                    dy = Number(m[6]);
    
                    let len = (nx * ny);
                    hs = new Float32Array(len);
                    us = new Float32Array(len);
                    vs = new Float32Array(len);
                    ws = new Float32Array(len);
                }
            }
            i++;
        };
    
        function axisData (nv,v0,dv) {
            let a = new Float32Array(nv);
            for (let i=0, v=v0; i<nv; i++, v += dv) { a[i] = v; }
            let vMin, vMax;
            if (dv < 0) {
                vMin = a[nv-1];
                vMax = a[0];
            } else {
                vMin = a[0];
                vMax = a[nv-1];
            }
        
            return { array: a, min: vMin, max: vMax };
        }
    
        let url = `./wind-data/${this.urlBase}.csv`;

        this.setStatus( WindFieldStatus.LOADING);
        await util.forEachTextLine( url, procLine);

        console.log("loaded ", i-2, " grid points from ", url);
    
        let data = {
            dimensions: { lon: nx, lat: ny, lev:1 },
            lon: axisData(nx, x0 < 0 ? 360 + x0 : x0, dx),  // normalize to 0..360
            lat: axisData(ny, y0, dy),
            lev: { array: new Float32Array([1]), min: 1, max: 1 },
            H: { array: hs, min: hMin, max: hMax },
            U: { array: us, min: uMin, max: uMax },
            V: { array: vs, min: vMin, max: vMax },
            W: { array: ws, min: wMin, max: wMax }
        };
        //console.log("@@ data:", data);
    
        this.particleSystem = new ParticleSystem( odinCesium.viewer.scene.context, data, this.renderOpts, viewerParameters);
        this.particleSystem.forEachPrimitive( p=> odinCesium.addPrimitive(p));
        odinCesium.setRequestRenderMode(false);
    }

    renderChanged() {
        if (this.particleSystem) {
            this.particleSystem.applyUserInput(this.renderOpts);
        }
    }

    // TODO - this needs a general suspend/resume mode, also for pan, zoom etc.
    updatePrimitives() {
        if (this.isAnimShowing()) { 
            this.showAnim(false);
            this.particleSystem.canvasResize(odinCesium.viewer.scene.context);
            this.showAnim(true);
        }
    }

    updateDisplayPanel() {
        let r = this.renderOpts;

        ui.setSliderValue( ui.getSlider("wind.anim.particles"), r.particlesTextureSize);
        ui.setSliderValue( ui.getSlider("wind.anim.height"), r.particleHeight);
        ui.setSliderValue( ui.getSlider("wind.anim.fade_opacity"), r.fadeOpacity);
        ui.setSliderValue( ui.getSlider("wind.anim.drop"), r.dropRate);
        ui.setSliderValue( ui.getSlider("wind.anim.drop_bump"), r.dropRateBump);
        ui.setSliderValue( ui.getSlider("wind.anim.speed"), r.speedFactor);
        ui.setSliderValue( ui.getSlider("wind.anim.width"), r.lineWidth);

        ui.setField( ui.getField("wind.anim.color"), r.color.toCssHexString());
    }
}

///a wind field with a vector primitive visualization
export class VectorField extends WindFieldVisualization {

    constructor (urlBase, defaultRenderOpts, statusChangeCallback=null) {
        super( DisplayType.DISPLAY_VECTOR, urlBase, defaultRenderOpts, statusChangeCallback);

        this.pointPrimitive = undefined; // Cesium.Primitive instantiated when showing the static vector field
        this.linePrimitive = undefined;
        this.vectors = undefined;
    }

    setVisible (showIt) {
        if (showIt != this.show) {
            this.show = showIt;
            if (showIt) {
                if (!this.pointPrimitive) {
                    this.loadVectorsFromUrl(); // this is async, it will set vectorPrimitives when done
                } else {
                    odinCesium.showPrimitive(this.pointPrimitive, true);
                    odinCesium.showPrimitive(this.linePrimitive, true);
                    odinCesium.requestRender();
                }
                this.setStatus( WindFieldStatus.SHOWING);

            } else {
                if (this.pointPrimitive) {
                    odinCesium.showPrimitive(this.pointPrimitive, false);
                    odinCesium.showPrimitive(this.linePrimitive, false);
                    odinCesium.requestRender();
                    this.setStatus( WindFieldStatus.LOADED);
                }
            }
        }
    }

    async loadVectorsFromUrl() {
        let points = new Cesium.PointPrimitiveCollection();
        let vectors = []; // array of GeometryInstances
        let i = 0;
        let j = 0;
        let renderOpts = this.renderOpts;

        let dc = new Cesium.DistanceDisplayConditionGeometryInstanceAttribute(0,50000);
        let vecAttrs = {  distanceDisplayCondition: dc };
        let vecClrs = [renderOpts.color];
        
        function procLine (line) {
            if (i > 1) { // vector line
                let values = util.parseCsvValues(line);
                if (values.length == 7) {
                    let p0 = new Cesium.Cartesian3(values[0],values[1],values[2]);
                    let p1 = new Cesium.Cartesian3(values[3],values[4],values[5]);
    
                    //let spd = values[6];
            
                    let pp = points.add({
                        position: p0,
                        pixelSize: renderOpts.pointSize,
                        color: renderOpts.color
                    });
                    pp.distanceDisplayCondition = new Cesium.DistanceDisplayCondition(0,150000);
                    // pp.scaleByDistance = new Cesium.NearFarScalar(1.5e2, 15, 8.0e6, 0.0);
                                
                    vectors[j++] = new Cesium.GeometryInstance({
                        geometry: new Cesium.PolylineGeometry({
                            positions: [p0,p1],
                            colors: vecClrs,
                            width: renderOpts.strokeWidth,
                        }),
                        attributes:  vecAttrs
                    });
                }
            } else if (i > 0) { // header line (ignore)
            } else { // prefix comment line "# columns: X, rows: Y"
                let m = line.match(csvVectorPrefixLine);
                if (m) {
                    let len = parseInt(m[1]);
                    vectors = Array(len);
                }
            }
            i++;
        };
    
        let url = `./wind-data/${this.urlBase}_vector.csv`;

        this.setStatus( WindFieldStatus.LOADING);
        await util.forEachTextLine( url, procLine);
        console.log("loaded ", i-2, " vectors from ", url);

        this.vectors = vectors;
        this.pointPrimitive = points;
        this.linePrimitive = this.createVectorPrimitive(vectors);

        odinCesium.addPrimitive(this.pointPrimitive);
        if (this.linePrimitive) odinCesium.addPrimitive(this.linePrimitive);
        odinCesium.requestRender();
    }

    createVectorPrimitive(vectors) {
        if (this.renderOpts.strokeWidth) {
            return new Cesium.Primitive({
                geometryInstances: vectors,
                appearance: polyLineColorAppearance,
                releaseGeometryInstances: false
            });
        } else {
            return null; // no point creating a primitive if there is nothing to renderOpts
        }
    }

    renderChanged() {
        let renderOpts = this.renderOpts;
        let oldLinePrimitive = this.pointPrimitive;
        if (oldLinePrimitive) {
            let len = oldLinePrimitive.length;
            for (let i=0; i<len; i++) {
                let pt = oldLinePrimitive.get(i);
                pt.color = renderOpts.color;
                pt.pixelSize = renderOpts.pointSize;
            }
        }

        oldLinePrimitive = this.linePrimitive;
        if (oldLinePrimitive) {
            // unfortunately we cannot change display of rendered primitive GeometryInstances - we have to re-create it
            let vectors = this.vectors;
            vectors.forEach( gi=> gi.geometry._colors[0] = renderOpts.color );
            this.linePrimitive = this.createVectorPrimitive(vectors);
            odinCesium.removePrimitive(oldLinePrimitive);
            odinCesium.addPrimitive(this.linePrimitive);
        }

        odinCesium.requestRender();
    }

    updateDisplayPanel() {
        let renderOpts = this.renderOpts;
        ui.setSliderValue("wind.vector.point_size", renderOpts.pointSize);
        ui.setSliderValue("wind.vector.width", renderOpts.strokeWidth);
        ui.setField("wind.vector.color", renderOpts.color.toCssHexString());
    }
}

/// a wind field with a countour (polygon) visualization
export class ContourField extends WindFieldVisualization {

    constructor (urlBase, defaultRenderOpts, statusChangeCallback=null) {
        super( DisplayType.DISPLAY_CONTOUR, urlBase, defaultRenderOpts, statusChangeCallback);

        this.dataSource = undefined;
    }

    setVisible (showIt) {
        if (showIt != this.show) {
            this.show = showIt;
            if (showIt) {
                if (!this.dataSource) {
                    this.loadContoursFromUrl(); // this is async, it will set vectorPrimitives when done
                } else {
                    this.dataSource.show = true;
                    odinCesium.requestRender();
                }
                this.setStatus( WindFieldStatus.SHOWING);

            } else {
                if (this.dataSource) {
                    this.dataSource.show = false;
                    odinCesium.requestRender();
                    this.setStatus( WindFieldStatus.LOADED);
                }
            }
        }
    }

    async loadContoursFromUrl() {
        let url = `./wind-data/${this.urlBase}.json`;

        let geoJsonRenderOpts = this.getGeoJsonRenderOpts;
        let response = await fetch( url);
        let data = await response.json();
        console.log("loaded contour from", url);

        Cesium.GeoJsonDataSource.load(data, geoJsonRenderOpts).then(  // TODO - does this support streams? 
            ds => {
                this.dataSource = ds;
                this.postProcessDataSource();

                odinCesium.addDataSource(ds);
                odinCesium.requestRender();
                //setTimeout( () => odinCesium.requestRender(), 300); // ??
            }
        );
    }

    getGeoJsonRenderOpts() {
        return { 
            stroke: this.renderOpts.strokeColor, 
            strokeWidth: this.renderOpts.strokeWidth, 
            fill: this.renderOpts.fillColors[0]
        };
    }

    postProcessDataSource() {
        let entities = this.dataSource.entities.values;
        let renderOpts = this.renderOpts;

        for (const e of entities) {
            let props = e.properties;
            if (props) {
                let spd = this.getPropValue(props, "spd");
                if (spd) {
                    let i = Math.max(0, Math.min( Math.trunc(spd / 5), renderOpts.fillColors.length-1)); // spd < 0 ??
                    e.polygon.material = renderOpts.fillColors[i];
                }
            }
        }
    }

    getPropValue(props,key) {
        let p = props[key];
        return p ? p._value : undefined;
    }

    updateDisplayPanel() {
    }
}