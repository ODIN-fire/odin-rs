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
const polyLineColorAppearance = new Cesium.PolylineColorAppearance();


/// the supported display types (values can be used as property names by callers)
export const AnimDisplay = "animation";
export const VectorDisplay = "vector";
export const ContourDisplay = "contour";

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
    if (!viewRectangle) viewRectangle = Cesium.Rectangle.MAX_VALUE; // globe not in view (e.g. looking at the sky)
    var lonLatRange = windUtils.viewRectangleToLonLatRange(viewRectangle);

    viewerParameters.lonRange.x = lonLatRange.lon.min;
    viewerParameters.lonRange.y = lonLatRange.lon.max;
    viewerParameters.latRange.x = lonLatRange.lat.min;
    viewerParameters.latRange.y = lonLatRange.lat.max;

    var pixelSize = viewer.camera.getPixelSize(
        new Cesium.BoundingSphere( getViewTarget( viewer), 0.0),
        viewer.scene.drawingBufferWidth,
        viewer.scene.drawingBufferHeight
    );

    if (pixelSize > 0) {
        viewerParameters.pixelSize = pixelSize;
    }
}

// the point on the terrain we compute the pixel size (meters per drawing buffer pixel) for. The particle shaders use
// this to scale lineWidth and speedFactor so that they appear the same on screen regardless of view distance.
// We use the terrain point at the screen center, or the terrain point below the camera if the center does not hit the
// globe (e.g. for views towards the horizon).
// NOTE - we cannot use a globe bounding sphere here since its distance is measured to the sphere surface, which would
// be ~64km below ground for a 0.99 earth radius sphere (i.e. pixelSize is way too large at low view heights)
function getViewTarget (viewer) {
    let scene = viewer.scene;
    let camera = viewer.camera;

    let center = new Cesium.Cartesian2( scene.canvas.clientWidth/2, scene.canvas.clientHeight/2);
    let ray = camera.getPickRay( center);
    let target = ray ? scene.globe.pick( ray, scene) : undefined;

    if (!target) {
        let carto = Cesium.Cartographic.clone( camera.positionCartographic);
        let h = scene.globe.getHeight( carto); // undefined if terrain is not loaded for this position
        carto.height = (h !== undefined) ? h : 0.0;
        target = Cesium.Cartographic.toCartesian( carto);
    }

    return target;
}

/* #region view change handling *****************************************************************************/

// wind field visualizations have to be hidden while the view changes (pan, zoom etc.). Since this applies to all
// WindFieldVisualization instances regardless of which module created them we track showing fields here
const showingFields = new Set(); // WindFieldVisualizations that currently have SHOWING status
const heldFields = new Set();    // WindFieldVisualizations we hid during a view change
const animFields = new Set();    // AnimFields that have a particle system (which depends on canvas size)

odinCesium.viewerReadyPromise.then( (viewer) => { // listeners require an initialized Cesium.Viewer
    viewer.camera.moveStart.addEventListener( startViewChange);
    viewer.camera.moveEnd.addEventListener( endViewChange);

    // particle system framebuffers depend on the canvas size. We process resizes before the next render
    let resized = false;
    window.addEventListener( "resize", () => { resized = true; });
    viewer.scene.preRender.addEventListener( () => {
        if (resized) {
            resized = false;
            updateViewerParameters();
            for (let af of animFields) af.canvasResize();
        }
    });
});

function startViewChange() {
    for (let wf of [...showingFields]) {
        heldFields.add( wf);
        wf.setVisible( false);
    }
}

// note that AnimFields apply the new viewer parameters when they are shown again
function endViewChange() {
    updateViewerParameters();
    for (let wf of [...heldFields]) {
        wf.setVisible( true);
    }
    heldFields.clear();
}

/* #endregion view change handling */

//--- types

/// the common base for all wind field visualization types
/// `url` is the complete URL of the respective data file (grid CSV, vector CSV or contour GeoJSON) since this depends on
/// the server routes of the module that uses the wind field
export class WindFieldVisualization {
    constructor (url, defaultRenderOpts, statusChangeCallback=null) {
        this.statusChangeCallback = statusChangeCallback;
        this.url = url;
        this.status = WindFieldStatus.REMOTE;
        this.show = false;
        this.loading = false; // set by subclasses while they load their data
        this.renderOpts = {...defaultRenderOpts};
    }

