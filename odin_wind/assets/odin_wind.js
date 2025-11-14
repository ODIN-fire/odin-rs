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

import * as main from "../odin_server/main.js";
import * as util from "../odin_server/ui_util.js";
import * as data from "../odin_server/ui_data.js";
import * as ui from "../odin_server/ui.js";
import * as ws from "../odin_server/ws.js";
import * as odinCesium from "../odin_cesium/odin_cesium.js";
import * as wf from "./windfield.js";
import { config } from "./odin_wind_config.js";


/* #region types **************************************************************************************/

const MODULE_PATH ="odin_wind::wind_service::WindService";

const RegionStatus = {
    ACTIVE: "active",
    WAITING: "⧖",
    INACTIVE: ""
};

const BBOX_PATTERN = util.glob2regexp("{rect/**,**/rect/**,**/rect}"); // any pathname with 'rect' in it

// the supported display types
const AnimDisplay = "animation";
const VectorDisplay = "vector";
const ContourDisplay = "contour";
var selDisplay = undefined;

// the supported wind sources
const WindNinjaSource = "windNinja";
const Hrrr10Source = "hrrr_10m";
const Hrrr80Source = "hrrr_80m";
var selSource = undefined;

//--- render parameters
var animRender = {...config.animRender};
var vectorRender = {...config.vectorRender};
var contourRender = {...config.contourRender};

class ForecastRegion {
    constructor (name, bbox, status) {
        this.name = name; // this is the key (path) of a shared GeoRect
        this.bbox = bbox;
        this.status = status;
        this.isSubscribed = false;
        this.asset = undefined; // set when we show the bbox

        this.forecasts = [];
    }

    isInactive() { return this.status === RegionStatus.INACTIVE }
    setActive(cond) { this.status = cond ? RegionStatus.ACTIVE : RegionStatus.INACTIVE; }
    nForecasts() { return this.forecasts.length; }
    isRectShowing() { return (this.asset != null); }

    addForecast (newForecast) {
        let isRegionSelected = Object.is( this, selectedRegion); 
        this.purgeOldForecasts();

        let forecasts = this.forecasts;

        for (let i=0; i<forecasts.length; i++) {
            let f = forecasts[i];

            if (newForecast.date < f.date) { // insert older forecast
                forecasts.splice(i, 0, newForecast);
                if (isRegionSelected) ui.insertListItem( forecastView, newForecast, i);

                return;
            } else if (newForecast.date == f.date) {
                if (newForecast.step <= f.step) { // this replaces an outdated forecast
                    forecasts[i] = newForecast;
                    if (isRegionSelected) ui.replaceListItem( forecastView, f, newForecast);

                } // otherwise the new forecast was dead on arrival
                return;
            }
        }

        forecasts.push( newForecast);
        if (isRegionSelected) ui.appendListItem( forecastView, newForecast);
    }

    purgeOldForecasts () {
        let forecasts = this.forecasts;
        let now = Date.now(); // TODO = should use simTime
        let nPurge = 0;
        for (let f of forecasts) {
            let dh = util.hoursBetween( f.date, now);
            if (dh < 0) break; // the rest is in the future
            if (dh > config.backHours) nPurge++; 
        }
        if (nPurge>0) {
            for (let i=0; i<nPurge; i++) {
                forecasts[i].clearWindFields(); // if they are purged we can't control their visibility anymore
                ui.removeListItemIndex( forecastView, 0);
            }
            forecasts.splice(0,nPurge);
        }
    }

