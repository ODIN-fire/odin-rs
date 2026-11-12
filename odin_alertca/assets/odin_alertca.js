/**
 * Copyright © 2025, United States Government, as represented by the Administrator of
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

import { config } from "./odin_alertca_config.js";              // associated static config for this module

import * as main from "../odin_server/main.js";               // global functions (e.g. for data sharing)
import * as util from "../odin_server/ui_util.js";            // common, cross-module support functions
import * as data from "../odin_server/ui_data.js";
import * as ui from "../odin_server/ui.js";                   // ODIN specific user interface library
import * as wnd from "../odin_server/ui_windows.js";
import * as ws from "../odin_server/ws.js";                   // websocket processing
import * as odinCesium from "../odin_cesium/odin_cesium.js";  // virtual globe rendering interface from odin_cesium


const MOD_PATH = "odin_alertca::alertca_service::AlertCaService";   // the name of the associated odin-rs SpaService

ws.addWsHandler( MOD_PATH, handleWsMessages);                 // incoming websocket messages for MOD_PATH
//main.addShareHandler( handleShareMessage);                  // if module uses shared data items
//main.addSyncHandler( handleSyncMessage);                    // if module supports synchronization commands

/* #region types ***************************************************************************************/

class CameraAssets {
    constructor(symbol, fov) {
        this.symbol = symbol;
        this.fov = fov;
    }

    showAssets (cond) {
        if (this.symbol) this.symbol.show = cond;
        if (this.fov) this.fov.show = cond;
    }
}

// the client side representation of an AlertCalifornia camera. Note that cameras can move, i.e. position can change
class CameraEntry {
    constructor (camera) {
        this.id = camera.id;      // not human readable
        this.name = camera.name;  // used as label
        this.label = camera.name;

        this.position = camera.position; // { lon, lat, alt }
        this.pos = toCartesian3( camera.pos);
        this.maxFov = camera.maxFov;
        this.setLocalAttributes( camera);

        this.data = data.CircularBuffer.fromArray( camera.data ? camera.data : [], config.maxHistory);
        this.alerts = data.CircularBuffer.fromArray( camera.alerts ? camera.alerts : [], config.maxHistory);

        this.assets = this.createAssets();
    }

    // configured (AlertCaConfig) camera height above ground [m] and tilt correction [deg]
    setLocalAttributes (camera) {
        this.heightAboveGround = (camera.heightAboveGround !== undefined) ? camera.heightAboveGround : 10;
        this.tiltOffset = camera.tiltOffset || 0;
    }

    latestData () {
        let data = this.data;
        return (data.length == 0) ? null : data.last();
    }

    latestDateString() {
        let data = this.latestData();
        return data ?  util.toLocalMDHMSString(data.date) : "-";
    }

    createAssets () {
        let symEntity = this.createSymbolEntity();
        return new CameraAssets( symEntity, null);
    }

    createSymbolEntity () {
        let clr = this.hasRecentAlerts() ? config.alertColor : config.color;
        let data = this.latestData();
        let azimut = data ? data.azimut : 0;

        let entity = new Cesium.Entity({
            id: this.id,
            position: this.pos,
            billboard: {
                image: "./asset/odin_alertca/camera-sym.png",
                distanceDisplayCondition: config.billboardDC,
                color: clr,
                heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
                disableDepthTestDistance: Number.MAX_SAFE_INTEGER,  // otherwise symbol might get clipped by terrain
                alignedAxis: Cesium.Cartesian3.UNIT_Z,
                rotation: util.toRadians( 360 - azimut)
            },
            label: {
                text: this.label,
                //scale: 0.8,
                horizontalOrigin: Cesium.HorizontalOrigin.LEFT,
                verticalOrigin: Cesium.VerticalOrigin.TOP,
                font: config.labelFont,
                fillColor: clr,
                showBackground: true,
                backgroundColor: config.labelBackground,
                backgroundPadding: new Cesium.Cartesian2( 3,3),
                pixelOffset: config.labelOffset,
                distanceDisplayCondition: config.labelDC,
                heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
                disableDepthTestDistance:Number.MAX_SAFE_INTEGER,
            },
            point: {
                pixelSize: config.pointSize,
                color: clr,
                outlineColor: config.pointOutlineColor,
                outlineWidth: config.pointOutlineWidth,
                distanceDisplayCondition: config.pointDC,
            }
        });
        entity._uiCameraEntry = this; // backlink for selection

        cameraDataSource.entities.add(entity);
        return entity;
    }