    dataUrl() {
        return this.url;
    }

    setStatus (newStatus) {
        this.status = newStatus;

        if (Object.is( newStatus, WindFieldStatus.SHOWING)) {
            showingFields.add( this);
        } else {
            showingFields.delete( this);
            if (Object.is( newStatus, WindFieldStatus.REMOTE)) heldFields.delete( this); // cleared during view change
        }

        if (this.statusChangeCallback) this.statusChangeCallback( this);
    }

    isShowing () {
        return Object.is( this.status, WindFieldStatus.SHOWING);
    }

    // the status after hiding, depending on whether we have loaded data, are still loading or neither
    hiddenStatus (loadedData) {
        if (loadedData) return WindFieldStatus.LOADED;
        else if (this.loading) return WindFieldStatus.LOADING;
        else return WindFieldStatus.REMOTE;
    }

    setRenderOpts (newRenderOpts) {
        this.renderOpts = {...newRenderOpts};
        this.renderChanged();
    }

    spdColor (spd) {
        let renderOpts = this.renderOpts;

        if (renderOpts.colors) {
            // spd is in m/sec
            let i = Math.floor( spd / 2.2352); // renderOpts colors are in 5mph steps
            if (i >= renderOpts.colors.length) {
                i = renderOpts.colors.length-1;
            } 
            return renderOpts.colors[i];
        } else {
            return null;
        }
    }

    //--- those are called by our owner and have to be overridden
    setVisible (showIt) {} // set visibility (but keep primitives)
    clear() {} // set invisible and release all resources

    renderChanged() {}
}

/// a wind field with a custom particle system visualization
export class AnimField extends WindFieldVisualization {

    constructor (url, defaultRenderOpts, statusChangeCallback=null) {
        super( url, defaultRenderOpts, statusChangeCallback);

        this.particleSystem = undefined; // only lives while we show the grid animation, owns a number of CustomPrimitives
        this.loading = false; // set while we load the grid data
    }

    static animShowing = 0; // number of showing AnimFields (we need continuous rendering if > 0)

    setVisible (showIt) {
        if (showIt != this.show) {
            this.show = showIt;

            if (showIt) {
                AnimField.animShowing++;
                if (this.particleSystem) {
                    this.particleSystem.applyViewerParameters( viewerParameters); // view might have changed while we were hidden
                    this.showPrimitives( true);
                    odinCesium.setRequestRenderMode(false);
                } else if (!this.loading) {
                    this.loadParticleSystemFromUrl(); // async - shows primitives when done (if we are still visible)
                }
                this.setStatus( WindFieldStatus.SHOWING);

            } else {
                if (AnimField.animShowing > 0) AnimField.animShowing--;
                if (AnimField.animShowing == 0) odinCesium.setRequestRenderMode(true);
                if (this.particleSystem) this.showPrimitives( false);
                this.setStatus( this.hiddenStatus( this.particleSystem));
                odinCesium.requestRender();
            }
        }
    }

    showPrimitives (showIt) {
        this.particleSystem.forEachPrimitive( p=> p.show = showIt);
        odinCesium.requestRender();
    }

    clear () {
        this.setVisible(false);
        this.releaseParticleSystem();
        this.setStatus( WindFieldStatus.REMOTE);
    }

    releaseParticleSystem() {
        if (this.particleSystem){
            animFields.delete( this);
            this.particleSystem.forEachPrimitive( p=> odinCesium.removePrimitive(p));
            this.particleSystem.clearFramebuffers();
            this.particleSystem.release();
            this.particleSystem = undefined;
        }
    }