    createAndShowAsset () {
        if (this.asset) return;

        let renderOpts = config.regionRender;
        let d = this.bbox;

        let colorMaterial = new Cesium.ColorMaterialProperty();
        colorMaterial.color = new Cesium.CallbackProperty( ()=>renderOpts.color, false);

        // Cesium rect properties do not support outlines that are clampToGround so we turn this into a polygon
        let points = [];
        points.push( new Cesium.Cartesian3.fromDegrees( d.west, d.north));
        points.push( new Cesium.Cartesian3.fromDegrees( d.east, d.north));
        points.push( new Cesium.Cartesian3.fromDegrees( d.east, d.south));
        points.push( new Cesium.Cartesian3.fromDegrees( d.west, d.south));
        points.push( points[0]);

        let entity = new Cesium.Entity({
            id: this.name,
            polyline: {
                positions: points,
                clampToGround: true,
                material: colorMaterial,
                width: new Cesium.CallbackProperty( ()=>renderOpts.lineWidth, false)
            }
        });

        if (renderOpts.fill) {
            let fillOpts = renderOpts.fill;
            let fillColorMaterial = new Cesium.ColorMaterialProperty();
            fillColorMaterial.color = new Cesium.CallbackProperty( ()=>fillOpts.color.withAlpha(fillOpts.alpha), false);

            entity.polygon = {
                hierarchy: points,
                heightReference: Cesium.CLAMP_TO_GROUND,
                fill: new Cesium.CallbackProperty( ()=>renderOpts.fill != undefined, false),
                material: fillColorMaterial,
                zIndex: 2,
            };
        }

        this.asset = entity;
        odinCesium.addEntity( entity);
    }

    clearAsset () {
        if (this.asset) {
            this.asset.show = false;
            odinCesium.removeEntity( this.asset);
            this.asset = null;
        }
    }
}

class Forecast {
    constructor (date,step,mesh,wxSrc,urlBase) {
        this.date = date;
        this.step = step;
        this.mesh = mesh;
        this.wxSrc = wxSrc;
        this.urlBase = urlBase;

        // note - property names have to match display type and wind source values defined above
        this.animation = {};
        this.vector = {};
        this.contour = {};

        this.animation.windNinja = new wf.AnimField( urlBase, "__grid.csv", animRender, wfStatusChanged);
        this.vector.windNinja = new wf.VectorField( urlBase, "__vector.csv", vectorRender, wfStatusChanged);
        this.contour.windNinja = new wf.ContourField( urlBase, "__contour.json", contourRender, wfStatusChanged);

        this.animation.hrrr_10m = new wf.AnimField( urlBase, "__hrrr__10__grid.csv", animRender, wfStatusChanged);
        this.vector.hrrr_10m = new wf.VectorField( urlBase, "__hrrr__10__vector.csv", vectorRender, wfStatusChanged);
        this.contour.hrrr_10m = new wf.ContourField( urlBase, "__hrrr__10__contour.json", contourRender, wfStatusChanged);

        this.animation.hrrr_80m = new wf.AnimField( urlBase, "__hrrr__80__grid.csv", animRender, wfStatusChanged);
        this.vector.hrrr_80m = new wf.VectorField( urlBase, "__hrrr__80__vector.csv", vectorRender, wfStatusChanged);
        this.contour.hrrr_80m = new wf.ContourField( urlBase, "__hrrr__80__contour.json", contourRender, wfStatusChanged);
    }

    status () { return this[selDisplay][selSource].status; }

    startViewChange () { 
        if (this.isShowing()) {
            this.showWindField(false);
            this._onHold = true;
            this[selDisplay][selSource].startViewChange(); 
        }
    }

    endViewChange () { 
        if (this._onHold) {
            this[selDisplay][selSource].endViewChange(); 
            this.showWindField(true);
            this._onHold = undefined;
        }
    }

    renderChanged () { 
        // TODO - to be consistent we either we have to set sliders upon source/display selection or we have
        // to update all source/display entries here

        if (Object.is(selDisplay, AnimDisplay)) {
            this.animation[selSource].setRenderOpts( animRender);
        } else if (Object.is(selDisplay, VectorDisplay)) {
            this.vector[selSource].setRenderOpts( vectorRender);
        } else if (Object.is(selDisplay, ContourDisplay)) {
            this.contour[selSource].setRenderOpts( contourRender);
        }
    }