    addFovEntity (data) {
        this.removeFovEntity(); // there can only be one

        let pos = this.pos;
        let pi2 = Math.PI/2;

        let hprFov = new Cesium.HeadingPitchRoll( util.toRadians( data.azimut) - pi2, 0, 0);
        let a2 = data.fovAngle / 2.0;
        let fovLeft = data.azimut - a2;
        let fovRight = data.azimut + a2;

        let entity = new Cesium.Entity({
            position: pos,
            orientation: Cesium.Transforms.headingPitchRollQuaternion( pos, hprFov),
            ellipsoid: {
                radii: new Cesium.Cartesian3( data.fovDist, data.fovDist, data.fovDist),
                innerRadii: new Cesium.Cartesian3(10.0, 10.0, 10.0),
                minimumClock: util.toRadians(-a2),
                maximumClock: util.toRadians(a2),
                minimumCone: pi2,
                maximumCone: pi2,
                material: config.fov.color,
                outline: true,
                outlineColor: config.fov.outlineColor,
                outlineWidth: 1,
                distanceDisplayCondition: config.fov.dc,
                zIndex: 1,
            }
        });

        this.assets.fov = entity;
        this.fovData = data; // the data item whose fov is shown
        cameraDataSource.entities.add(entity);
        return entity;
    }

    removeFovEntity () {
        if (this.assets.fov) {
            cameraDataSource.entities.remove(this.assets.fov); // there can only be one
            this.assets.fov = null;
        }
        this.fovData = null;
    }

    hasRecentAlerts () {
        return false; // not yet
    }

    // camera has not been updated for more than config.maxAlertAge seconds
    isInactive () {
        let d = this.latestData();
        return (!d || (Date.now() - d.date) > config.maxAlertAge * 1000);
    }

    removeSymbolEntityLabel () {
        this.assets.symbol.label = null;
    }

    replaceSymbolEntityLabel (newLabel) {
        this.assets.symbol.label.text = newLabel;
    }

    // merge a camera object from a snapshot or update message (data contains only new states for updates)
    update (camera) {
        if (camera.data) camera.data.forEach( d => this.data.push(d));
        if (camera.alerts) camera.alerts.forEach( a => this.alerts.push(a));
        this.maxFov = camera.maxFov;
        this.setLocalAttributes( camera);

        if (camera.name && camera.name !== this.name) {
            this.name = camera.name;
            this.label = camera.name;
            if (this.assets.symbol && this.assets.symbol.label) this.replaceSymbolEntityLabel( this.label);
        }

        if (!samePosition( this.position, camera.position)) { // camera has moved
            this.position = camera.position;
            this.pos = toCartesian3( camera.pos);
            if (this.assets.symbol) this.assets.symbol.position = this.pos;
            this.removeFovEntity(); // no longer valid
        }

        let d = this.latestData();
        if (d && this.assets.symbol) {
            this.assets.symbol.billboard.rotation = util.toRadians( 360 - d.azimut);
        }
    }
}

function toCartesian3 (p) {
    return new Cesium.Cartesian3( p.x, p.y, p.z);
}

function samePosition (p1, p2) {
    return p2 && (p1.lon == p2.lon) && (p1.lat == p2.lat) && (p1.alt == p2.alt);
}

/* #endregion types */

/* #region initialization ******************************************************************************/

var cameraEntries = new Map(); // id->Camera
var viewsheds = new Map(); // camera id -> { dataSource, show } (viewsheds are static - we keep them across snapshots)

var cameraView = undefined;
var selectedCameraEntry = null;

var dataView = undefined;
var alertView = undefined;

var showLatestCb = undefined;
var latestImage = undefined; // { ce, data, window } of the image window that is automatically shown in "show latest" mode
var showFovCb = undefined;
var latestFovEntry = undefined; // the camera entry that shows the fov of its latest image ("show latest" + "show fov" mode)

// interactively changeable render parameters (initialized from config, see "layer parameters" panel)
const TRACK_WIDTHS = [1, 1.5, 2, 3, 4, 5];
const RANGE_WIDTHS = [2, 3, 4, 6, 8, 10];
const VSH_ALPHAS = [0.1, 0.2, 0.25, 0.3, 0.4, 0.5, 0.6];

var render = {
    trackColor: config.viewTrack.color,
    trackWidth: config.viewTrack.width,
    rangeColor: config.viewTrack.rangeColor,
    rangeWidth: config.viewTrack.rangeWidth,
    vshColor: config.vsh.color,
    vshAlpha: config.vsh.alpha
};

createIcon();
createWindow();

//await odinCesium.viewerReadyPromise;
var cameraDataSource = odinCesium.createDataSource("alertCA", config.layer.show);

initCameraView();
initDataView();
initAlertsView();
initLayerParameters();

odinCesium.setEntitySelectionHandler( cameraSelection);
odinCesium.initLayerPanel("alert-ca", config, showAlertCa);

setInterval( () => ui.updateListItems( cameraView), 60000); // update inactive status of cameras
console.log("odin_alertca initialized");

/* #endregion initialization */

function createIcon() {
    return ui.Icon("./asset/odin_alertca/alertca.svg", (e)=> ui.toggleWindow(e,'alertca'), "Alert CA cameras");
}

