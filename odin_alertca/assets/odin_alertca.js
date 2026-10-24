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
    constructor(symbol, viewshed) {
        this.symbol = symbol;
        this.viewshed = viewshed;
    }

    showAssets (cond) {
        if (this.symbol) this.symbol.show = cond;
        if (this.viewshed) this.viewshed.show = cond;
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

        this.data = data.CircularBuffer.fromArray( camera.data ? camera.data : [], config.maxHistory);
        this.alerts = data.CircularBuffer.fromArray( camera.alerts ? camera.alerts : [], config.maxHistory);

        this.assets = this.createAssets();
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
                //showBackground: true,
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
                material: config.fovColor,
                outline: true,
                outlineColor: config.fovOutlineColor,
                outlineWidth: 1,
                distanceDisplayCondition: config.fovDC,
                zIndex: 1,
            }
        });

        this.assets.viewshed = entity;
        cameraDataSource.entities.add(entity);
        return entity;
    }

    removeFovEntity () {
        if (this.assets.viewshed) {
            cameraDataSource.entities.remove(this.assets.viewshed); // there can only be one
            this.assets.viewshed = null;
        }
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

var cameraView = undefined;
var selectedCameraEntry = null;

var dataView = undefined;
var alertView = undefined;

var showLatestCb = undefined;
var latestImage = undefined; // { data, window } of the image window that is automatically shown in "show latest" mode

createIcon();
createWindow();

//await odinCesium.viewerReadyPromise;
var cameraDataSource = odinCesium.createDataSource("alertCA", config.layer.show);

initCameraView();
initDataView();
initAlertsView();

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
        ui.LayerPanel("alert-ca", toggleShowAlertCa),

        ui.Panel("cameras", true) (
            (cameraView = ui.List("alertca.cameras", 10, selectCameraEntry,null,null,zoomToCamera)),
        ),
        ui.Panel("data", true)(
            ui.RowContainer("start")(
                (showLatestCb = ui.CheckBox("show latest", toggleShowLatest, "alertca.showLatest", false)),
                ui.HorizontalSpacer(0.5),
                (dataView = ui.List( "alertca.data", 10))
            )
        ),
        ui.Panel("alerts", true)(
            (alertView = ui.List( "alertca.alerts", 5))
        )
    );
}

function initCameraView() {
    let view = ui.getList("alertca.cameras");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "", width: "2.2rem", attrs: [], map: e => e.hasRecentAlerts() ?  "⚠️" : ""},
            { name: "name", width: "12rem", attrs: ["alignLeft"], map: e => util.maxString( e.name, 24) },
            ui.listItemSpacerColumn(0.5),
            { name: "stat", tip: "not updated within max alert age", width: "2rem", attrs:["alignRight"], map: e => e.isInactive() ? "🔺" : "✓" },
            { name: "data", tip: "number of data points", width: "3rem", attrs:["fixed", "alignRight"], map: e => e.data.length },
            ui.listItemSpacerColumn(0.5),
            { name: "last report", width: "9rem", attrs: ["fixed", "alignRight"], map: e => e.latestDateString() }
        ])
    }
}

function initDataView() {
    let view = ui.getList("alertca.data");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "show", tip: "show image", width: "3rem", attrs: [], map: e => ui.createCheckBox(e.window, toggleShowImage) },
            { name: "fov", tip: "show fov", width: "3rem", attrs: [], map: e => ui.createCheckBox(e.fovAsset, toggleShowFov) },
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
                    let w = wnd.ImageWindow(
                        imageTitle(data), null,
                        () => { ui.setCheckBox(cb, false); },
                        uri, "",
                        config.imageWidth, config.imageHeight,
                        event.clientX + 10, event.clientY + 10
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

// show the latest image of the selected camera if we are in "show latest" mode. If we already have a latest image window
// we just update its image, which keeps window position, size, scale and scroll offsets
function showLatestImage() {
    if (!isShowLatest() || !selectedCameraEntry) return;

    let data = selectedCameraEntry.latestData();
    if (!data || !data.image) return;
    if (latestImage && Object.is( latestImage.data, data)) return; // already showing

    let uri = "./alertca-data/" + data.image;
    if (latestImage) {
        latestImage.window.setImage( uri, imageTitle(data));
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
            x, y
        );
        latestImage = { data, window: w };
    }
}

function closeLatestImage() {
    if (latestImage) {
        let w = latestImage.window;
        latestImage = undefined; // before we remove it so that latestImageClosed() knows this was not a user action
        ui.removeWindow(w);
    }
}

// closeAction of latest image windows. If the user closes the window we turn off "show latest" mode
function latestImageClosed (w) {
    if (latestImage && Object.is( latestImage.window, w)) {
        latestImage = undefined;
        ui.setCheckBox( showLatestCb, false);
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
        }
        odinCesium.requestRender();
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
    odinCesium.requestRender();
}

function toggleShowAlertCa(event) { // show action triggered by layer view (not panel)
    cameraDataSource.show = !cameraDataSource.show;
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