    // the canvas size has changed - we have to re-create the particle system primitives (and replace them in the scene)
    canvasResize() {
        if (this.particleSystem) {
            this.particleSystem.forEachPrimitive( p=> odinCesium.removePrimitive(p));
            this.particleSystem.canvasResize( odinCesium.viewer.scene.context);
            this.particleSystem.forEachPrimitive( p=> { p.show = this.show; odinCesium.addPrimitive(p); });
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
    
        let url = this.dataUrl();

        this.loading = true;
        this.setStatus( WindFieldStatus.LOADING);
        await util.forEachTextLine( url, procLine);
        this.loading = false;

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

        updateViewerParameters(); // make sure we have a valid pixelSize (normally only set after the first view change)
        this.particleSystem = new ParticleSystem( odinCesium.viewer.scene.context, data, this.renderOpts, viewerParameters);
        this.particleSystem.forEachPrimitive( p=> { p.show = this.show; odinCesium.addPrimitive(p); }); // we might have been hidden while loading
        animFields.add( this);

        if (this.show) {
            odinCesium.setRequestRenderMode(false);
            this.setStatus( WindFieldStatus.SHOWING);
        } else {
            this.setStatus( WindFieldStatus.LOADED);
        }
        odinCesium.requestRender();
    }

    renderChanged() {
        if (this.particleSystem) {
            this.particleSystem.applyUserInput(this.renderOpts); // preserves primitive visibility
            odinCesium.requestRender();
        }
    }
}

///a wind field with a vector primitive visualization
export class VectorField extends WindFieldVisualization {

    constructor (url, defaultRenderOpts, statusChangeCallback=null) {
        super( url, defaultRenderOpts, statusChangeCallback);

        this.pointPrimitive = undefined; // Cesium.PointPrimitiveCollection instantiated when showing the static vector field
        this.linePrimitive = undefined;  // Cesium.Primitive with the vector polylines (undefined if lineWidth is 0)
        this.vectors = undefined;        // array of {p0,p1,spd} objects we need to re-create the line primitive
        this.loading = false;
    }

    setVisible (showIt) {
        if (showIt != this.show) {
            this.show = showIt;
            if (showIt) {
                if (this.pointPrimitive) {
                    this.showPrimitives( true);
                } else if (!this.loading) {
                    this.loadVectorsFromUrl(); // async - shows primitives when done (if we are still visible)
                }
                this.setStatus( WindFieldStatus.SHOWING);

            } else {
                if (this.pointPrimitive) this.showPrimitives( false);
                this.setStatus( this.hiddenStatus( this.pointPrimitive));
            }
        }
    }

    showPrimitives (showIt) {
        this.pointPrimitive.show = showIt;
        if (this.linePrimitive) this.linePrimitive.show = showIt;
        odinCesium.requestRender();
    }

    clear () {
        this.setVisible(false);
        if (this.pointPrimitive) odinCesium.removePrimitive( this.pointPrimitive);
        if (this.linePrimitive) odinCesium.removePrimitive( this.linePrimitive);
        this.pointPrimitive = undefined;
        this.linePrimitive = undefined;
        this.vectors = undefined;
        this.setStatus( WindFieldStatus.REMOTE);
    }

    async loadVectorsFromUrl() {
        let points = new Cesium.PointPrimitiveCollection();
        let vectors = [];
        let i = 0;

        let url = this.dataUrl();

        this.loading = true;
        this.setStatus( WindFieldStatus.LOADING);
        await util.forEachTextLine( url, (line)=> {
            this.procLine( line, i, points, vectors);
            i++;
        });
        this.loading = false;
        console.log("loaded ", i-2, " vectors from ", url);

        this.vectors = vectors;
        this.pointPrimitive = points;
        this.linePrimitive = this.createVectorPrimitive();

        odinCesium.addPrimitive(this.pointPrimitive);
        if (this.linePrimitive) odinCesium.addPrimitive(this.linePrimitive);
        this.showPrimitives( this.show); // we might have been hidden while loading

        this.setStatus( this.show ? WindFieldStatus.SHOWING : WindFieldStatus.LOADED);
    }

    procLine (line, i, points, vectors) {
        if (i > 1) { // vector line
            let values = util.parseCsvValues(line);
            if (values.length == 7) { // 2 cartesian3 + spd
                let p0 = new Cesium.Cartesian3(values[0],values[1],values[2]);
                let p1 = new Cesium.Cartesian3(values[3],values[4],values[5]);
                let spd = values[6];

                let pp = points.add({
                    position: p0,
                    pixelSize: this.renderOpts.pointSize,
                    color: this.spdColor( spd)
                });
                pp.distanceDisplayCondition = new Cesium.DistanceDisplayCondition(0,150000);
                // pp.scaleByDistance = new Cesium.NearFarScalar(1.5e2, 15, 8.0e6, 0.0);

                vectors.push( {p0, p1, spd}); // point index == vector index
            }
        } // ignore prefix comment line "# length:N_VECS" and header line
    }