function createWindow() {
    return ui.Window("Alert-CA Cameras", "alertca", "./asset/odin_alertca/alertca.svg")(
        ui.LayerPanel("alert-ca"),

        ui.Panel("cameras", true) (
            (cameraView = ui.List("alertca.cameras", 10, selectCameraEntry,null,null,zoomToCamera)),
        ),
        ui.Panel("data", true)(
            ui.RowContainer("start")(
                ui.ColumnContainer("start")(
                    (showLatestCb = ui.CheckBox("show latest", toggleShowLatest, "alertca.showLatest", false)),
                    (showFovCb = ui.CheckBox("show fov", toggleShowFovMode, "alertca.showFov", false))
                ),
                ui.HorizontalSpacer(0.5),
                (dataView = ui.List( "alertca.data", 10))
            )
        ),
        ui.Panel("alerts", true)(
            (alertView = ui.List( "alertca.alerts", 5))
        ),
        ui.Panel("layer parameters", false)(
            ui.RowContainer()(
                ui.Label("track", null, "4rem"),
                ui.ColorField("color", "alertca.track.color", true, "2rem", { changeAction: trackColorChanged }),
                ui.Choice("width", "alertca.track.width", trackWidthChanged, "3rem"),
                ui.HorizontalSpacer(0.5),
                ui.ColorField("range", "alertca.range.color", true, "2rem", { changeAction: rangeColorChanged }),
                ui.Choice("width", "alertca.range.width", rangeWidthChanged, "3rem")
            ),
            ui.RowContainer()(
                ui.Label("viewshed", null, "4rem"),
                ui.ColorField("color", "alertca.vsh.color", true, "2rem", { changeAction: vshColorChanged }),
                ui.Choice("alpha", "alertca.vsh.alpha", vshAlphaChanged, "3rem")
            ),
            ui.Button("reset", resetLayerParameters)
        )
    );
}

//--- layer parameters

function initLayerParameters() {
    ui.setField( ui.getField("alertca.track.color"), render.trackColor.toCssHexString());
    ui.setChoiceItems( "alertca.track.width", TRACK_WIDTHS, closestIndex( TRACK_WIDTHS, render.trackWidth));
    ui.setField( ui.getField("alertca.range.color"), render.rangeColor.toCssHexString());
    ui.setChoiceItems( "alertca.range.width", RANGE_WIDTHS, closestIndex( RANGE_WIDTHS, render.rangeWidth));
    ui.setField( ui.getField("alertca.vsh.color"), render.vshColor.toCssHexString());
    ui.setChoiceItems( "alertca.vsh.alpha", VSH_ALPHAS, closestIndex( VSH_ALPHAS, render.vshAlpha));
}

function resetLayerParameters() {
    render.trackColor = config.viewTrack.color;
    render.trackWidth = config.viewTrack.width;
    render.rangeColor = config.viewTrack.rangeColor;
    render.rangeWidth = config.viewTrack.rangeWidth;
    render.vshColor = config.vsh.color;
    render.vshAlpha = config.vsh.alpha;

    initLayerParameters();
    updateViewTrackRendering();
    updateViewshedRendering();
}

function closestIndex (values, v) {
    let idx = 0;
    values.forEach( (x,i) => { if (Math.abs(x - v) < Math.abs(values[idx] - v)) idx = i; });
    return idx;
}

function selectedChoiceNumber (event) {
    return Number( ui.getSelectedChoiceValue( ui.getChoice(event.target)));
}

function trackColorChanged (event) {
    let clrSpec = event.target.value;
    if (clrSpec) {
        render.trackColor = Cesium.Color.fromCssColorString(clrSpec);
        updateViewTrackRendering();
    }
}

function trackWidthChanged (event) {
    render.trackWidth = selectedChoiceNumber(event);
    updateViewTrackRendering();
}

function rangeColorChanged (event) {
    let clrSpec = event.target.value;
    if (clrSpec) {
        render.rangeColor = Cesium.Color.fromCssColorString(clrSpec);
        updateViewTrackRendering();
    }
}

function rangeWidthChanged (event) {
    render.rangeWidth = selectedChoiceNumber(event);
    updateViewTrackRendering();
}

function vshColorChanged (event) {
    let clrSpec = event.target.value;
    if (clrSpec) {
        render.vshColor = Cesium.Color.fromCssColorString(clrSpec);
        updateViewshedRendering();
    }
}

function vshAlphaChanged (event) {
    render.vshAlpha = selectedChoiceNumber(event);
    updateViewshedRendering();
}

function vshMaterial() {
    return render.vshColor.withAlpha( render.vshAlpha);
}

// apply current render parameters to all loaded viewsheds
function updateViewshedRendering() {
    let material = vshMaterial();
    for (const vsh of viewsheds.values()) {
        if (vsh.dataSource) {
            for (const e of vsh.dataSource.entities.values) {
                if (e.polygon) e.polygon.material = material;
            }
        }
    }
    odinCesium.requestRender();
}

