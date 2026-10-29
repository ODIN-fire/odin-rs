/*
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

/// module for an interactive map scale ruler that can be used to measure distances and headings on the globe.
/// The ruler is drawn on a transparent canvas that is rotated (via CSS transform) around the left end of the ruler bar.
/// Interaction:
///  - drag the middle of the ruler to move it
///  - drag the (transparent) left handle to extend/shrink the ruler along its bar (the right end stays fixed)
///  - drag the (transparent) right handle to rotate the ruler around its left end in 5 degree increments
///  - double click the middle of the ruler to reset its position, length and orientation
/// The tick resolution is re-computed from the current view whenever the camera stops moving
///
/// If the view is tilted (not looking straight down) the ruler switches to tilt mode: it is forced to be horizontal
/// (rotation is disabled) and represents a fixed ground distance, i.e. its pixel length changes with its y position
/// according to the ground resolution of the respective screen row (at the horizontal center of the view). For flat
/// ground this resolution is uniform along a screen row, i.e. the ruler measures horizontal distances in tilted views.
/// The ruler is shown dashed and cannot be moved above the horizon in tilt mode. Returning to an orthogonal view
/// does a full reset of the ruler

import { config } from "./odin_cesium_config.js";
import * as util from "../odin_server/ui_util.js";
import * as odinCesium from "./odin_cesium.js";

// NOTE - no top level await here since this would delay evaluation of all importing modules

const metricScaleUnits = [ 1000000, 500000, 100000, 50000, 10000, 5000, 1000, 500, 100, 50, 10 ]; // in meters
const usMileScaleUnits = [ 1000, 500, 100, 50, 10, 5, 1, 0.25 ];
const usYardScaleUnits = [ 500, 100, 50, 10 ];

const X_MARGIN = 12;        // canvas pixels left and right of the bar, which are also the handle widths
const HANDLE_SLACK = 4;     // handles extend this many pixels into the bar
const ANGLE_STEP = 5;       // rotation increment in degrees
const ANGLE_LABEL_WIDTH = 50; // canvas pixels reserved right of the bar for the heading label (only if rotated)
const INITIAL_ANGLE = 90;   // horizontal (pointing right)
const PICK_DX = 10;         // horizontal pixel distance for ground resolution computation
const HORIZON_GAP = 20;     // minimum pixel distance of tilt mode ruler bar from horizon

//--- ruler state (screen coordinates are CSS pixels relative to the window)
var canvas = undefined;
var scale = undefined;      // { unit, nUnits, unitPx, tickBase, unitLabel } computed from current view
var nUnits = undefined;     // number of displayed units (ticks - 1), either from scale or from user extension
var extLenPx = undefined;   // user set ruler length in pixels (undefined in initial state)
var angle = INITIAL_ANGLE;  // screen angle of bar in degrees (clockwise from up, i.e. 90 is horizontal pointing right)
var anchor = undefined;     // screen position {x,y} of the left end of the bar (rotation center), undefined if not moved yet

var tiltMode = false;       // set if view is not orthogonal
var extLenM = undefined;    // tilt mode: user set ground length in meters (undefined if not extended)
var minY = undefined;       // tilt mode: minimum y of bar (below horizon)

initRuler();

function initRuler() {
    canvas = document.createElement("canvas");
    canvas.classList.add("ui_moveable_canvas", "ui_mapscale");
    canvas.height = config.scale.height;
    canvas.style.transformOrigin = `${X_MARGIN}px ${config.scale.height/2}px`;

    canvas.addEventListener("mousedown", handleMouseDown);
    canvas.addEventListener("mousemove", handleHover);
    canvas.addEventListener("dblclick", handleDblClick);
    window.addEventListener("resize", () => { if (!anchor) drawRuler(); });

    document.body.appendChild(canvas);

    odinCesium.registerMetricChangeHandler( updateScale);
    odinCesium.viewerReadyPromise.then( (viewer) => {
        viewer.camera.moveEnd.addEventListener( updateScale);
        updateScale(); // this also shows the canvas
    });
}

//--- geometry

function barLength() {
    return nUnits * scale.unitPx;
}

function direction() { // unit vector of bar in screen coordinates (y down)
    let a = Cesium.Math.toRadians(angle - 90);
    return { x: Math.cos(a), y: Math.sin(a) };
}

// the left end of the bar. In the initial (not moved) state the ruler is anchored at the lower right window corner
function getAnchor() {
    if (anchor) return anchor;
    let len = scale ? barLength() : 0;
    return {
        x: window.innerWidth - config.scale.right - X_MARGIN - len,
        y: window.innerHeight - config.scale.bottom - config.scale.height/2
    };
}

function getRightEnd() {
    let a = getAnchor();
    let d = direction();
    let len = barLength();
    return { x: a.x + d.x * len, y: a.y + d.y * len };
}

// position along the bar (in pixels from the left end) for a mouse event
function barPosition(event) {
    let a = getAnchor();
    let d = direction();
    return (event.clientX - a.x) * d.x + (event.clientY - a.y) * d.y;
}

function isLeftHandle(pos) {
    return pos < HANDLE_SLACK;
}

function isRightHandle(pos) {
    return !tiltMode && (pos > barLength() - HANDLE_SLACK); // no rotation in tilt mode
}

function isRotated() {
    return angle != INITIAL_ANGLE;
}

// would text that is rotated with the bar be upside down (i.e. does the bar point left)
function isFlipped() {
    return angle > 180;
}

// geographic heading of the bar, assuming a (close to) top down view
function getHeading() {
    let cameraHeading = Cesium.Math.toDegrees( odinCesium.viewer.camera.heading);
    return Math.round( (angle + cameraHeading) % 360);
}

//--- interaction

function handleHover(event) {
    if (!scale) return;
    let pos = barPosition(event);
    if (isLeftHandle(pos)) canvas.style.cursor = "ew-resize";
    else if (isRightHandle(pos)) canvas.style.cursor = "grab";
    else canvas.style.cursor = "move";
}

function handleMouseDown(event) {
    if (!scale || event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();

    let pos = barPosition(event);
    let onMove;

    if (isLeftHandle(pos)) {
        let r = getRightEnd(); // stays fixed while we extend to the left
        onMove = (e) => extendTo( e, r);
    } else if (isRightHandle(pos)) {
        onMove = rotateTo;
    } else {
        let a = getAnchor();
        let dx = event.clientX - a.x;
        let dy = event.clientY - a.y;
        onMove = (e) => {
            anchor = { x: e.clientX - dx, y: e.clientY - dy };
            if (tiltMode) {
                clampAnchor();
                updateRowResolution();
            }
            drawRuler();
        };
    }

    let onUp = () => {
        document.removeEventListener("mousemove", onMove);
        document.removeEventListener("mouseup", onUp);
        if (tiltMode && barLength() > maxBarLength()) { // moved too far down - fall back to default length for this row
            extLenM = undefined;
            updateScale();
        }
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
}

// set the length (in full units) from the mouse position projected onto the bar, keeping the right end r fixed
function extendTo(event, r) {
    event.preventDefault();
    let d = direction();
    let len = (r.x - event.clientX) * d.x + (r.y - event.clientY) * d.y;

    nUnits = Math.max( 1, Math.round( len / scale.unitPx));
    let l = barLength();
    if (tiltMode) extLenM = nUnits * scale.unitM; else extLenPx = l;
    anchor = { x: r.x - d.x * l, y: r.y - d.y * l };
    drawRuler();
}

// rotate around the left end of the bar so that it points towards the mouse position
function rotateTo(event) {
    event.preventDefault();
    let a = getAnchor();
    let deg = Cesium.Math.toDegrees( Math.atan2( event.clientY - a.y, event.clientX - a.x)) + 90;
    deg = Math.round( deg / ANGLE_STEP) * ANGLE_STEP;
    angle = ((deg % 360) + 360) % 360;

    if (!anchor) anchor = a; // don't move the rotation center if the ruler length changes
    drawRuler();
}

function handleDblClick(event) {
    if (!scale) return;
    let pos = barPosition(event);
    if (!isLeftHandle(pos) && !isRightHandle(pos)) {
        event.preventDefault();
        event.stopPropagation();
        resetRuler();
    }
}

export function resetRuler() {
    resetState();
    updateScale();
}

function resetState() {
    anchor = undefined;
    extLenPx = undefined;
    extLenM = undefined;
    angle = INITIAL_ANGLE;
}

//--- tilt mode

// the ruler keeps its position and ground length but is forced to be horizontal
function enterTiltMode() {
    if (extLenPx && scale) extLenM = nUnits * scale.unitM;
    extLenPx = undefined;
    angle = INITIAL_ANGLE;
    tiltMode = true;
}

function exitTiltMode() {
    tiltMode = false;
    minY = undefined;
    resetState();
}

function clampAnchor() {
    if (minY !== undefined) {
        let a = getAnchor(); // don't fix the default position unless we have to
        if (a.y < minY) anchor = { x: a.x, y: minY };
    }
}

function maxBarLength() {
    return window.innerWidth - 2*X_MARGIN;
}

// keep the ground length (units) of the ruler but adapt its pixel length to the ground resolution of the current row
function updateRowResolution() {
    let dPixel = metersPerPixelAt( getAnchor().y);
    if (dPixel) scale.unitPx = scale.unitM / dPixel;
}

// smallest window y at the horizontal center of the view that is below the horizon (undefined if there is none)
function getHorizonY() {
    let h = odinCesium.viewer.scene.canvas.clientHeight;
    if (!hitsEllipsoid(h-1)) return undefined;
    if (hitsEllipsoid(0)) return 0;

    let y0 = 0, y1 = h-1; // y0 misses, y1 hits - the horizon is a single transition from sky (top) to ground (bottom)
    while (y1 - y0 > 1) {
        let y = Math.floor((y0 + y1) / 2);
        if (hitsEllipsoid(y)) y1 = y; else y0 = y;
    }
    return y1;
}

function hitsEllipsoid(y) {
    let camera = odinCesium.viewer.scene.camera;
    let x = Math.round( odinCesium.viewer.scene.canvas.clientWidth / 2);
    return camera.pickEllipsoid( new Cesium.Cartesian2(x, y)) && camera.pickEllipsoid( new Cesium.Cartesian2(x + PICK_DX, y));
}

//--- scale computation

export function updateScale () {
    let isTilted = !odinCesium.isOrthoView();
    if (tiltMode && !isTilted) exitTiltMode();
    else if (!tiltMode && isTilted) enterTiltMode();

    if (tiltMode) {
        let horizonY = getHorizonY();
        minY = (horizonY !== undefined) ? horizonY + HORIZON_GAP : undefined;
        clampAnchor();
    }

    let dPixel = tiltMode ? metersPerPixelAt( getAnchor().y) : centerMetersPerPixel();
    if (dPixel) {
        scale = odinCesium.isMetric ? getMetricScale(dPixel) : getUsScale(dPixel);
        if (tiltMode) {
            nUnits = extLenM ? Math.max( 1, Math.round( extLenM / scale.unitM)) : scale.nUnits;
            if (barLength() > maxBarLength()) { // user set ground length does not fit on screen anymore
                extLenM = undefined;
                nUnits = scale.nUnits;
            }
        } else {
            nUnits = extLenPx ? Math.max( 1, Math.round( extLenPx / scale.unitPx)) : scale.nUnits;
        }
        drawRuler();
        canvas.classList.add("show");
    } else { // no ground below the ruler - nothing to measure
        canvas.classList.remove("show");
    }
}

// ground resolution [m/px] at the horizontal center of the given window row (undefined if the row does not hit the ground)
function metersPerPixelAt (y) {
    let scene = odinCesium.viewer.scene;
    let x = Math.round( scene.canvas.clientWidth / 2);
    let p1 = groundPosition( scene, x, y);
    let p2 = groundPosition( scene, x + PICK_DX, y);
    return (p1 && p2) ? Cesium.Cartesian3.distance( p1, p2) / PICK_DX : undefined;
}

// terrain position for given window coordinates, falling back to the ellipsoid if terrain tiles are not loaded
function groundPosition (scene, x, y) {
    let wp = new Cesium.Cartesian2( x, y);
    let p = scene.globe.pick( scene.camera.getPickRay(wp), scene);
    return p ? p : scene.camera.pickEllipsoid(wp);
}

function centerMetersPerPixel() {
    let dPixel = metersPerPixelAt( Math.round( odinCesium.viewer.scene.canvas.clientHeight / 2));
    if (!dPixel) withMetersPerPixel( (d) => dPixel = d); // fall back to the camera height based computation
    return dPixel;
}

function withMetersPerPixel (f) {
    let viewer = odinCesium.viewer;
    let w = viewer.scene.canvas.clientWidth;
    let h = viewer.scene.canvas.clientHeight;

    // FIXME - this returns 0 if we get close (no tile level?). It also won't cover terrain height, which would cause
    // problems as we zoom in (where we need distances most)
    //let dPixel = viewer.scene.camera.getPixelSize( WGS84BoundingSphere, w, h);  // distance [m] per pixel
    //if (dPixel) return dPixel;

    // the hard way to work around this - compute two rays close to the center of the canvas and deduce distance from
    // respective ellipsoid points. Note this does not account yet for elevation at the center
    let dx = 2;
    let camera = viewer.scene.camera;
    let wp = new Cesium.Cartesian2( Math.round(w/2), Math.round(h/2)); // center window coordinates
    let ray1 = camera.getPickRay(wp);
    wp.x += dx; // add dx horizontal pixels
    let ray2 = camera.getPickRay(wp);

    let cp = ray1.origin;
    let geo = util.ecefToGeo( cp.x, cp.y, cp.z);
    let cameraHeight = geo.alt; // of camera above ellipsoid

    if (cameraHeight < 80000) { // factor in terrain height
        geo = new Cesium.Cartographic( geo.lon, geo.lat);
        geo.height = viewer.scene.globe.getHeight(geo);
        cameraHeight -= geo.height; // subtract the terrain elevation from the camera height
    }

    let p1 = Cesium.Cartesian3.add( ray1.origin,
        Cesium.Cartesian3.multiplyByScalar( ray1.direction, cameraHeight,new Cesium.Cartesian3()), new Cesium.Cartesian3());
    let p2 = Cesium.Cartesian3.add( ray2.origin,
                Cesium.Cartesian3.multiplyByScalar( ray2.direction, cameraHeight, new Cesium.Cartesian3()), new Cesium.Cartesian3());
    let dist = Cesium.Cartesian3.distance( p1, p2, new Cesium.Cartesian3());
    let dPixel = dist / dx;

    f( dPixel)
}

function getMetricScale (dPixel) {
    let unit, nUnits, unitPx;
    let maxWidth = config.scale.width;

    outer: for ( let i=0; i < metricScaleUnits.length; i++) {
        unit = metricScaleUnits[i];
        unitPx = unit / dPixel;

        let nu = [5,4,3,2];
        for (let j=0; j<nu.length; j++) {
            nUnits = nu[j];
            if ((unitPx * nUnits) <= maxWidth) break outer;
        }
    }

    if (unit >= 1000) {
        return { unit, unitM: unit, nUnits, unitPx, tickBase: unit / 1000, unitLabel: "km" };
    } else {
        return { unit, unitM: unit, nUnits, unitPx, tickBase: unit, unitLabel: "m" };
    }
}

function getUsScale (dPixel) {
    let unit, nUnits, unitPx;
    let maxWidth = config.scale.width;

    //--- start with US miles
    for ( let i=0; i < usMileScaleUnits.length; i++) {
        unit = usMileScaleUnits[i];
        unitPx = (unit * util.metersPerUsMile) / dPixel;

        let nu = mileTicks(unit);
        for (let j=0; j<nu.length; j++) {
            nUnits = nu[j];
            if ((unitPx * nUnits) <= maxWidth) {
                return { unit, unitM: unit * util.metersPerUsMile, nUnits, unitPx, tickBase: unit, unitLabel: "mi" };
            }
        }
    }

    //--- switch to yards
    for ( let i=0; i < usYardScaleUnits.length; i++) {
        unit = usYardScaleUnits[i];
        unitPx = (unit * util.metersPerYard) / dPixel;

        let nu = [5,4,3,2];
        for (let j=0; j<nu.length; j++) {
            nUnits = nu[j];
            if ((unitPx * nUnits) <= maxWidth) {
                return { unit, unitM: unit * util.metersPerYard, nUnits, unitPx, tickBase: unit, unitLabel: "yd" };
            }
        }
    }

    return { unit, unitM: unit * util.metersPerYard, nUnits, unitPx, tickBase: unit, unitLabel: "yd" }; // smallest yard unit
}

function mileTicks (unit) {
    if (unit > 0.25) {
        return [5,4,3,2];
    } else {
        return [4,2];
    }
}

function formatValue (v) {
    return parseFloat( v.toFixed(2)).toString(); // avoid floating point artifacts such as 0.30000000000000004
}

//--- rendering

function drawRuler () {
    if (!scale) return;

    let unitPx = scale.unitPx;
    let tickBase = scale.tickBase;
    let len = barLength();
    let label = `${formatValue( tickBase * nUnits)} ${scale.unitLabel}`;
    let clr = config.scale.cssColor;

    let canvasHeight = config.scale.height;
    let canvasWidth = Math.ceil( 2*X_MARGIN + len + (isRotated() ? ANGLE_LABEL_WIDTH : 0));
    canvas.width = canvasWidth; // this also clears the canvas
    canvas.height = canvasHeight;

    //--- position and orientation of the canvas (rotated around the left end of the bar)
    let a = getAnchor();
    canvas.style.left = `${Math.round(a.x - X_MARGIN)}px`;
    canvas.style.top = `${Math.round(a.y - canvasHeight/2)}px`;
    canvas.style.transform = isRotated() ? `rotate(${angle - 90}deg)` : "";

    let ctx = canvas.getContext("2d");
    ctx.fillStyle = clr;
    ctx.strokeStyle = clr;
    ctx.textBaseline = "middle";

    let y = canvasHeight / 2;

    //--- the bar
    ctx.beginPath();
    ctx.lineWidth = 2;
    if (tiltMode) ctx.setLineDash([8,4]); // visual clue for tilt mode (bar is only valid for horizontal ground distance)
    ctx.moveTo( X_MARGIN, y);
    ctx.lineTo( X_MARGIN + len, y);
    ctx.stroke();
    ctx.setLineDash([]);

    //--- the ticks
    if (unitPx > 25) {
        ctx.font = config.scale.smallFont;
        ctx.beginPath();
        for (let i = 0; i<=nUnits; i++) {
            let x = X_MARGIN + (i*unitPx);
            ctx.moveTo(x, y);
            ctx.lineTo(x, y + 5);

            drawText( ctx, formatValue(tickBase * i), x, y+10, "center");
        }
        ctx.closePath();
        ctx.stroke();
    }

    //--- the length string
    ctx.font = tiltMode ? `italic ${config.scale.font}` : config.scale.font;
    drawText( ctx, label, Math.round( X_MARGIN + len / 2), y - 8, "center");

    //--- the heading (only if rotated)
    if (isRotated()) {
        drawText( ctx, `${getHeading()}°`, X_MARGIN + len + X_MARGIN, y, "left");
    }
}

// draw text with its vertical center at (x,y). If the bar points left we rotate the text by 180 degrees around that point
// so that it is not upside down, which means we also have to swap left/right alignment
function drawText (ctx, text, x, y, align) {
    if (isFlipped()) {
        ctx.save();
        ctx.translate( x, y);
        ctx.rotate( Math.PI);
        ctx.textAlign = (align === "left") ? "right" : (align === "right") ? "left" : align;
        ctx.fillText( text, 0, 0);
        ctx.restore();
    } else {
        ctx.textAlign = align;
        ctx.fillText( text, x, y);
    }
}