    isShowing() { return Object.is( this.status(), wf.WindFieldStatus.SHOWING); }

    getResolution() { return Object.is( selSource, WindNinjaSource) ? this.mesh : 3000; }

    showWindField (showIt) { 
        this[selDisplay][selSource].setVisible( showIt); 
    }

    // this keeps them loaded but sets primitives invisible
    hideWindFields () {
        this.animation.windNinja.setVisible(false);
        this.vector.windNinja.setVisible(false);
        this.contour.windNinja.setVisible(false);

        this.animation.hrrr_10m.setVisible(false);
        this.vector.hrrr_10m.setVisible(false);
        this.contour.hrrr_10m.setVisible(false);

        this.animation.hrrr_80m.setVisible(false);
        this.vector.hrrr_80m.setVisible(false);
        this.contour.hrrr_80m.setVisible(false);
    }

    // this releases all resources and causes a reload of the data files upon next display
    clearWindFields () {
        this.animation.windNinja.clear();
        this.vector.windNinja.clear();
        this.contour.windNinja.clear();

        this.animation.hrrr_10m.clear();
        this.vector.hrrr_10m.clear();
        this.contour.hrrr_10m.clear();

        this.animation.hrrr_80m.clear();
        this.vector.hrrr_80m.clear();
        this.contour.hrrr_80m.clear();
    }
}

/* #endregion types */

ws.addWsHandler( MODULE_PATH, handleWsMessages);
main.addShareHandler( handleShareMessage);

//--- our data model
var forecastRegions = new Map(); // name -> ForecastRegion

//--- UI state we track
var regionView = undefined;
var forecastView = undefined;
var displayCb = undefined;
var sourceCb = undefined;

var selectedRegion = undefined;
var selectedForecast = undefined;

createIcon();
createWindow();

initDisplayCb();
initSourceCb();

initRegionView();
initForecastView();

initAnimDisplayControls();
initVectorDisplayControls();
initContourDisplayControls();

//const viewer = await odinCesium.viewerReadyPromise; // Safari bug workaround (setupEventListeners use odinCesium)
setupEventListeners();

odinCesium.initLayerPanel("wind", config, showWind);
console.log("odin_wind initialized");

/* #region websocket message handler ***********************************************************************/

function handleWsMessages(msgType, msg) {
    switch (msgType) {
        case "forecastRegions": handleForecastRegions(msg); break;
        case "startForecastRegion": handleStartForecastRegion(msg); break;
        case "stopForecastRegion": handleStopForecastRegion(msg); break;
        case "rejectForecastRegion": handleRejectForecastRegion(msg); break;
        case "forecast": handleForecast(msg); break;
        default: console.log("unknown websock message ", msgType, " ignored");
    }
}

// this is the response to connecting - all forecast regions that are currently active
function handleForecastRegions (frsMsg) {
    for (let r of frsMsg.regions) {
        let fr = new ForecastRegion( r.region, r.bbox, RegionStatus.ACTIVE);
        forecastRegions.set(fr.name, fr);

        for (let fcMsg of r.forecasts) { // those are reported without region name, no need to recreate
            let fc = new Forecast( fcMsg.date, fcMsg.step, fcMsg.mesh, util.intern(fcMsg.wxSrc), fcMsg.urlBase);
            fr.addForecast( fc);
        }
    }

    let tree = data.ExpandableTreeNode.from( forecastRegions.values(), e=>e.name);
    ui.setTree( regionView, tree);
    ui.clearList( forecastView);
}

// this is the notification everybody gets when a new region becomes active
function handleStartForecastRegion (startFcr) {
    let fr = forecastRegions.get(startFcr.name);
    if (fr) { 
        fr.setActive(true);
        ui.updateListItem( regionView, fr);

    } else {
        // TODO - should we add regions that are not (yet) shared here? Sharing should always happen before this
        console.log("ignore unknown forecast region ", fr.name);
    }
}