function initCameraView() {
    let view = ui.getList("alertca.cameras");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "", width: "2.2rem", attrs: [], map: e => e.hasRecentAlerts() ?  "⚠️" : ""},
            { name: "name", width: "12rem", attrs: ["alignLeft"], map: e => util.maxString( e.name, 24) },
            ui.listItemSpacerColumn(0.5),
            { name: "vsh", tip: "show viewshed", width: "1.8rem", attrs: [], map: e => ui.createCheckBox( isViewshedShown(e.id), toggleShowViewshed) },
            { name: "stat", tip: "not updated within max alert age", width: "1.8rem", attrs:["alignRight"], map: e => e.isInactive() ? "🔺" : "✓" },
            { name: "data", tip: "number of data points", width: "2.5rem", attrs: ["fixed", "alignRight"], map: e => e.data.length },
            ui.listItemSpacerColumn(1.0),
            { name: "last report", width: "8.5rem", attrs: ["fixed", "alignRight"], map: e => e.latestDateString() }
        ])
    }
}

function initDataView() {
    let view = ui.getList("alertca.data");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "show", tip: "show image", width: "3rem", attrs: [], map: e => ui.createCheckBox(e.window, toggleShowImage) },
            { name: "fov", tip: "show fov", width: "3rem", attrs: [], map: e => ui.createCheckBox(isFovShown(e), toggleShowFov) },
            ui.listItemSpacerColumn(0.5),
            { name: "dir", tip: "camera direction", width: "2.5rem", attrs: ["fixed", "alignRight"], map: e => util.f_0.format( e.azimut) },
            { name: "zoom", tip: "zoom factor (only for PTZ cameras)", width: "3rem", attrs: ["fixed", "alignRight"], map: e => (e.zoom !== undefined) ? util.f_1.format( e.zoom) : "-" },
            ui.listItemSpacerColumn(0.5),
            { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.date) }
        ])
    }
}

function initAlertsView() {
    let view = ui.getList("alertca.alerts");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "alert", width: "15rem", attrs:[], map: e => e.alert },

            ui.listItemSpacerColumn(),
            { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.date) }
        ])
    }
}

function toggleShowImage(event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let data = ui.getListItemOfElement(cb);
        if (data) {
            if (data.window) {
                ui.removeWindow(data.window);
                data.window = null;
            } else {
                if (isShowLatest()) setShowLatest(false); // explicit image selection disables "show latest" mode

                setTimeout(() => { // otherwise the mouseUp will put the focus back on dataView
                    let uri = "./alertca-data/" + data.image;
                    let ce = selectedCameraEntry;
                    let w = wnd.ImageWindow(
                        imageTitle(data), null,
                        () => { ui.setCheckBox(cb, false); clearViewTrack(); },
                        uri, "",
                        config.imageWidth, config.imageHeight,
                        event.clientX + 10, event.clientY + 10,
                        undefined, undefined, undefined, // default scale range
                        (pos) => trackViewpoint( ce, data, pos)
                    );
                    data.window = w;
                }, 0);
            }
        }
    }
}

//--- "show latest" mode: automatically show the latest image of the selected camera. This is mutually exclusive with
//    explicitly shown images (dataView "show" checkboxes) to avoid automatically appearing image viewers

function isShowLatest() {
    return ui.isCheckBoxSelected( showLatestCb);
}

function setShowLatest (cond) {
    ui.setCheckBox( showLatestCb, cond);
    if (!cond) closeLatestImage();
}

function toggleShowLatest (event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        if (ui.isCheckBoxSelected(cb)) {
            closeExplicitImages(); // uncheck "show" items
            showLatestImage();
        } else {
            closeLatestImage();
        }
    }
}

// show the latest image of the selected camera if we are in "show latest" mode (and its fov in "show fov" mode)
function showLatestImage() {
    showLatestImageWindow();
    updateLatestFov();
}

// if we already have a latest image window we just update its image, which keeps window position, size, scale and
// scroll offsets
function showLatestImageWindow() {
    if (!isShowLatest() || !selectedCameraEntry) return;

    let data = selectedCameraEntry.latestData();
    if (!data || !data.image) return;
    if (latestImage && Object.is( latestImage.data, data)) return; // already showing

    let uri = "./alertca-data/" + data.image;
    if (latestImage) {
        latestImage.window.setImage( uri, imageTitle(data));
        latestImage.ce = selectedCameraEntry;
        latestImage.data = data;

    } else {
        let x, y;
        let r = ui.getWindowRect("alertca");
        if (r) { // place it to the right of our window
            x = r.right + 10;
            y = r.top;
        }

        let w = wnd.ImageWindow(
            imageTitle(data), null,
            () => latestImageClosed(w),
            uri, "",
            config.imageWidth, config.imageHeight,
            x, y,
            undefined, undefined, undefined, // default scale range
            (pos) => { if (latestImage) trackViewpoint( selectedCameraEntry, latestImage.data, pos); } // data changes with image
        );
        latestImage = { ce: selectedCameraEntry, data, window: w };
    }
}

function closeLatestImage() {
    if (latestImage) {
        let w = latestImage.window;
        latestImage = undefined; // before we remove it so that latestImageClosed() knows this was not a user action
        ui.removeWindow(w);
        updateLatestFov();
    }
}

// closeAction of latest image windows. If the user closes the window we turn off "show latest" mode
function latestImageClosed (w) {
    clearViewTrack();
    if (latestImage && Object.is( latestImage.window, w)) {
        latestImage = undefined;
        ui.setCheckBox( showLatestCb, false);
        updateLatestFov();
    }
}