    // since we cannot change colors or widths of GeometryInstances in a rendered Primitive we have to re-create it
    createVectorPrimitive() {
        let renderOpts = this.renderOpts;
        if (renderOpts.lineWidth > 0) {
            let attributes = { distanceDisplayCondition: new Cesium.DistanceDisplayConditionGeometryInstanceAttribute(0,50000) };
            let geometryInstances = this.vectors.map( v=> new Cesium.GeometryInstance({
                geometry: new Cesium.PolylineGeometry({
                    positions: [v.p0, v.p1],
                    colors: [this.spdColor( v.spd)],
                    width: renderOpts.lineWidth,
                }),
                attributes
            }));

            return new Cesium.Primitive({
                geometryInstances,
                appearance: polyLineColorAppearance,
                releaseGeometryInstances: true
            });
        } else {
            return undefined; // no point creating a primitive if there is nothing to render
        }
    }

    renderChanged() {
        if (!this.pointPrimitive) return; // not loaded yet - renderOpts are used when loading

        let renderOpts = this.renderOpts;
        let points = this.pointPrimitive;
        for (let i=0; i<points.length; i++) {
            let pt = points.get(i);
            pt.color = this.spdColor( this.vectors[i].spd);
            pt.pixelSize = renderOpts.pointSize;
        }

        if (this.linePrimitive) odinCesium.removePrimitive( this.linePrimitive); // this destroys it
        this.linePrimitive = this.createVectorPrimitive();
        if (this.linePrimitive) {
            this.linePrimitive.show = this.show;
            odinCesium.addPrimitive(this.linePrimitive);
        }

        odinCesium.requestRender();
    }
}

/// a wind field with a countour (polygon) visualization
export class ContourField extends WindFieldVisualization {

    constructor (url, defaultRenderOpts, statusChangeCallback=null) {
        super( url, defaultRenderOpts, statusChangeCallback);

        this.dataSource = undefined;
        this.loading = false;
    }

    setVisible (showIt) {
        if (showIt != this.show) {
            this.show = showIt;
            if (showIt) {
                if (this.dataSource) {
                    this.dataSource.show = true;
                    odinCesium.requestRender();
                } else if (!this.loading) {
                    this.loadContoursFromUrl(); // async - shows dataSource when done (if we are still visible)
                }
                this.setStatus( WindFieldStatus.SHOWING);

            } else {
                if (this.dataSource) {
                    this.dataSource.show = false;
                    odinCesium.requestRender();
                }
                this.setStatus( this.hiddenStatus( this.dataSource));
            }
        }
    }

    clear () {
        this.setVisible(false);
        if (this.dataSource) odinCesium.removeDataSource( this.dataSource);
        this.dataSource = undefined;
        this.setStatus( WindFieldStatus.REMOTE);
    }

    async loadContoursFromUrl() {
        let url = this.dataUrl();

        let geoJsonRenderOpts = { 
            material: this.renderOpts.colors[0], 
            strokeWidth: this.renderOpts.lineWidth, 
            //fill: this.renderOpts.colors[0]
        };

        this.loading = true;
        this.setStatus( WindFieldStatus.LOADING);
        let response = await fetch( url);
        let data = await response.json();

        let ds = await Cesium.GeoJsonDataSource.load(data, geoJsonRenderOpts); // TODO - does this support streams?
        this.loading = false;
        this.dataSource = ds;
        this.postProcessDataSource();
        console.log("loaded contour from", url);

        ds.show = this.show; // we might have been hidden while loading
        odinCesium.addDataSource(ds);
        this.setStatus( this.show ? WindFieldStatus.SHOWING : WindFieldStatus.LOADED);

        odinCesium.requestRender();
        setTimeout( () => odinCesium.requestRender(), 300); // ??
    }

    postProcessDataSource() {
        let entities = this.dataSource.entities.values;
        let renderOpts = this.renderOpts;

        for (const e of entities) {
            let props = e.properties;
            if (props) {
                this.setPolygonAttrs( e, this.getPropValue(props, "minSpeed"), this.getPropValue(props, "maxSpeed"));
                //this.setPolylineAttrs( e, this.getPropValue(props, "windSpeed"));
            }
        }
    }