// this is the notification the requester gets when a region request is rejected
function handleRejectForecastRegion (rejectFcr) {
    let fr = forecastRegions.get(rejectFcr.name);
    if (fr) { 
        fr.setActive(false);
        ui.updateListItem( regionView, fr);
        alert( `region request rejected: ${rejectFcr.rejection}` );
    }
}

function handleStopForecastRegion (stopFcr) {
    let fr = forecastRegions.get(stopFcr.region);
    if (fr) { 
        fr.setActive(false);
        ui.updateListItem( regionView, fr);
    }
}

// this is the notification everybody gets when a new forecast for one of the active regions becomes available
function handleForecast (fcMsg) {
    let fr = forecastRegions.get(fcMsg.region);
    if (fr) {
        let fc = new Forecast( fcMsg.date, fcMsg.step, fcMsg.mesh, util.intern(fcMsg.wxSrc), fcMsg.urlBase);
        fr.addForecast( fc); // this takes care of updating the forecastView if the region is selected
        ui.updateListItem( regionView, fr);
    }
}

/* #endregion websocket message handler */

/* #region share message handler ***************************************************************************/

function handleShareMessage (msg) {
    if (msg.SHARE_INITIALIZED) { // we get that no matter what the share implementation is
        setSharedGeoRects();

    } else if (main.isShareInitialized()) { // if not we still get a SHARE_INITIALIZED
        if (msg.setShared) {
            let sharedItem = msg.setShared;
            if (sharedItem.key.match(BBOX_PATTERN)) {
                addSharedGeoRect(sharedItem);// 
            }
        } else if (msg.removeShared) {
            removeSharedGeoRect( msg.removeShared);
            // if subscribed unsubscribe then remove from regionView
        }
    }
}

// note this might come *after* we got a 'forecastRegions` websocket message - don't overwrite
function setSharedGeoRects() {
    let sharedItems = getSharedGeoRects();

    for (let si of sharedItems) {
        if (!forecastRegions.get( si.key)) {
            let fr = new ForecastRegion( si.key, si.value.data, RegionStatus.INACTIVE);
            forecastRegions.set( fr.name, fr);
        }
    }

    let tree = data.ExpandableTreeNode.from( forecastRegions.values(), e=>e.name);
    ui.setTree( regionView, tree);
}

function addSharedGeoRect(si) {
    let fr = new ForecastRegion( si.key, si.value.data, RegionStatus.INACTIVE);
    forecastRegions.set( fr.name, fr);
    ui.sortInTreeItem( regionView, fr, fr.name);
}

function removeSharedGeoRect(sharedItemKey) {

}

function getSharedGeoRects() {
    let rects = [];
    let items = main.getAllMatchingSharedItems( BBOX_PATTERN);
    for (let item of items) {
        if (item.value.type == "GeoRect") {
            let p = item.value.data;
            rects.push( item);
        }
    }

    return rects;
}

/* #endregion share message handler */

/* #region UI window ***************************************************************************************/

function createIcon() {
    return ui.Icon("./asset/odin_wind/wind-icon.svg", (e)=> ui.toggleWindow(e,'wind'), "local wind prediction");
}