// close all image windows that were explicitly opened with dataView "show" checkboxes
function closeExplicitImages() {
    for (let ce of cameraEntries.values()) {
        ce.data.forEach( (data) => {
            if (data.window) {
                let w = data.window;
                data.window = null;
                ui.removeWindow(w);
            }
        });
    }
    ui.updateListItems( dataView);
}

function imageTitle (data) {
    if (selectedCameraEntry) {
        return `${selectedCameraEntry.label} -- ${util.toLocalMDHMSString(data.date)} -- ${data.azimut}°`;
    }
}

// "show fov" mode: automatically show the fov of the latest image (only effective in "show latest" mode)
function isShowFov() {
    return ui.isCheckBoxSelected( showFovCb);
}

function toggleShowFovMode (event) {
    updateLatestFov();
}

// show the fov of the latest image if "show latest" and "show fov" are set, otherwise remove a previous latest fov.
// Note the latest image might belong to another camera than the selected one if the selected one has no image
function updateLatestFov() {
    let ce = (isShowFov() && latestImage) ? latestImage.ce : undefined;

    if (latestFovEntry && !Object.is( latestFovEntry, ce)) {
        latestFovEntry.removeFovEntity();
        latestFovEntry = undefined;
    }
    if (ce) {
        if (!Object.is( ce.fovData, latestImage.data)) ce.addFovEntity( latestImage.data);
        latestFovEntry = ce;
    }

    ui.updateListItems( dataView); // fov checkboxes
    odinCesium.requestRender();
}

function isFovShown (data) {
    return selectedCameraEntry ? Object.is( selectedCameraEntry.fovData, data) : false;
}

function toggleShowFov (event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let data = ui.getListItemOfElement(cb);
        if (data && selectedCameraEntry) {
            if (ui.isCheckBoxSelected(cb)) {
                selectedCameraEntry.addFovEntity(data);
            } else {
                selectedCameraEntry.removeFovEntity();
            }
            ui.updateListItems( dataView); // there is only one fov per camera, i.e. other fov checkboxes might change
        }
        odinCesium.requestRender();
    }
}

//--- viewsheds: per camera GeoJSON polygons that are computed offline (compute_vsh) and loaded on demand

function isViewshedShown (id) {
    let vsh = viewsheds.get(id);
    return vsh !== undefined && vsh.show;
}

function toggleShowViewshed (event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let ce = ui.getListItemOfElement(cb);
        if (ce) {
            let show = ui.isCheckBoxSelected(cb);
            let vsh = viewsheds.get(ce.id);
            if (vsh) {
                vsh.show = show;
                if (vsh.dataSource) vsh.dataSource.show = show && cameraDataSource.show;
                odinCesium.requestRender();
            } else if (show) {
                loadViewshed( ce, cb);
            }
        }
    }
}

async function loadViewshed (ce, cb) {
    let vsh = { dataSource: undefined, show: true };
    viewsheds.set( ce.id, vsh); // set before loading so that we don't load twice

    try {
        let response = await fetch( `./alertca-vsh/${ce.id}.geojson`);
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        let data = await response.json();

        // no polygon height - otherwise polygons are not draped over terrain
        let ds = await Cesium.GeoJsonDataSource.load( data, { clampToGround: true });
        for (const e of ds.entities.values) {
            if (e.polygon) {
                e.polygon.material = vshMaterial();
                e.polygon.outline = false;
            }
        }

        vsh.dataSource = ds;
        ds.show = vsh.show && cameraDataSource.show; // might have been toggled while loading
        odinCesium.addDataSource( ds);
        odinCesium.requestRender();

    } catch (e) {
        console.log(`no viewshed for camera ${ce.id}: ${e}`);
        viewsheds.delete( ce.id);
        ui.setCheckBox( cb, false);
    }
}

function updateViewshedVisibility() {
    for (const vsh of viewsheds.values()) {
        if (vsh.dataSource) vsh.dataSource.show = vsh.show && cameraDataSource.show;
    }
}

// list -> entity
function selectCameraEntry(event) {
    let ce = event.detail.curSelection;
    if (ce) {
        selectedCameraEntry = ce;

        ui.setListItems( dataView, ce.data.toReversed());
        ui.setListItems( alertView, ce.alerts.toReversed());
        showLatestImage();
    }
}

// entity -> list
function cameraSelection() {
    let sel = odinCesium.getSelectedEntity();
    if (sel && sel._uiCameraEntry) {
        if (!Object.is( sel._uiCameraEntry,selectedCameraEntry)) {
            ui.setSelectedListItem(cameraView, sel._uiCameraEntry);
        }
    }
}

function showAlertCa (cond) { // triggered by panel
    cameraDataSource.show = cond;
    updateViewshedVisibility();
    odinCesium.requestRender();
}