    renderChanged() {
        if (!this.dataSource) return; // not loaded yet - renderOpts are used when loading
        let entities = this.dataSource.entities.values;
        let renderOpts = this.renderOpts;

        for (const e of entities) {
            let props = e.properties;
            if (props) {
                this.setPolygonAttrs( e, this.getPropValue(props, "minSpeed"), this.getPropValue(props, "maxSpeed"));
                //this.setPolylineAttrs( e, this.getPropValue(props, "windSpeed"));
            }
        }

        odinCesium.requestRender();
    }

    setPolygonAttrs (entity, minSpeed, maxSpeed) {
        let renderOpts = this.renderOpts;
        let clrIdx = Math.floor( (maxSpeed + minSpeed)/2 / 2.2352);
        let alpha = renderOpts.alpha * (clrIdx +1) / renderOpts.colors.length;

        entity.polygon.material = renderOpts.colors[clrIdx].withAlpha( alpha);
        entity.polygon.height = 0;
        entity.polygon.outline = false;
        entity.polygon.heightReference = Cesium.HeightReference.CLAMP_TO_GROUND;
    }

    setPolylineAttrs (entity, windSpeed) {
        let renderOpts = this.renderOpts;
        let clrIdx = Math.floor( (windSpeed-2.2352) / 2.2352);

        entity.polyline.width = renderOpts.lineWidth;
        entity.polyline.material = renderOpts.colors[clrIdx];
        entity.polyline.clampToGround = true;
    }

    getPropValue(props,key) {
        let p = props[key];
        return p ? p._value : undefined;
    }
}

/* #region render controls **********************************************************************************/

/// the UI panels and change callbacks to control wind field rendering (animation, vector and contour display).
/// All display types share one set of wind speed colors (in 5mph steps) and one line width (particle trails, vector lines,
/// contour outlines). Each module that displays wind fields creates its own instance, with
///  - `idPrefix`: unique prefix for panel element ids (e.g. "wind" -> "wind.anim.particles")
///  - `renderConfig`: default render options of the form
///        { colors: [Cesium.Color..], lineWidth: N,
///          anim: { particlesTextureSize, speedFactor, particleHeight, fadeOpacity, dropRate, dropRateBump, color, colorBySpeed },
///          vector: { pointSize },
///          contour: { alpha } }
///  - `renderChangeAction`: function( displayType) that is called with AnimDisplay, VectorDisplay or ContourDisplay when the
///    user has changed render options that affect the respective display type. This is how the caller updates its own
///    WindFieldVisualizations, e.g. by calling `setRenderOpts( controls.renderOpts( displayType))` on its shown field.
/// Usage: add the panel returned by `displayPanel()` to the caller window, then call `initialize()` once the window
/// is created. Use `renderOpts(displayType)` (or the `animRender`,
/// `vectorRender` and `contourRender` getters) as render options when creating new WindFieldVisualizations
export class WindFieldRenderControls {
    constructor (idPrefix, renderConfig, renderChangeAction) {
        this.idPrefix = idPrefix;
        this.renderChangeAction = renderChangeAction;

        // copy so that we don't modify the config (which might be shared)
        this.render = {
            colors: [...renderConfig.colors],
            lineWidth: renderConfig.lineWidth,
            anim: {...renderConfig.anim},
            vector: {...renderConfig.vector},
            contour: {...renderConfig.contour}
        };

        this.pendingAnimChange = false; // we have to delay particleSystem updates while user moves the slider
    }

    //--- the (flat) render options for the WindFieldVisualization types

    get animRender() {
        let r = this.render;
        let n = r.anim.particlesTextureSize;
        return { ...r.anim, maxParticles: n*n, lineWidth: r.lineWidth, colors: r.colors };
    }

    get vectorRender() {
        let r = this.render;
        return { ...r.vector, lineWidth: r.lineWidth, colors: r.colors };
    }

    get contourRender() {
        let r = this.render;
        return { ...r.contour, lineWidth: r.lineWidth, colors: r.colors };
    }