function createWindow() {
    return ui.Window("Wind", "wind", "./asset/odin_wind/wind-icon.svg")(
        ui.LayerPanel("wind", toggleShowWind),
        
        ui.Panel("wind-fields", true)(
            ui.RowContainer()(
                (regionView = ui.TreeList("wind.regions", 10, "25rem", selectRegion, null,null, zoomRegion)),
            ),
            ui.RowContainer()(
                (displayCb = ui.Choice( "display", "wind.field.display", selectWindDisplay)),
                ui.HorizontalSpacer(1),
                (sourceCb = ui.Choice( "source", "wind.field.source", selectWindSource))
            ),
            (forecastView = ui.List("wind.forecasts", 6, selectForecast)),
            ui.ListControls("wind.forecasts",null,null,null,null,clearWindFields)
        ),
        ui.Panel("anim display")(
            ui.ColumnContainer("align_right")(
                ui.Slider("particles", "wind.anim.particles", windParticlesChanged),
                ui.Slider("extra height", "wind.anim.height", windHeightChanged),
                ui.Slider("fade opacity", "wind.anim.fade_opacity", windFadeOpacityChanged),
                ui.Slider("drop", "wind.anim.drop", windDropRateChanged),
                ui.Slider("drop bump", "wind.anim.drop_bump", windDropRateBumpChanged),
                ui.Slider("speed factor", "wind.anim.speed", windSpeedChanged),
                ui.Slider("line width", "wind.anim.width", windWidthChanged),
                ui.ColorField("color", "wind.anim.color", true, animColorChanged),
            )
        ),
        // TODO - those fields should be created from config
        ui.Panel("vector display")(
            ui.Slider("point size", "wind.vector.point_size", vectorPointSizeChanged),
            ui.Slider("line width", "wind.vector.width", vectorLineWidthChanged),
            ...colorFields( vectorRender, "wind.vector", vectorLineColorChanged)
        ),
        ui.Panel("contour display")(
            ui.Slider("stroke width", "wind.contour.stroke_width", contourStrokeWidthChanged),
            ui.Slider("fill alpha", "wind.contour.alpha", contourAlphaChanged),
            ...colorFields( contourRender, "wind.contour", contourColorChanged)
        )
    );
}

function colorFields (renderOpts, group, callback) {
    let fields = [];
    for (let i=0; i<renderOpts.colors.length; i++) {
        let spd = i*5;
        let lbl = (i < renderOpts.colors.length-1) ? `${spd}-${spd+5}mph` : `>${spd}mph`;
        let id = `${group}.color${i}`;
        fields.push( ui.ColorField( lbl, id, true, callback));
    }
    return fields;
}

function initDisplayCb() {
    ui.setChoiceItems( displayCb, [AnimDisplay, VectorDisplay, ContourDisplay], 0);
    selDisplay = ui.getSelectedChoiceValue( displayCb);
}

function isAnimDisplay () {
    return selDisplay == "animation";
}

function isVectorDisplay () {
    return selDisplay == "vector";
}

function isContourDisplay () {
    return selDisplay == "contour";
}

function initSourceCb() {
    ui.setChoiceItems( sourceCb, [WindNinjaSource, Hrrr10Source, Hrrr80Source], 0);
    selSource = ui.getSelectedChoiceValue( sourceCb);
}

function initRegionView() {
    let view = regionView;
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "status", width: "3rem", attrs: [], map: e => e.status },
            { name: "fcs", tip: "number of available forecasts",  width: "2rem", attrs: ["fixed", "alignRight"], map: e => e.nForecasts() },
            ui.listItemSpacerColumn(1),
            { name: "upd", tip: "update", width: "2.1rem", attrs: [], map: e => ui.createCheckBox(e.isSubscribed, toggleRegionSubscribe) },
            { name: "show", tip: "show region bbox", width: "2.1rem", attrs: [], map: e => ui.createCheckBox( e.isRectShowing(), toggleShowRegion) }
        ]);
    }
}

function initForecastView() {
    let view = forecastView;
    if (view) {
        ui.setListItemDisplayColumns(view, ["header"], [
            { name: "forecast", width: "10rem",  attrs: ["fixed"], map: e => util.toLocalDateHMTimeString( e.date) },
            { name: "Δt", tip: "hours from forecast creation", width: "2rem", attrs: ["fixed", "alignRight"], map: e => e.step },
            ui.listItemSpacerColumn(1),
            { name: "res", tip: "mesh resolution in meters", width: "3rem", attrs: ["fixed", "alignRight"], map: e => e.getResolution() },
            ui.listItemSpacerColumn(1),
            { name: "", width: "1rem", attrs:[], map: e => e.status() },
            { name: "show", tip: "toggle windfield visibility", width: "2.1rem", attrs: [], map: e => ui.createCheckBox(e.isShowing(), toggleShowWindField) },
        ]);
    }
    return view;
}