function zoomToCamera (event) {
    let lv = ui.getList(event);
    if (lv) {
        let ce = ui.getSelectedListItem(lv);
        if (ce) {
            let position = ce.position;
            let pos = Cesium.Cartographic.fromDegrees( position.lon, position.lat, config.zoomHeight);

            odinCesium.zoomTo( Cesium.Cartographic.toCartesian(pos));
            odinCesium.setSelectedEntity(ce.assets.symbol);
        }
    }
}


/* #region websock handlers *****************************************************************************/

function handleWsMessages(msgType, msg) {
    switch (msgType) {
        case "snapshot": handleSnapshotMsg(msg); break;
        case "update": handleUpdateMsg(msg); break;
        // ..alerts to follow
    }
}

// snapshots are complete replacements - we get one for each (re-)connect and server side data initialization,
// i.e. this has to remove all previous state (otherwise we would get duplicated entities)
function handleSnapshotMsg(msg) {
    closeLatestImage(); // belongs to a camera we remove
    cameraDataSource.entities.removeAll();
    cameraEntries.clear();
    selectedCameraEntry = null;
    ui.clearList( dataView);
    ui.clearList( alertView);

    msg.cameras.forEach( (camera) => {
        let ce = new CameraEntry( camera);
        ce.date = msg.date;
        cameraEntries.set( ce.id, ce);
    });

    deDuplicateCameraEntries(); // for cameras at the same location
    setCameraViewItems();
}

function setCameraViewItems() {
    let cameraList = [...cameraEntries.values()].sort( sortCameras);
    ui.setListItems( cameraView, cameraList);
    if (selectedCameraEntry) ui.setSelectedListItem( cameraView, selectedCameraEntry);
}

function deDuplicateCameraEntries() {
    for (let ce of cameraEntries.values()) {
        if (!ce.isGroup) { // don't process group members twice
            let lbl = ce.label;
            let last = lbl.charAt( lbl.length-1);
            if (last > "0" && last <= "9") { // check if there is a same position 1
                let ces = Array.from(cameraEntries.values()).filter( (o) => samePos(ce.pos,o.pos)).sort( sortCameras);
                if (ces.length > 1) {
                    let lbl = ces[0].label;
                    let last = lbl.charAt( lbl.length-1);
                    ces[0].replaceSymbolEntityLabel(((last > "0" && last <= "9") ? lbl.substring(0,lbl.length-1).trim() : lbl));
                    ces[0].isGroup = true;

                    for (let i=1; i<ces.length; i++) {
                        ces[i].removeSymbolEntityLabel();
                        ces[i].isGroup = true;
                    }
                }
            }
        }
    }
}

function samePos (pos1, pos2) {
    return (pos1.x == pos2.x) && (pos1.y == pos2.y) && (pos1.z == pos2.z);
}

function sortCameras (a,b) {
    if (a.name < b.name) return -1;
    else if (a.name > b.name) return 1;
    else return 0;
}

// changes contain the same camera objects as snapshots, but data only contains new states. Changes can also contain
// cameras we did not have before
function handleUpdateMsg (msg) {
    let hasNewCameras = false;

    msg.changes.forEach( (camera) => {
        let ce = cameraEntries.get( camera.id);
        if (ce) {
            ce.date = msg.date;
            ce.update( camera);
            ui.updateListItem( cameraView, ce);

            if (Object.is( selectedCameraEntry, ce)) {
                ui.setListItems( dataView, ce.data.toReversed());
                if (camera.alerts && camera.alerts.length > 0) ui.setListItems( alertView, ce.alerts.toReversed());
                showLatestImage();
            }

        } else { // new camera
            ce = new CameraEntry( camera);
            ce.date = msg.date;
            cameraEntries.set( ce.id, ce);
            hasNewCameras = true;
        }
    });

    if (hasNewCameras) {
        deDuplicateCameraEntries();
        setCameraViewItems();
    } else {
        ui.updateListItems( cameraView);
    }
    odinCesium.requestRender();
}

/* #endregion websock handlers */

/* #region viewpoint tracking **************************************************************************/

// image windows report tracked (relative) image positions, which we turn into a bearing line from the camera position
// (horizontal position) and - for cameras that report their tilt - a terrain intersection range (vertical position).
// Since distances are very sensitive to pitch errors at grazing angles we show the range between the terrain
// intersections for pitch +- config.viewTrack.pitchUncertainty instead of a single point

var viewTrack = undefined;  // { bearing, range, hit, angle } Cesium entities
var viewTrackSeq = 0;       // to discard outdated (async) terrain intersection results
var viewTrackTimer = undefined;