    renderOpts (displayType) {
        switch (displayType) {
            case AnimDisplay: return this.animRender;
            case VectorDisplay: return this.vectorRender;
            case ContourDisplay: return this.contourRender;
            default: return undefined;
        }
    }

    id (suffix) { return `${this.idPrefix}.${suffix}`; }

    //--- panel creation (to be added to the caller window)

    // a single panel with all render options. Shared options (line width, speed colors) have no label prefix,
    // options that only apply to one display type are prefixed with it. Color settings come first since the speed
    // colors also serve as a legend and should be visible when the panel is expanded
    displayPanel (title="wind display", isExpanded=false) {
        return ui.Panel( title, isExpanded)(
            ...this.colorRows( (e)=>this.colorChanged(e)),
            ui.RowContainer()(
                ui.CheckBox("anim speed colors", (e)=>this.animColorBySpeedChanged(e), this.id("anim.color_by_speed"), this.render.anim.colorBySpeed),
                ui.HorizontalSpacer(1),
                ui.ColorField("anim color", this.id("anim.color"), true, (e)=>this.animColorChanged(e), "2rem")
            ),

            ui.ColumnContainer("align_right")(
                ui.Slider("line width", this.id("line_width"), (e)=>this.lineWidthChanged(e)),

                ui.Slider("anim particles", this.id("anim.particles"), (e)=>this.animParticlesChanged(e)),
                ui.Slider("anim extra height", this.id("anim.height"), (e)=>this.animHeightChanged(e)),
                ui.Slider("anim fade opacity", this.id("anim.fade_opacity"), (e)=>this.animFadeOpacityChanged(e)),
                ui.Slider("anim drop", this.id("anim.drop"), (e)=>this.animDropRateChanged(e)),
                ui.Slider("anim drop bump", this.id("anim.drop_bump"), (e)=>this.animDropRateBumpChanged(e)),
                ui.Slider("anim speed factor", this.id("anim.speed"), (e)=>this.animSpeedChanged(e)),

                ui.Slider("vector point size", this.id("vector.point_size"), (e)=>this.vectorPointSizeChanged(e)),
                ui.Slider("contour fill alpha", this.id("contour.alpha"), (e)=>this.contourAlphaChanged(e)),
            )
        );
    }

    // the wind speed color fields (in 5mph steps) as a grid of narrow fields
    colorRows (callback, colorsPerRow=3) {
        let colors = this.render.colors;
        let rows = [];
        let fields = [];

        for (let i=0; i<colors.length; i++) {
            let spd = i*5;
            let lbl = (i < colors.length-1) ? `${spd}-${spd+5}mph` : `>${spd}mph`;
            fields.push( ui.ColorField( lbl, this.id(`color${i}`), true, callback, "2rem", "5rem"));

            if (fields.length == colorsPerRow || i == colors.length-1) {
                rows.push( ui.RowContainer()( ...fields));
                fields = [];
            }
        }
        return rows;
    }

    //--- panel initialization (requires panels to be created)

    initialize() {
        this.initCommonControls();
        this.initAnimControls();
        this.initVectorControls();
        this.initContourControls();
    }

    initCommonControls() {
        let r = this.render;

        let e = ui.getSlider( this.id("line_width"));
        ui.setSliderRange(e, 0, 5.0, 0.1, util.f_1);
        ui.setSliderValue(e, r.lineWidth);

        for (let i=0; i<r.colors.length; i++) {
            let e = ui.getField( this.id(`color${i}`));
            if (e) {
                e._uiIdx = i;
                ui.setField(e, r.colors[i].toCssHexString());
            }
        }
    }