function initAnimDisplayControls() {
    var e = undefined;

    let r = animRender;

    e = ui.getSlider("wind.anim.particles");
    ui.setSliderRange(e, 0, 128, 16, util.f_0);
    ui.setSliderValue(e, r.particlesTextureSize);

    e = ui.getSlider("wind.anim.height");
    ui.setSliderRange(e, 0, 10000, 500, util.f_0);
    ui.setSliderValue(e, r.particleHeight);

    e = ui.getSlider("wind.anim.fade_opacity");
    ui.setSliderRange(e, 0.8, 1.0, 0.01, util.f_2);
    ui.setSliderValue(e, r.fadeOpacity);

    e = ui.getSlider("wind.anim.drop");
    ui.setSliderRange(e, 0.0, 0.01, 0.001, util.f_3);
    ui.setSliderValue(e, r.dropRate);

    e = ui.getSlider("wind.anim.drop_bump");
    ui.setSliderRange(e, 0.0, 0.05, 0.005, util.f_3);
    ui.setSliderValue(e, r.dropRateBump);

    e = ui.getSlider("wind.anim.speed");
    ui.setSliderRange(e, 0.0, 0.3, 0.02, util.f_2);
    ui.setSliderValue(e, r.speedFactor);

    e = ui.getSlider("wind.anim.width");
    ui.setSliderRange(e, 0.0, 3.0, 0.5, util.f_1);
    ui.setSliderValue(e, r.lineWidth);

    e = ui.getField("wind.anim.color");
    ui.setField(e, r.color.toCssHexString());
}

function initVectorDisplayControls() {
    var e = undefined;

    e = ui.getSlider("wind.vector.point_size");
    ui.setSliderRange(e, 0, 8, 0.5, util.f_1);
    ui.setSliderValue(e, vectorRender.pointSize);

    e = ui.getSlider("wind.vector.width");
    ui.setSliderRange(e, 0, 5, 0.2, util.f_1);
    ui.setSliderValue(e, vectorRender.strokeWidth);

    for (var i=0; i<vectorRender.colors.length; i++) {
        e = ui.getField(`wind.vector.color${i}`);
        if (e) {
            e._uiIdx = i;
            ui.setField(e, vectorRender.colors[i].toCssHexString());
        }
    }
}

function initContourDisplayControls() {
    var e = undefined;

    e = ui.getSlider("wind.contour.stroke_width");
    ui.setSliderRange(e, 0, 3, 0.5, util.f_1);
    ui.setSliderValue(e, contourRender.strokeWidth);

    e = ui.getSlider("wind.contour.alpha");
    ui.setSliderRange(e, 0, 1.0, 0.1, util.f_1);
    ui.setSliderValue(e, contourRender.alpha);

    for (var i = 0; i<contourRender.colors.length; i++) {
        e = ui.getField(`wind.contour.color${i}`);
        if (e) {
            ui.setField(e, contourRender.colors[i].toCssHexString());
        }
    }
}

/* #endregion UI window */

/* #region UI callbacks *************************************************************************************/

/// some of our windfield visualizations have to be aware of view changes
function setupEventListeners() {
    // since this requires an initialized Cesium.Viewer we have to sync with odin_cesium.js init
    odinCesium.viewerReadyPromise.then( (viewer) => {
        let scene = viewer.scene;

        viewer.camera.moveStart.addEventListener(startViewChange);
        viewer.camera.moveEnd.addEventListener(endViewChange);

        var resized = false;

        window.addEventListener("resize", () => {
            resized = true;
            //scene.primitives.show = false;
            //windEntries.forEach(e => e.removePrimitives());
        });

        scene.preRender.addEventListener(() => {
            if (resized) {
                //windEntries.forEach(e => e.updatePrimitives());
                resized = false;
                //scene.primitives.show = true;
            }
        });
    });
}