function trackViewpoint (ce, data, pos) {
    if (!pos || !ce || !data) {
        clearViewTrack();
        return;
    }

    let hfov = data.fovAngle;
    let bearing = (data.azimut + util.toDegrees( Math.atan( pos.x * Math.tan( util.toRadians( hfov/2)))) + 360) % 360;
    let p0 = ce.position;
    let p1 = destination( p0.lon, p0.lat, bearing, config.viewTrack.maxDist);

    ensureViewTrack();
    viewTrack.bearing.polyline.positions = Cesium.Cartesian3.fromDegreesArray([ p0.lon, p0.lat, p1.lon, p1.lat ]);
    viewTrack.bearing.show = true;
    viewTrack.angle.position = Cesium.Cartesian3.fromDegrees( p0.lon, p0.lat);
    viewTrack.angle.label.text = viewAngleText( bearing); // pitch is added by computeViewRange()
    viewTrack.angle.show = true;
    viewTrack.range.show = false;
    viewTrack.hit.show = false;
    odinCesium.requestRender();

    // terrain intersections are expensive - wait until the mouse rests (or immediately for frozen positions)
    let seq = ++viewTrackSeq;
    if (viewTrackTimer) clearTimeout( viewTrackTimer);
    if (data.tilt !== undefined) {
        viewTrackTimer = setTimeout( () => {
            computeViewRange( seq, ce, data, pos, bearing).catch( (err) => console.log("view range computation failed: ", err));
        }, pos.isFrozen ? 0 : config.viewTrack.delay);
    } else { // make it obvious why there is no distance
        viewTrack.hit.position = Cesium.Cartesian3.fromDegrees( p1.lon, p1.lat);
        viewTrack.hit.label.text = "no camera tilt data";
        viewTrack.hit.show = true;
    }
}

async function computeViewRange (seq, ce, data, pos, bearing) {
    let hfov = util.toRadians( data.fovAngle);
    let vfov = 2 * Math.atan( Math.tan( hfov/2) * pos.height / pos.width); // assuming square pixels
    let pitch = data.tilt + ce.tiltOffset + util.toDegrees( Math.atan( pos.y * Math.tan( vfov/2)));
    let u = config.viewTrack.pitchUncertainty;
    let maxDist = config.viewTrack.maxDist;

    let p0 = ce.position;
    let pitches = [pitch, pitch - u, pitch + u]; // hit, near, far

    let hits = (config.viewTrack.serverSide && !serverViewTrackFailed) ? await serverTerrainIntersections(ce, bearing, pitches, maxDist) : undefined;
    if (!hits) hits = await clientTerrainIntersections( ce, bearing, pitches, maxDist);
    let [hit, near, far] = hits;
    if (seq != viewTrackSeq || !viewTrack) return; // outdated

    viewTrack.angle.label.text = viewAngleText( bearing, pitch);

    if (near) {
        let pFar = far ? far.position : Cesium.Cartographic.fromDegrees( ...lonLatOf( destination( p0.lon, p0.lat, bearing, maxDist)));
        viewTrack.range.polyline.positions = [ Cesium.Cartographic.toCartesian( near.position), Cesium.Cartographic.toCartesian( pFar) ];
        viewTrack.range.show = true;
    }
    if (hit) {
        viewTrack.hit.position = Cesium.Cartographic.toCartesian(hit.position);
        let fd = far ? (far.distance / 1000).toFixed(1) : undefined;
        let nd = near ? (near.distance / 1000).toFixed(1) : undefined;
        let rangeText = "";
        if (near && far) {
            rangeText = ` ±${((far.distance - near.distance)/2000).toFixed(1)}`;
        } else if (near) {
            rangeText = ` -${((hit.distance - near.distance)/1000).toFixed(1)}`;
        } else {
            rangeText = ` +${((far.distance - hit.distance) / 1000).toFixed(1)}`;
        }

        viewTrack.hit.label.text = `${(hit.distance / 1000).toFixed(1)}${rangeText} km`;
        viewTrack.hit.show = true;

    } else if (!near) { // make it obvious that the ray (within maxDist) does not hit the terrain
        viewTrack.hit.position = Cesium.Cartesian3.fromDegrees( ...lonLatOf( destination( p0.lon, p0.lat, bearing, maxDist)));
        viewTrack.hit.label.text = "no terrain intersection";
        viewTrack.hit.show = true;
    }
    odinCesium.requestRender();
}

var serverViewTrackFailed = false; // set if the server cannot compute terrain intersections (e.g. no DEM configured)

// compute the terrain intersections of the camera view rays with the given pitches on the server (from the DEM
// configured in AlertCaConfig). Ray origin is the camera height above the DEM terrain at the camera position.
// Returns an array of { position: Cartographic, distance } or undefined entries, or undefined if the request failed
async function serverTerrainIntersections (ce, bearing, pitches, maxDist) {
    let p0 = ce.position;
    let queries = pitches.map( pitch => ({ lon: p0.lon, lat: p0.lat, height: ce.heightAboveGround, aboveGround: true, bearing, pitch, maxDist }));

    try {
        let response = await fetch( "./alertca-rays", {
            method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify( queries)
        });
        if (!response.ok) {
            let msg = await response.text();
            if (response.status == 503) { // no DEM on server - don't try again
                serverViewTrackFailed = true;
                console.warn("server side view tracking not available, using client side terrain intersections: ", msg);
            } else {
                console.warn("server side view tracking failed: ", msg);
            }
            return undefined;
        }
        let rayHits = await response.json();
        // heights are DEM (MSL) heights but we clamp hit positions to the (Cesium) terrain anyways
        return rayHits.map( h => h ? { position: Cesium.Cartographic.fromDegrees( h.lon, h.lat, 0), distance: h.distance } : undefined);

    } catch (err) {
        console.warn("server side view tracking request failed: ", err);
        return undefined;
    }
}