    initAnimControls() {
        let r = this.render.anim;
        let e = undefined;

        e = ui.getSlider( this.id("anim.particles"));
        ui.setSliderRange(e, 0, 128, 16, util.f_0);
        ui.setSliderValue(e, r.particlesTextureSize);

        e = ui.getSlider( this.id("anim.height"));
        ui.setSliderRange(e, 0, 10000, 500, util.f_0);
        ui.setSliderValue(e, r.particleHeight);

        e = ui.getSlider( this.id("anim.fade_opacity"));
        ui.setSliderRange(e, 0.8, 1.0, 0.01, util.f_2);
        ui.setSliderValue(e, r.fadeOpacity);

        e = ui.getSlider( this.id("anim.drop"));
        ui.setSliderRange(e, 0.0, 0.01, 0.001, util.f_3);
        ui.setSliderValue(e, r.dropRate);

        e = ui.getSlider( this.id("anim.drop_bump"));
        ui.setSliderRange(e, 0.0, 0.05, 0.005, util.f_3);
        ui.setSliderValue(e, r.dropRateBump);

        e = ui.getSlider( this.id("anim.speed"));
        ui.setSliderRange(e, 0.0, 0.3, 0.02, util.f_2);
        ui.setSliderValue(e, r.speedFactor);

        ui.setCheckBox( this.id("anim.color_by_speed"), r.colorBySpeed);

        e = ui.getField( this.id("anim.color"));
        ui.setField(e, r.color.toCssHexString());
    }

    initVectorControls() {
        let e = ui.getSlider( this.id("vector.point_size"));
        ui.setSliderRange(e, 0, 8, 0.5, util.f_1);
        ui.setSliderValue(e, this.render.vector.pointSize);
    }

    initContourControls() {
        let e = ui.getSlider( this.id("contour.alpha"));
        ui.setSliderRange(e, 0, 1.0, 0.1, util.f_1);
        ui.setSliderValue(e, this.render.contour.alpha);
    }

    //--- common callbacks (affect all display types)

    lineWidthChanged (event) {
        this.render.lineWidth = ui.getSliderValue(event.target);
        this.allRenderChanged();
    }

    colorChanged (event) {
        let e = event.target;
        let clr = Cesium.Color.fromCssColorString( e.value);
        let idx = e._uiIdx;
        if (clr && idx >= 0 && idx < this.render.colors.length) {
            this.render.colors[idx] = clr;
            this.allRenderChanged();
        }
    }

    allRenderChanged() {
        this.triggerAnimRenderChange(true);
        this.renderChangeAction( VectorDisplay);
        this.renderChangeAction( ContourDisplay);
    }

    //--- anim display callbacks

    // particle system updates are expensive - we wait until the user has stopped moving the slider
    triggerAnimRenderChange (newInput) {
        if (newInput) this.pendingAnimChange = true;

        setTimeout(() => {
            if (this.pendingAnimChange) {
                this.pendingAnimChange = false;
                this.triggerAnimRenderChange(false);
            } else {
                this.renderChangeAction( AnimDisplay);
            }
        }, 500);
    }

    animParticlesChanged (event) {
        this.render.anim.particlesTextureSize = ui.getSliderValue(event.target);
        this.triggerAnimRenderChange(true);
    }

    animFadeOpacityChanged (event) {
        this.render.anim.fadeOpacity = ui.getSliderValue(event.target);
        this.triggerAnimRenderChange(true);
    }

    animSpeedChanged (event) {
        this.render.anim.speedFactor = ui.getSliderValue(event.target);
        this.triggerAnimRenderChange(true);
    }

    animHeightChanged (event) {
        this.render.anim.particleHeight = ui.getSliderValue(event.target);
        this.triggerAnimRenderChange(true);
    }

    animDropRateChanged (event) {
        this.render.anim.dropRate = ui.getSliderValue(event.target);
        this.triggerAnimRenderChange(true);
    }

    animDropRateBumpChanged (event) {
        this.render.anim.dropRateBump = ui.getSliderValue(event.target);
        this.triggerAnimRenderChange(true);
    }

    animColorBySpeedChanged (event) {
        this.render.anim.colorBySpeed = ui.isCheckBoxSelected( ui.getCheckBox(event.target));
        this.triggerAnimRenderChange(true);
    }

    animColorChanged (event) {
        let clr = Cesium.Color.fromCssColorString( event.target.value);
        if (clr) {
            this.render.anim.color = clr;
            this.triggerAnimRenderChange(true);
        }
    }

    //--- vector display callbacks

    vectorPointSizeChanged (event) {
        this.render.vector.pointSize = ui.getSliderValue(event.target);
        this.renderChangeAction( VectorDisplay);
    }

    //--- contour display callbacks

    contourAlphaChanged (event) {
        this.render.contour.alpha = ui.getSliderValue(event.target);
        this.renderChangeAction( ContourDisplay);
    }
}

/* #endregion render controls */