function startViewChange() {
    for (let region of forecastRegions.values()) {
        for (let forecast of region.forecasts) {
            forecast.startViewChange();
        }
    }
}

function endViewChange() {
    wf.updateViewerParameters();
    for (let region of forecastRegions.values()) {
        for (let forecast of region.forecasts) {
            
            forecast.endViewChange();
        }
    }
}

function toggleShowWind (event) { // our local show/hide
    // show/hide wind assets
}

function showWind (cond) { // layer mgnt interface

}

function toggleRegionSubscribe (event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let fcr = ui.getListItemOfElement(cb);
        if (ui.isCheckBoxSelected(cb)) {
            fcr.status = RegionStatus.WAITING;
            fcr.isSubscribed = true;
            ui.updateListItem( regionView, fcr);
            ws.sendWsMessage( MODULE_PATH, "addWindClient", {name: fcr.name, bbox: fcr.bbox});

        } else {
            // no status update yet - if this was the last client we will get a notification from the server
            fcr.isSubscribed = false;
            ui.updateListItem( regionView, fcr);
            ws.sendWsMessage( MODULE_PATH, "removeWindClient", {name: fcr.name, bbox: fcr.bbox});
        }
    }
}

function toggleShowRegion (event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let fcr = ui.getListItemOfElement(cb);
        if (ui.isCheckBoxSelected(cb)) { // show fcr.bbox
            fcr.createAndShowAsset();
        } else { // hide fcr.bbox
            fcr.clearAsset();
        }
    }
}

function toggleShowWindField (event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let fc = ui.getListItemOfElement(cb);
        if (fc) {
            fc.showWindField( ui.isCheckBoxSelected(cb));
        }
    }
}

function clearWindFields() {
    if (selectedRegion) {
        for (let forecast of selectedRegion.forecasts) {
            forecast.clearWindFields();
        }
        ui.updateListItems( forecastView);
    }
}

// callback when a wind field visualization has changed status (it might not be showing)
function wfStatusChanged (wf) {
    if (selectedRegion) {
        for (let forecast of selectedRegion.forecasts) {
            if (Object.is( forecast[selDisplay][selSource], wf)) {
                ui.updateListItem( forecastView, forecast);
                return;
            }
        }
    }
}

function updateForecasts () {
    if (selectedRegion) {
        for (let forecast of selectedRegion.forecasts) {
            ui.updateListItem( forecastView, forecast);
        }
    }
}

function selectRegion (event) {
    selectedRegion = ui.getSelectedListItem( regionView);
    if (selectedRegion) {
        ui.setListItems( forecastView, selectedRegion.forecasts);
    } else {
        ui.clearList( forecastView);
    }
}

function zoomRegion (event) {
    if (selectedRegion) {
        let bbox = selectedRegion.bbox;
        let margin = config.zoomMargin;
        let rect = Cesium.Rectangle.fromDegrees( bbox.west - margin, bbox.south - margin, bbox.east + margin, bbox.north + margin);
        let cameraPos = odinCesium.viewer.camera.getRectangleCameraCoordinates(rect);
        odinCesium.zoomTo(cameraPos);
    }
}

function selectForecast (event) {
    selectedForecast = ui.getSelectedListItem( forecastView);
}

function selectWindDisplay (event) {
    selDisplay = ui.getSelectedChoiceValue( event.target);
    updateForecasts();
}

function selectWindSource (event) {
    selSource = ui.getSelectedChoiceValue( event.target);
    updateForecasts();
}

/* #endregion UI callbacks */


/* #region anim display settings ****************************************************************************/

// we have to delay particleSystem updates while user moves the slider
var pendingUserInputChange = false; 