// compute the terrain intersections of the camera view rays with the given pitches in the browser (from Cesium terrain)
async function clientTerrainIntersections (ce, bearing, pitches, maxDist) {
    // the reported camera altitude is relative to MSL (and is the ground elevation) but Cesium terrain heights are
    // relative to the WGS84 ellipsoid (~30m lower in CA). We therefore use the terrain height at the camera position
    // plus the configured camera height above ground unless there is no topographic terrain
    let p0 = ce.position;
    let origin = Cesium.Cartographic.fromDegrees( p0.lon, p0.lat, 0);
    let th = await odinCesium.terrainHeight( origin);
    origin.height = ((th != 0) ? th : (p0.alt || 0)) + ce.heightAboveGround;

    return Promise.all( pitches.map( pitch => odinCesium.terrainIntersection( origin, bearing, pitch, maxDist)));
}

// bearing and (optional) pitch of the tracked view ray
function viewAngleText (bearing, pitch) {
    let s = `${bearing.toFixed(1)}°`;
    if (pitch !== undefined) s += ` / ${pitch.toFixed(2)}°`;
    return s;
}

function ensureViewTrack() {
    if (!viewTrack) {
        let bearing = new Cesium.Entity({
            polyline: { positions: [], clampToGround: true, width: render.trackWidth, material: render.trackColor }
        });
        let range = new Cesium.Entity({
            polyline: { positions: [], clampToGround: true, width: render.rangeWidth, material: render.rangeColor }
        });
        let hit = new Cesium.Entity({
            point: {
                pixelSize: config.viewTrack.hitSize, color: config.viewTrack.hitColor,
                outlineColor: config.viewTrack.hitOutlineColor, outlineWidth: config.viewTrack.hitOutlineWidth,
                heightReference: Cesium.HeightReference.CLAMP_TO_GROUND, disableDepthTestDistance: Number.POSITIVE_INFINITY
            },
            label: {
                text: "", font: config.labelFont, fillColor: render.trackColor,
                showBackground: true, backgroundColor: config.labelBackground,
                horizontalOrigin: Cesium.HorizontalOrigin.LEFT, verticalOrigin: Cesium.VerticalOrigin.BOTTOM,
                pixelOffset: config.labelOffset, heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
                disableDepthTestDistance: Number.POSITIVE_INFINITY
            }
        });
        let angle = new Cesium.Entity({ // bearing (and pitch) of the view ray, shown above the camera position
            label: {
                text: "", font: config.labelFont, fillColor: render.trackColor,
                showBackground: true, backgroundColor: config.labelBackground,
                horizontalOrigin: Cesium.HorizontalOrigin.LEFT, verticalOrigin: Cesium.VerticalOrigin.BOTTOM,
                pixelOffset: config.viewTrack.angleOffset, heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
                disableDepthTestDistance: Number.POSITIVE_INFINITY
            }
        });
        [bearing, range, hit, angle].forEach( e => cameraDataSource.entities.add(e));
        viewTrack = { bearing, range, hit, angle };
    }
}

// apply current render parameters to the view track entities (if any)
function updateViewTrackRendering() {
    if (viewTrack) {
        viewTrack.bearing.polyline.material = render.trackColor;
        viewTrack.bearing.polyline.width = render.trackWidth;
        viewTrack.range.polyline.material = render.rangeColor;
        viewTrack.range.polyline.width = render.rangeWidth;
        viewTrack.hit.label.fillColor = render.trackColor;
        viewTrack.angle.label.fillColor = render.trackColor;
        odinCesium.requestRender();
    }
}

function clearViewTrack() {
    viewTrackSeq++;
    if (viewTrackTimer) { clearTimeout( viewTrackTimer); viewTrackTimer = undefined; }
    if (viewTrack) {
        [viewTrack.bearing, viewTrack.range, viewTrack.hit, viewTrack.angle].forEach( e => cameraDataSource.entities.remove(e));
        viewTrack = undefined;
        odinCesium.requestRender();
    }
}

// destination point (spherical earth) for given start point [deg], bearing [deg] and distance [m]
function destination (lon, lat, bearing, dist) {
    const R = 6371000.0;
    let d = dist / R;
    let b = util.toRadians( bearing);
    let la1 = util.toRadians( lat);
    let lo1 = util.toRadians( lon);
    let la2 = Math.asin( Math.sin(la1) * Math.cos(d) + Math.cos(la1) * Math.sin(d) * Math.cos(b));
    let lo2 = lo1 + Math.atan2( Math.sin(b) * Math.sin(d) * Math.cos(la1), Math.cos(d) - Math.sin(la1) * Math.sin(la2));
    return { lon: util.toDegrees( lo2), lat: util.toDegrees( la2) };
}

function lonLatOf (p) { return [p.lon, p.lat]; }

/* #endregion viewpoint tracking */