function triggerAnimRenderChange(forecast, newInput) {
    if (isAnimDisplay()) {
        if (newInput) pendingUserInputChange = true;

        setTimeout(() => {
            if (pendingUserInputChange) {
                pendingUserInputChange = false;
                triggerAnimRenderChange(forecast, false);
            } else {
                forecast.renderChanged();
            }
        }, 500);
    }
}

function windParticlesChanged(event) {
    let n = ui.getSliderValue(event.target);
    animRender.particlesTextureSize = n;
    animRender.maxParticles = n*n;
    if (selectedForecast && isAnimDisplay()) { triggerAnimRenderChange( selectedForecast, true); }
}

function windFadeOpacityChanged(event) {
    animRender.fadeOpacity = ui.getSliderValue(event.target);
    if (selectedForecast && isAnimDisplay()) { triggerAnimRenderChange( selectedForecast, true); }
}

function windSpeedChanged(event) {
    animRender.speedFactor = ui.getSliderValue(event.target);
    if (selectedForecast && isAnimDisplay()) { triggerAnimRenderChange( selectedForecast, true); }
}

function windWidthChanged(event) {
    animRender.lineWidth = ui.getSliderValue(event.target);
    if (selectedForecast && isAnimDisplay()) { triggerAnimRenderChange( selectedForecast, true); }
}

function windHeightChanged(event) {
    animRender.particleHeight = ui.getSliderValue(event.target);
    if (selectedForecast && isAnimDisplay()) { triggerAnimRenderChange( selectedForecast, true); }
}

function windDropRateChanged(event) {
    animRender.dropRate = ui.getSliderValue(event.target);
    if (selectedForecast && isAnimDisplay()) { triggerAnimRenderChange( selectedForecast, true); }
}

function windDropRateBumpChanged(event) {
    animRender.dropRateBump = ui.getSliderValue(event.target);
    if (selectedForecast && isAnimDisplay()) { triggerAnimRenderChange( selectedForecast, true); }
}

function animColorChanged(event) {
    let clr= Cesium.Color.fromCssColorString( event.target.value);
    if (clr) {
        animRender.color = clr;
        if (selectedForecast && isAnimDisplay()) { triggerAnimRenderChange( selectedForecast, true); }
    }
}

/* #endregion anim display settings */

/* #region vector display settings **************************************************************************/

function vectorPointSizeChanged(event) {
    vectorRender.pointSize =  ui.getSliderValue(event.target);
    if (selectedForecast && isVectorDisplay()) { selectedForecast.renderChanged();}
}

function vectorLineWidthChanged(event) {
    vectorRender.strokeWidth =  ui.getSliderValue(event.target);
    if (selectedForecast && isVectorDisplay()) { selectedForecast.renderChanged();}
}

function vectorLineColorChanged(event) {
    let e = event.target;
    let clr= Cesium.Color.fromCssColorString( event.target.value);
    if (clr) {
        let idx = e._uiIdx;
        if (idx >= 0 && idx < vectorRender.colors.length) {
            vectorRender.colors[idx] = clr;
            if (selectedForecast && isVectorDisplay()) { selectedForecast.renderChanged(); }
        }
    }
}

/* #endregion vector display settings */

/* #region contour display settigns *************************************************************************/

function contourStrokeWidthChanged(event) {
    contourRender.strokeWidth =  ui.getSliderValue(event.target);
    if (selectedForecast && isContourDisplay()) { selectedForecast.renderChanged();}
}

function contourColorChanged(event) {
    let idx = colorIndex( event.target);
    let clr= Cesium.Color.fromCssColorString( event.target.value);
    if (clr) {
        contourRender.colors[idx] = clr;
        if (selectedForecast && isContourDisplay()) { selectedForecast.renderChanged();}
    }
}

function contourAlphaChanged(event) {
    contourRender.alpha =  ui.getSliderValue(event.target);
    if (selectedForecast && isContourDisplay()) { selectedForecast.renderChanged();}
}

/* #endregion contour display settings */

function colorIndex (e) {
    let id = e.id;
    return parseInt( id.charAt( id.length-1));
}

