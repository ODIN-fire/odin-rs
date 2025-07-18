/**
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

// the ecmascript module that is our CesiumJS interface. Not this is an async module


import { config } from "./odin_cesium_config.js";
import * as main from "../odin_server/main.js";
import * as util from "../odin_server/ui_util.js";
import * as data from "../odin_server/ui_data.js";
import * as ui from "../odin_server/ui.js";
import * as ws from "../odin_server/ws.js";

const MOD_PATH = "odin_cesium::CesiumService";
const VIEW_PATTERN = util.glob2regexp("{view/**,**/view/**,**/view}");

ws.addWsHandler( MOD_PATH, handleWsMessages);

// initialize share interface of this module
main.addShareHandler( handleShareMessage);
main.addShareEditor( "GeoPoint3", "current view", withCurrentCameraPosition);
main.addSyncHandler( handleSyncMessage);

setCesiumContainerVisibility(false); // don't render before everybody is initialized

const UI_POSITIONS = "race-ui-positions";
const LOCAL = "local-";  // prefix for local position set names

class LayerEntry {
    constructor (wid,layerConfig,showAction) {
        this.id = layerConfig.name;    // (unique) full path: /cat/.../name

        let p = util.matchPath(this.id);
        this.name = p[p.length-1]; // last path element
        this.category = p[1]; // the first path element (p[0] is the whole match)

        this.config = layerConfig;     // at minimum {name,description,show}
        this.show = layerConfig.show;  // the configured initial state
        this.showAction = showAction;   // module provided function to toggle visibility of assets

        this.modulePanelCb = undefined;
        this.layerOrderCb = undefined;
    }

    setVisible(showIt) {
        this.show = showIt;
        this.showAction(showIt);
        ui.setCheckBox(this.modulePanelCb,showIt); // in the module window
        ui.setCheckBox(this.layerOrderCb, showIt);
    }
}

// we can't directly use SharedItems here since we have to add properties that are display/layer specific (assets, home)
class CameraPosition {
    constructor (key, lonDeg, latDeg, altM, isLocal=true, home=false) {
        this.key = key;
        this.lon = lonDeg;
        this.lat = latDeg;
        this.alt = altM;
        this.isLocal = isLocal;
        this.home = home;

        this.asset = undefined; // on-demand point entity
    }
}

// set when constructing elements and initialized through websocket message
var localClock = undefined;  // do we need a promise for it?
var utcClock = undefined; 

// a promise other modules can latch on to if they need an initialized utc clock
export const utcClockPromise = new Promise( (resolve,reject) => {
    let tid = setInterval(() => {
        if (ui.isClockSet(utcClock)) {
            console.log("clock running.");
            clearTimeout(tid);
            resolve(utcClock);
        }
    }, 2000); // check every 500 ms
});

export function withCurrentUtcMillis (f) {
    utcClockPromise.then( (utcClock)=>f( ui.getClockEpochMillis(utcClock)))
}

//export var viewer = undefined;

var cameraSpec = undefined;
var lastCamera = undefined; // saved last position & orientation

var requestRenderMode = config.requestRenderMode;
var pendingRenderRequest = false;
var targetFrameRate = -1;

var layerOrder = []; // populated by initLayerPanel calls from modules
var layerOrderView = undefined; // showing the registered module layers
var layerHierarchy = [];
var layerHierarchyView = undefined;

var mouseMoveHandlers = [];
var mouseDownHandlers = [];
var mouseUpHandlers = [];
var mouseClickHandlers = [];
var mouseDblClickHandlers = [];
var keyDownHandlers = [];
var terrainChangeHandlers = [];

var homePosition = undefined;
var initPosition = undefined;
var positions = new Map(); // list of known CameraPositions
var positionsView = undefined;

var isSelectedView = false;

var mapScale; // canvas to show map scale
export var isMetric = true;

const centerOrientation = {
    heading: Cesium.Math.toRadians(0.0),
    pitch: Cesium.Math.toRadians(-90.0),
    roll: Cesium.Math.toRadians(0.0)
};

if (Cesium.Ion.defaultAccessToken) {
    console.log("using configured Ion access token");
}

export const ellipsoidTerrainProvider = new Cesium.EllipsoidTerrainProvider();
var topoTerrainProvider = undefined; 

export const topoTerrainProviderPromise = (config.terrainProviderPromise ? config.terrainProviderPromise : Cesium.createWorldTerrainAsync()).then(
    (tp) => {
        topoTerrainProvider = tp;
        console.log("topo terrain provider initialized to ", tp);
    }
);

//Cesium.createWorldTerrainAsync().then( (tp) => {  // needs to be exported since other modules might chain on it
//    topoTerrainProvider = tp;
//    console.log("topo terrain provider initialized");
//});

export function withTopoTerrain (f) {
    topoTerrainProviderPromise.then( () => { f(); });
}

var terrainProvider = ellipsoidTerrainProvider; // this is our initial terrain as it is immediately available. Switched on demand in postInitialize

var osmBuildings = undefined; // OSM buildings 3D tileset loaded on demand

export const viewer = new Cesium.Viewer('cesiumContainer', {
    terrainProvider: terrainProvider,
    skyBox: false,
    infoBox: false,
    baseLayerPicker: false,  // if true primitives don't work anymore ?? 
    baseLayer: false,        // set during imageryService init
    sceneModePicker: true,
    navigationHelpButton: false,
    homeButton: false,
    timeline: false,
    animation: false,
    requestRenderMode: requestRenderMode,
});

checkImagery();

let dataSource = new Cesium.CustomDataSource("positions");
addDataSource(dataSource);

initTimeWindow();
initViewWindow();
initLayerWindow();
initMapScale();

// position fields
let cameraLat = ui.getField("view.camera.latitude");
let cameraLon = ui.getField("view.camera.longitude");
let cameraAlt = ui.getField("view.camera.altitude");
let pointerLat = ui.getField("view.pointer.latitude");
let pointerLon = ui.getField("view.pointer.longitude");
let pointerElev = ui.getField("view.pointer.elevation");
let pointerUtmN = ui.getField("view.pointer.utmN");
let pointerUtmE = ui.getField("view.pointer.utmE");
let pointerUtmZ = ui.getField("view.pointer.utmZ");
let cameraName = ui.getField("view.camera.name");

setTargetFrameRate(config.targetFrameRate);
initFrameRateSlider();

if (requestRenderMode) ui.setCheckBox("view.rm", true);

setCanvasSize();
window.addEventListener('resize', setCanvasSize);

viewer.resolutionScale = window.devicePixelRatio; // 2.0
viewer.scene.fxaa = true;
//viewer.scene.globe.depthTestAgainstTerrain=true;

//showContext(); // for debugging purposes

Cesium.GeoJsonDataSource.clampToGround = true; // should this be configured?

// event listeners
viewer.camera.moveEnd.addEventListener(updateCamera);

registerMouseMoveHandler(updateMouseLocation);
viewer.scene.canvas.addEventListener('mousemove', handleMouseMove);
viewer.scene.canvas.addEventListener('mousedown', handleMouseDown);
viewer.scene.canvas.addEventListener('mouseup', handleMouseUp);
viewer.scene.canvas.addEventListener('click', handleMouseClick);
viewer.scene.canvas.addEventListener('dblclick', handleMouseDblClick);

document.addEventListener('keydown', handleKeyDown); // does not work on canvas
registerKeyDownHandler( globalKeyDownHandler); // global hotkeys
registerMouseClickHandler( globalMouseClickHandler); // global click

// FIXME - this seems to be broken as of Cesium 105.1
//viewer.scene.postRender.addEventListener(function() {
viewer.scene.preRender.addEventListener(function() {
    pendingRenderRequest = false;
});

setInitialViewPositions();
setInitialView();

console.log("ui_cesium initialized");

//--- end initialization

function showContext() {
    let canvas = viewer.canvas;
    let gl = canvas.getContext("webgl2");
    let scene = viewer.scene;
    console.log("webGL extensions: ", gl.getSupportedExtensions());
    console.log("clamp-to-height supportet:", scene.clampToHeightSupported);
    console.log("logarithmic depth buffer:", scene.logarithmicDepthBuffer, ", far/near ratio:", scene.logarithmicDepthFarToNearRatio);
}

//--- terrain handling

const ORTHO_PITCH = -Math.PI/2;
const TERRAIN_HEIGHT = 100000; // in meters

export function isOrthoView () {
    let pitch = viewer.camera.pitch;
    return Math.abs(ORTHO_PITCH - pitch) < 0.0005;
}

function toggleOsmBuildings (event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        if (ui.isCheckBoxSelected(cb)) {
            // ?? it seems we can't just re-add an already loaded tileset ??
            Cesium.createOsmBuildingsAsync( config.osmBuildingOpts).then( (tileset) => {
                osmBuildings = tileset;
                console.log("osmBuildings 3D tileset initialized");
                viewer.scene.primitives.add(tileset);
            })

        } else { // hide
            viewer.scene.primitives.remove( osmBuildings);
            console.log("osmBuildings 3D tileset removed");
            osmBuildings = null;
        }
    }
}

function useEllipsoidTerrain() {
    if (!isOrthoView()) {
        let height = viewer.camera.positionCartographic.height;
        return height > TERRAIN_HEIGHT;
    }
    return true;
}

export async function getTopoTerrainProvider() {
    return await topoTerrainProviderPromise;
}

function toggleTerrain(event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        if (ui.isCheckBoxSelected(cb)) {
            switchToTopoTerrain();
        } else {
            switchToEllipsoidTerrain();
        }
    }
}

function switchToEllipsoidTerrain() {
    if (!(terrainProvider === ellipsoidTerrainProvider)) {
        terrainProvider = ellipsoidTerrainProvider;
        console.log("switching to ellipsoid terrain");
        viewer.scene.terrainProvider = terrainProvider;
        handleTerrainChange();
        //requestRender();
    }
}

function switchToTopoTerrain() {
    if (terrainProvider === ellipsoidTerrainProvider) {
        topoTerrainProviderPromise.then( () => {
            terrainProvider = topoTerrainProvider;
            console.log("switching to topographic terrain");
            viewer.scene.terrainProvider = terrainProvider;

            ui.setCheckBox( "view.show_terrain", true);
            handleTerrainChange();
            requestRender();
        })
    }
} 

export function isUsingTopoTerrain() {
    return Object.is( terrainProvider, topoTerrainProvider);
}

export function registerTerrainChangeHandler (handler) {
    terrainChangeHandlers.push(handler);
}

export function releaseTerrainChangeHandler (handler) {
    let idx = terrainChangeHandlers.findIndex(h => h === handler);
    if (idx >= 0) terrainChangeHandlers.splice(idx,1);
}

function handleTerrainChange() {
    let e = viewer.scene.terrainProviderChanged;
    terrainChangeHandlers.forEach( h=> h(e));
}

//--- imagery

function checkImagery() {
    // TODO - check if this works since it is recursive
    import("./imglayer.js").catch((err) => {
        console.log("no imglayer configured, using default imagery");
        const imageryProvider = Cesium.ImageryLayer.fromWorldImagery({
            style: Cesium.IonWorldImageryStyle.AERIAL_WITH_LABELS
        });
        viewer.imageryLayers.add(imageryProvider);
    });
}

function initViewWindow() {
    createViewIcon();
    createViewWindow();
    positionsView = initPositionsView();
}

function createViewWindow() {
    let fieldOpts = {isFixed: true, isDisabled: true};

    return ui.Window("View", "view", "./asset/odin_cesium/camera.svg")(
        ui.RowContainer()(
            ui.CheckBox("metric", toggleIsMetric, null, isMetric),
            ui.CheckBox("fullscreen", toggleFullScreen),
            ui.HorizontalSpacer(1),
            ui.CheckBox("terrain", toggleTerrain, "view.show_terrain"),
            ui.CheckBox("OSM bldgs", toggleOsmBuildings, "view.show_bldgs"),
            ui.HorizontalSpacer(1),
            ui.Button("⟘", setDownView, 2.5),  // ⇩  ⊾ ⟘
            ui.Button("⌂", setHomeView, 2.5) // ⌂ ⟐ ⨁
          ),
          ui.RowContainer()(
            ui.TextInput("pointer [φ,λ,m]", "view.pointer.latitude", "5rem", fieldOpts),
            ui.TextInput("", "view.pointer.longitude", "6rem", fieldOpts),
            ui.TextInput("", "view.pointer.elevation", "5.5rem", fieldOpts),
            ui.HorizontalSpacer(0.4)
          ),
          ui.RowContainer()(
            ui.TextInput("UTM [N,E,z]", "view.pointer.utmN", "5rem", fieldOpts),
            ui.TextInput("", "view.pointer.utmE", "6rem", fieldOpts),
            ui.TextInput("", "view.pointer.utmZ", "5.5rem", fieldOpts),
            ui.HorizontalSpacer(0.4)
          ),
          ui.RowContainer()(
            ui.TextInput("camera", "view.camera.latitude", "5rem", {changeAction: setViewFromFields, isFixed: true}),
            ui.TextInput("", "view.camera.longitude", "6rem", {changeAction: setViewFromFields, isFixed: true}),
            ui.TextInput("", "view.camera.altitude", "5.5rem", {changeAction: setViewFromFields, isFixed: true}),
            ui.HorizontalSpacer(0.4)
          ),
          ui.TreeList("view.positions", 10, "30rem", setCameraFromSelection, setCameraName),
          ui.RowContainer()(
            ui.TextInput("name", "view.camera.name", "15rem", {isFixed: true, placeHolder: 'enter path of new view'}),
            ui.Button("pick", pickViewPoint),
            ui.Button("current", addCurrentView),
            ui.Button("del", removeView)
          ),
          ui.Panel("view parameters", false)(
            ui.CheckBox("render on-demand", toggleRequestRenderMode, "view.rm"),
            ui.Slider("frame rate", "view.fr", setFrameRate)
          )
    );
}

function initPositionsView() {
    let view = ui.getList("view.positions");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "", width: "1.5rem", attrs: [], map: e => !e.isLocal ? '\u{1f310}' : '' },
            { name: "pt", tip: "show/hide ground point", width: "1.5rem", attrs: [], map: e => ui.createCheckBox(e.asset, toggleShowPosition) },
            { name: "lat", tip: "latitude [deg]", width:  "4.5rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.lat,4)},
            { name: "lon", tip: "longitude [deg]", width:  "6.5rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.lon,4)},
            { name: "alt", tip: "altitude [km]", width:  "4rem", attrs: ["fixed", "alignRight"], map: e => Math.round(e.alt / 1000)}
        ]);
    }

    return view;
}

function createViewIcon() {
    return ui.Icon("./asset/odin_cesium/camera.svg", (e)=> ui.toggleWindow(e,'view'), "camera view");
}


/* #region time window ****************************************************************/

function initTimeWindow() {
    createTimeIcon();
    createTimeWindow();
}

function createTimeWindow() {
    let view = ui.Window("clock", "time", "./asset/odin_cesium/time.svg")(
        ui.Clock("time UTC", "time.utc", "UTC"),
        ui.Clock("time loc", "time.loc",  config.localTimeZone),
        ui.Timer("elapsed", "time.elapsed")
    );
    utcClock = ui.getClock("time.utc");
    localClock = ui.getClock("time.loc");

    return view;
}

function createTimeIcon() {
    return ui.Icon("./asset/odin_cesium/time.svg", (e)=> ui.toggleWindow(e,'time'), "clock");
}

/* #endregion time window */

/* #region layer window ***************************************************************/

function initLayerWindow() {
    createLayerIcon();
    createLayerWindow();
    layerOrderView = initLayerOrderView();
    layerHierarchyView = initLayerHierarchyView();
}

function createLayerWindow() {
    return ui.Window("module layers", "layer", "./asset/odin_cesium/layers.svg")(
        ui.Panel("module Z-order", true)(
            ui.List("layer.order", 10),
            ui.RowContainer()(
                ui.Button("↑", raiseModuleLayer),
                ui.Button("↓", lowerModuleLayer)
            )
        ),
        ui.Panel("module hierarchy", false)(
            ui.TreeList("layer.hierarchy", 15, "25rem")
        )
    );
}

function createLayerIcon() {
    return ui.Icon("./asset/odin_cesium/layers.svg", (e)=> ui.toggleWindow(e,'layer'), "map layers");
}

function initLayerOrderView() {
    let v = ui.getList("layer.order");
    if (v) {
        ui.setListItemDisplayColumns(v, ["fit", "header"], [
            { name: "", width: "2rem", attrs: [], map: e =>  setLayerOrderCb(e) },
            { name: "name", width: "8rem", attrs: [], map: e => e.name },
            { name: "cat", width: "10rem", attrs: [], map: e => e.category}
        ]);
    }
    return v;
}

/* #endregion layer window */

/* #region view position sets **********************************************************/

function toggleShowPosition(event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let pos = ui.getListItemOfElement(cb);
        if (pos) {
            if (ui.isCheckBoxSelected(cb)){
                if (!pos.asset) setPositionAsset(pos);
            } else {
                if (pos.asset) clearPositionAsset(pos);
            }
        }
    }
}

function addCurrentView() {
    let lon = Number.parseFloat(ui.getFieldValue("view.camera.longitude"));
    let lat = Number.parseFloat(ui.getFieldValue("view.camera.latitude"));
    let alt = Number.parseFloat(ui.getFieldValue("view.camera.altitude"));

    if (isNaN(lat) || isNaN(lon) || isNaN(alt)){
        alert("please enter valid latitude, longitude and altitude");
        return;
    }

    let key = ui.getFieldValue(cameraName);
    if (key && isValidViewKey(key)) {
        main.setSharedItem( key, "GeoPoint3", new main.GeoPoint3( lon, lat, alt), true); // we add this as a locally shared var and update from the sharedHandler
    } else alert("please enter valid view name: ", VIEW_PATTERN);
}

function pickViewPoint() {
    let btn = ui.getButton("view.pickPos");
    ui.setElementColors( btn, ui.getRootVar("--selected-data-color"), ui.getRootVar("--selection-background"));

    // system prompt blocks DOM manipulation so we need to defer the action
    setTimeout( ()=> {
        let key = sanitizeViewKey( ui.getFieldValue( cameraName));
        if (key && isValidViewKey(key)) {
            enterGeoPoint( (cp) => {
                if (cp) {
                    cp.alt = Number.parseFloat( ui.getFieldValue("view.camera.altitude"));
                    
                    ui.setField("view.pointer.latitude", cp.lat);
                    ui.setField("view.pointer.longitude", cp.lon);
                    
                    main.setSharedItem( key, "GeoPoint3", cp, true);

                }
                ui.resetElementColors(btn);
            });
        } else {
            alert("please enter valid view name: ", VIEW_PATTERN);
            ui.resetElementColors(btn);
        }
    }, 100);
}

function sanitizeViewKey(key) {
    if (key && key.startsWith("/")) {
        return key.substring(1);
    } else {
        return key;
    }
}

function isValidViewKey(key) {
    return key.match( VIEW_PATTERN);
}

function removeView() {
    let pos = ui.getSelectedListItem(positionsView);
    if (pos) {
        if (pos.isLocal) {
            main.removeSharedItem( pos.key);
        } else alert("only local views can be removed here");
    } else alert( "please select view to remove");
}

function getConfigViews() {
    return config.defaultViews.map( p=> new CameraPosition( p.key, p.default.lon, p.default.lat, p.default.alt, true, p.home));
}

function getSharedViews() {
    let views = [];
    let items = main.getAllMatchingSharedItems( VIEW_PATTERN);
    for (let item of items) {
        if (item.value.type == "GeoPoint3") {
            let p = item.value.data;
            views.push( new CameraPosition( item.key, p.lon, p.lat, p.alt, item.isLocal));
        }
    }

    return views;
}

function getQueryView() {
    let queryString = window.location.search;
    if (queryString.length > 0) {
        let params = new URLSearchParams(queryString);
        let view = params.get("view");
        if (view) {
            let elems = view.split(',');
            if (elems.length > 1) {
                try {
                    for (let i=0; i<elems.length; i++) {
                        elems[i] = parseFloat( elems[i], 10);
                    }
                    if (elems.length == 2) { // no height given
                        elems.push( 150000);
                    } else {
                        if (elems[2] < 10000) { // assume this is in km
                            elems[2] = elems[2] * 1000;
                        }
                    }
                    return new CameraPosition( "view/<initial>", elems[0], elems[1], elems[2], true); // name,lon,lat,alt,isLocal

                } catch (e) {
                    console.log("ignoring invalid initial position spec: ", view);
                }
            }
        }
    }
    return null;
}

function setInitialViewPositions() {
    let vps = getConfigViews();

    let home = vps.find( p=> p.home );
    homePosition = home ? home : vps[0];

    let queryView = getQueryView();
    if (queryView) {
        initPosition = queryView;
        vps.push(queryView);
    }
    
    // add all configured views as locally shared items (if they are not overriding existing shared items)
    vps.forEach( p => {
        if (!main.getSharedItem(p.key)) {
            main.setSharedItem(p.key, "GeoPoint3", new main.GeoPoint3( p.lon, p.lat, p.alt), true)
        }
    });

    positions = new Map();
    vps.forEach( p=> positions.set(p.key, p));

    let tree = data.ExpandableTreeNode.from( vps, e=>e.key );
    ui.setTree( positionsView, tree);
}

function updateSharedViewPositions() {
    let vps = getSharedViews();

    let newPositions = new Map();
    vps.forEach( p=> newPositions.set(p.key, p));
    positions = newPositions;

    updatePositionsView();
}

function updatePositionsView() {
    let tree = data.ExpandableTreeNode.from( positions, e=>e.key );
    //let tree = data.ExpandableTreeNode.from( positions.values(), e=>e.key );
    ui.setTree( positionsView, tree);
}

/* #endregion view list */

function filterAssets(k,v) {
    if (k === 'asset') return undefined;
    else return v;
}

function setPositionAsset(pos) {
    let cfg = config;

    let e = new Cesium.Entity({
        id: pos.key,
        position: Cesium.Cartesian3.fromDegrees( pos.lon, pos.lat),
        point: {
            pixelSize: cfg.pointSize,
            color: cfg.color,
            outlineColor: cfg.outlineColor,
            outlineWidth: 1,
            disableDepthTestDistance: Number.NEGATIVE_INFINITY
        },
        label: {
            text: pos.key,
            font: cfg.font,
            fillColor: cfg.outlineColor,
            showBackground: true,
            backgroundColor: cfg.labelBackground,
            //heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
            horizontalOrigin: Cesium.HorizontalOrigin.LEFT,
            verticalOrigin: Cesium.VerticalOrigin.TOP,
            pixelOffset: new Cesium.Cartesian2( 5, 5)
        }
    });
    pos.asset = e;
    dataSource.entities.add(e);
    requestRender();
}

function clearPositionAsset(pos) {
    if (pos.asset) {
        dataSource.entities.remove(pos.asset);
        pos.asset = undefined;
        requestRender();
    }
}

function initFrameRateSlider() {
    let e = ui.getSlider('view.fr');
    if (e) {
        ui.setSliderRange(e, 0.0, 60, 10, util.f_0);
        ui.setSliderValue(e, targetFrameRate);
    }
}

function setTargetFrameRate(fr) {
    targetFrameRate = fr;
    if (fr > 0) {
        viewer.targetFrameRate = targetFrameRate;
    } else {
        viewer.targetFrameRate = undefined; // whatever the browser default animation rate is
    }
}

function toggleIsMetric (event) {
    let cb = event.target;
    isMetric = ui.isCheckBoxSelected(cb);
    updateScale();
}

export function lowerFrameRateWhile(action, lowFr) {
    viewer.targetFrameRate = lowFr;
    action();
    viewer.targetFrameRate = targetFrameRate;
}

export function lowerFrameRateFor(msec, lowFr) {
    let curFr = viewer.targetFrameRate;
    viewer.targetFrameRate = lowFr;
    setTimeout(() => {
        viewer.targetFrameRate = curFr;
        requestRender();
    }, msec);
}

export function isRequestRenderMode() {
    return requestRenderMode;
}

var renderTimer = undefined;

// this is a workaround for missing scene updates (e.g. when rendering corridors)
function setRequestRenderTimer() {
    if (requestRender) {
        if (!renderTimer) {
            renderTimer = setInterval( () => {
                viewer.scene.requestRender();
            }, 1000);
        }
    } else {
        if (renderTimer) {
            clearInterval( renderTimer);
            renderTimer = undefined;
        }
    }
}

export function setRequestRenderMode (enable) {
    if (enable != requestRenderMode) {
        requestRenderMode = enable;
        console.log("set requestRender mode: ", requestRenderMode);
        viewer.scene.requestRenderMode = requestRenderMode;
        ui.setCheckBox("view.rm", requestRenderMode);
        setRequestRenderTimer();
    }
}

function toggleRequestRenderMode(event) {
    let cb = ui.getCheckBox("view.rm");
    if (cb) {
        let enable = ui.isCheckBoxSelected(cb);
        if (enable != requestRenderMode) {
            requestRenderMode = enable;
            console.log("set requestRender mode: ", requestRenderMode);
            viewer.scene.requestRenderMode = requestRenderMode;
            setRequestRenderTimer();
        }
    }
}

// if there is no pending scene rendering request issue one. Note this still is subject
// to not exceeding the target framerate of Cesium, i.e. it might not result in rendering
export function requestRender(force = false) {
    // TODO - this "double-tap" is a (not fully dependable) workaround for a race-condition in Cesium that might not have a fully 
    // updated scene when rendering, which can lead to missed updates 

    viewer.scene.requestRender();
    setTimeout( ()=>viewer.scene.requestRender(), 200);
    /*
    if (force || (requestRenderMode && !pendingRenderRequest)) {
        pendingRenderRequest = true;
        //viewer.scene.render();
        viewer.scene.requestRender();
        // TODO - this does not dependably trigger a redraw as of Cesium 1.126 (requires screen interaction - like moving mouse over canvas)
    }
    */
}

// imperative rendering
export function render() {
    //viewer.scene.render();
    viewer.render();
}

export async function withDetailedSampledTerrain(positions, action) {
    let nPoints = positions.length;

    if ( nPoints > 100) { // this tends to throw ERR_INSUFFICIENT_RESOURCES errors in Chrome, resulting in undefined heights
        for (let i=0; i<nPoints; i+=100) {
            let i1 = Math.min(i + 100, nPoints);
            let chunk = positions.slice( i, i1);
            await Cesium.sampleTerrainMostDetailed(viewer.terrainProvider, chunk);
        }
        action();

    } else {
        const promise = Cesium.sampleTerrainMostDetailed(viewer.terrainProvider, positions);
        Promise.resolve(promise).then(action);
    }
}

export function createScreenSpaceEventHandler() {
    return new Cesium.ScreenSpaceEventHandler(viewer.scene.canvas);
}

export function getInputAction(type,mod=null) {
    return viewer.screenSpaceEventHandler.getInputAction(type,mod);
}

export function removeInputAction(type,mod=null) {
    return viewer.screenSpaceEventHandler.removeInputAction(type,mod);
}

export function setInputAction(action,type,mod=null) {
    return viewer.screenSpaceEventHandler.setInputAction(action,type,mod);
}

export function setCursor(cssCursorSpec) {
    viewer.scene.canvas.style.cursor = cssCursorSpec;
}

export function setDefaultCursor() {
    viewer.scene.canvas.style.cursor = "default";
}

function setCanvasSize() {
    viewer.canvas.width = window.innerWidth;
    viewer.canvas.height = window.innerHeight;
}

export function setDoubleClickHandler (action) {
    let selHandler = new Cesium.ScreenSpaceEventHandler(viewer.scene.canvas);
    selHandler.setInputAction(action, Cesium.ScreenSpaceEventType.LEFT_DOUBLE_CLICK);
}

export function setEntitySelectionHandler(onSelect) {
    let selHandler = new Cesium.ScreenSpaceEventHandler(viewer.scene.canvas);
    selHandler.setInputAction(onSelect, Cesium.ScreenSpaceEventType.LEFT_CLICK);
}

export function addDataSource(dataSrc) {
    viewer.dataSources.add(dataSrc);
}

export function removeDataSource(dataSrc) {
    viewer.dataSources.remove(dataSrc);
}

export function toggleDataSource(dataSrc) {
    if (viewer.dataSources.contains(dataSrc)) {
        viewer.dataSources.remove(dataSrc);
    } else {
        viewer.dataSources.add(dataSrc);
    }
}

export function isDataSourceShowing(dataSrc) {
    return viewer.dataSources.contains(dataSrc);
}

export function addPrimitive(prim) {
    viewer.scene.primitives.add(prim);
}

export function addPrimitives(primitives) {
    let pc = viewer.scene.primitives;
    primitives.forEach( p=> pc.add(p));
    requestRender();
}

export function showPrimitive(prim, show) {
    prim.show = show;
    requestRender();
}
export function showPrimitives(primitives, show) {
    primitives.forEach( p=> p.show = show);
    requestRender();
}

export function removePrimitive(prim) {
    viewer.scene.primitives.remove(prim); // watch out - this destroys prim
}
export function removePrimitives(primitives) {
    let pc = viewer.scene.primitives;
    primitives.forEach( p=> pc.remove(p));
    requestRender();
}

export function showSelectionIndicator (cond) {
    let vis = cond ? 'visible' : 'hidden';
    viewer.selectionIndicator.viewModel.selectionIndicatorElement.style.visibility = vis;
}

export function clearSelectedEntity() {
    viewer.selectedEntity = null;
}

export function getSelectedEntity() {
    return viewer.selectedEntity;
}

export function setSelectedEntity(e) {
    viewer.selectedEntity = e;
}

export function addEntity(e) {
    viewer.entities.add(e);
}
export function removeEntity(e) {
    viewer.entities.remove(e);
}


//--- websock handler funcs

function handleWsMessages(msgType, msg) {
    switch (msgType) {
        case "camera":
            handleCameraMessage(msg.camera);
        case "clock":
            handleSetClock(msg);
    }
}

function handleCameraMessage(newCamera) {
    cameraSpec = newCamera;
    setHomeView();
}

function handleSetClock(setClock) {
    ui.setClock( utcClock, setClock.time, setClock.timeScale, true);
    console.log("UTC clock set to ", ui.getClockDate(utcClock).toUTCString());

    ui.setClock( localClock, setClock.time, setClock.timeScale);
    console.log("local clock set to ", ui.getClockDate(utcClock));

    ui.resetTimer("time.elapsed", setClock.timeScale);
    ui.startTime();
}

function updateCamera() {
    let pos = viewer.camera.positionCartographic;
    let lat = Cesium.Math.toDegrees(pos.latitude);
    let lon = Cesium.Math.toDegrees(pos.longitude);
    let alt = Math.round(pos.height);

    ui.setField(cameraLat, lat.toFixed(4));
    ui.setField(cameraLon, lon.toFixed(4));
    ui.setField(cameraAlt, alt.toString());

    if (isSelectedView) {
        isSelectedView = false;
    } else {
        ui.clearSelectedListItem(positionsView); // we moved away from it
    }

    main.publishCmd( { updateCamera: {lon, lat, alt} });

    /*
    if (useEllipsoidTerrain()) {
        switchToEllipsoidTerrain(); // this checks if we already use it
    } else {
        switchToTopoTerrain();
    }
    */

    //saveCamera();
    updateScale();
}

/* #region map scale ************************************************************/

const WGS84BoundingSphere = Cesium.BoundingSphere.fromEllipsoid(Cesium.Ellipsoid.WGS84);

function initMapScale() {
    mapScale = ui.MoveableCanvas(["ui_mapscale"], {right: 50, bottom: 10});
    mapScale.width = config.scale.width;
    mapScale.height = config.scale.height;

    ui.showMoveableCanvas(mapScale);
}

function updateScale () {
    withMetersPerPixel( (dPixel)=> {
        let scale = isMetric ? getMetricScale(dPixel) : getUsScale(dPixel);
        drawScale( scale, dPixel);
    });
}

function withMetersPerPixel (f) {
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
        geo = new Cesium.Cartographic.fromRadians( geo.lon, geo.lat);
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

const metricScaleUnits = [ 1000000, 500000, 100000, 50000, 10000, 5000, 1000, 500, 100, 50, 10 ]; // in meters

function getMetricScale (dPixel) { 
    let unit, nUnits, unitPx, label, tickBase;
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
        tickBase = unit / 1000;
        label = `${tickBase * nUnits} km`;
    } else {
        tickBase = unit;
        label = `${tickBase * nUnits} m`;
    }
    
    return { unit, nUnits, unitPx, tickBase, label }
}

const usMileScaleUnits = [ 1000, 500, 100, 50, 10, 5, 1, 0.25 ];
const usYardScaleUnits = [ 500, 100, 50, 10 ];

function getUsScale (dPixel) {
    let unit, nUnits, unitPx, label, tickBase;
    let maxWidth = config.scale.width;

    //--- start with US miles
    for ( let i=0; i < usMileScaleUnits.length; i++) {
        unit = usMileScaleUnits[i];
        unitPx = (unit * util.metersPerUsMile) / dPixel;

        let nu = mileTicks(unit);
        for (let j=0; j<nu.length; j++) {
            nUnits = nu[j];
            if ((unitPx * nUnits) <= maxWidth) {
                return { unit, nUnits, unitPx, tickBase: unit, label: `${unit * nUnits} mi` };
            }
        }
    }
 
    //--- switch to yards
    for ( let i=0; i < usYardScaleUnits.length; i++) {
        unit = usYardScaleUnits[i];
        unitPx = (unit * util.metersPerYard) / dPixel;

        let nu = i % 2 ? mOdd : mEven;
        for (let j=0; j<nu.length; j++) {
            nUnits = nu[j];
            if ((unitPx * nUnits) <= maxWidth) {
                return { unit, nUnits, unitPx, tickBase: unit, label: `${unit * nUnits} yd` };
            }
        }
    }
}

function mileTicks (unit) {
    if (unit > 0.25) {
        return [5,4,3,2];
    } else if (unit == 0.25) {
        return [4,2];
    }
}

function drawScale ( scale, dPixel ) {
    let unit = scale.unit;
    let nUnits = scale.nUnits;
    let label = scale.label;
    let unitPx = scale.unitPx;
    let tickBase = scale.tickBase;
    //console.log("@@ unit:", unit, ", nUnits:", nUnits, ", label:", label, ", dPixel:", dPixel);

    let clr = config.scale.cssColor;
    let xMargin = 12;

    let canvasHeight = mapScale.clientHeight;
    let canvasWidth = Math.ceil( 2* xMargin + (unitPx * nUnits));
    mapScale.width = canvasWidth;

    let ctx = mapScale.getContext("2d");
    ctx.clearRect(0,0,canvasWidth,canvasHeight);

    ctx.fillStyle = clr;
    ctx.strokeStyle = clr;
    ctx.textAlign = "center";

    let y = canvasHeight / 2;

    //--- the bar
    ctx.beginPath();
    ctx.lineWidth = 2;
    ctx.moveTo( xMargin, y);
    ctx.lineTo( xMargin + (nUnits * unitPx), y);
    ctx.closePath();
    ctx.stroke();

    //--- the ticks
    if (unitPx > 25) {
        ctx.font = config.scale.smallFont;
        ctx.beginPath();
        for (let i = 0; i<=nUnits; i++) {
            let x = xMargin + (i*unitPx);
            ctx.moveTo(x, y);
            ctx.lineTo(x, y + 5);

            ctx.fillText( tickBase * i, x, y+14);
        }
        ctx.closePath();
        ctx.stroke();
    }

    //--- the length string
    ctx.font = config.scale.font;
    let x = Math.round( xMargin + (nUnits * unitPx) / 2);
    ctx.fillText( label, x, y - 3);
}

/* #endregion map scale */

//--- 2nd level event handlers

export function registerMouseMoveHandler(handler) {
    mouseMoveHandlers.push(handler);
}

export function releaseMouseMoveHandler(handler) {
    let idx = mouseMoveHandlers.findIndex(h => h === handler);
    if (idx >= 0) mouseMoveHandlers.splice(idx,1);
}

export function registerMouseClickHandler(handler) {
    mouseClickHandlers.push(handler);
}

export function releaseMouseClickHandler(handler) {
    let idx = mouseClickHandlers.findIndex(h => h === handler);
    if (idx >= 0) mouseClickHandlers.splice(idx,1);
}

export function registerMouseDownHandler(handler) {
    mouseDownHandlers.push(handler);
}

export function releaseMouseDownHandler(handler) {
    let idx = mouseDownHandlers.findIndex(h => h === handler);
    if (idx >= 0) mouseDownHandlers.splice(idx,1);
}

export function registerMouseUpHandler(handler) {
    mouseUpHandlers.push(handler);
}

export function releaseMouseUpHandler(handler) {
    let idx = mouseUpHandlers.findIndex(h => h === handler);
    if (idx >= 0) mouseUpHandlers.splice(idx,1);
}

export function registerMouseDblClickHandler(handler) {
    mouseDblClickHandlers.push(handler);
}

export function releaseMouseDblClickHandler(handler) {
    let idx = mouseDblClickHandlers.findIndex(h => h === handler);
    if (idx >= 0) mouseDblClickHandlers.splice(idx,1);
}

export function registerKeyDownHandler(handler) {
    keyDownHandlers.push(handler);
}

export function releaseKeyDownHandler(handler) {
    let idx = keyDownHandlers.findIndex(h => h === handler);
    if (idx >= 0) keyDownHandlers.splice(idx,1);
}

function handleMouseMove(e) {
    mouseMoveHandlers.forEach( handler=> handler(e));
}

function handleMouseClick(e) {
    mouseClickHandlers.forEach( handler=> handler(e));
}

function handleMouseDown(e) {
    mouseDownHandlers.forEach( handler=> handler(e));
}

function handleMouseUp(e) {
    mouseUpHandlers.forEach( handler=> handler(e));
}

function handleMouseDblClick(e) {
    mouseDblClickHandlers.forEach( handler=> handler(e));
}

function handleKeyDown(e) {
    keyDownHandlers.forEach( handler=> handler(e));
}

// global hotkeys - make sure these don't collide with module specific handlers
function globalKeyDownHandler (event) {
    if (Object.is( event.target, document.body)) { // otherwise this wasn't for us
        if (event.shiftKey) {
            if (event.keyCode >= 49 && event.keyCode <= 57) {
                let i = Math.min(event.keyCode - 49, config.zoomLevels.length-1);
                zoomToHeight( config.zoomLevels[i]);
            }
        }
    }
}

function globalMouseClickHandler (event) {
    if (event.shiftKey) {
        let camera = viewer.camera;
        let cp = camera.positionCartographic;
        let pos = getCartographicMousePosition(event);
        pos.height = cp.height;
        zoomTo( Cesium.Cartographic.toCartesian(pos));
    }
}

// mouse query cached positions
const cp2 = new Cesium.Cartesian2(); // screen
const cp3 = new Cesium.Cartesian3(); // ecef

export function getCartographicMousePosition(e, result=null) {
    cp2.x = e.clientX;
    cp2.y = e.clientY;

    let ellipsoid = viewer.scene.globe.ellipsoid;
    
    //let cartesian = viewer.camera.pickEllipsoid( cp2, ellipsoid, cp3); // mouse might be outside globe    
    let cartesian = viewer.scene.pickPosition( cp2, result);
    return cartesian ? ellipsoid.cartesianToCartographic( cartesian, result) : undefined;
}

export function getCartesian3MousePosition(e, result=null) {
    cp2.x = e.clientX;
    cp2.y = e.clientY;

    //let ellipsoid = viewer.scene.globe.ellipsoid;
    //return viewer.camera.pickEllipsoid( cp2, ellipsoid, result);
    return viewer.scene.pickPosition( cp2, result);
}

export function getWindowMousePosition(e) {
    cp2.x = e.clientX;
    cp2.y = e.clientY;

    return cp2;
}

var deferredMouseUpdate = undefined;

function updateMouseLocation(e) {
    if (deferredMouseUpdate) clearTimeout(deferredMouseUpdate);
    deferredMouseUpdate = setTimeout( () => {
        let pos = getCartographicMousePosition(e);
        if (pos) {
            let latDeg = Cesium.Math.toDegrees(pos.latitude);
            let lonDeg = Cesium.Math.toDegrees(pos.longitude);

            let longitudeString = lonDeg.toFixed(4);
            let latitudeString = latDeg.toFixed(4);
    
            ui.setField(pointerLat, latitudeString);
            ui.setField(pointerLon, longitudeString);
    
            if (topoTerrainProvider) {
                let a = [pos];
                Cesium.sampleTerrainMostDetailed(topoTerrainProvider, a).then( (a) => {
                    ui.setField(pointerElev, Math.round(a[0].height));
                });
            }

            let utm = util.latLon2Utm(latDeg, lonDeg);
            ui.setField(pointerUtmN, utm.northing);
            ui.setField(pointerUtmE, utm.easting);
            ui.setField(pointerUtmZ, `${utm.utmZone} ${utm.band}`);
        }
    }, 300);
}

//--- user control 

function setViewFromFields() {
    let lat = ui.getFieldValue(cameraLat);
    let lon = ui.getFieldValue(cameraLon);
    let alt = ui.getFieldValue(cameraAlt);

    if (lat && lon && alt) {
        let latDeg = parseFloat(lat);
        let lonDeg = parseFloat(lon);
        let altM = parseFloat(alt);

        // TODO - we should check for valid ranges here
        if (isNaN(latDeg)) { alert("invalid latitude: " + lat); return; }
        if (isNaN(lonDeg)) { alert("invalid longitude: " + lon); return; }
        if (isNaN(altM)) { alert("invalid altitude: " + alt); return; }

        viewer.camera.flyTo({
            destination: Cesium.Cartesian3.fromDegrees(lonDeg, latDeg, altM),
            orientation: centerOrientation
        });
    } else {
        alert("please enter latitude, longitude and altitude");
    }
}

export function saveCamera() {
    let camera = viewer.camera;
    let pos = camera.positionCartographic;

    lastCamera = {
        lat: util.toDegrees(pos.latitude),
        lon: util.toDegrees(pos.longitude),
        alt: pos.height,
        heading: util.toDegrees(camera.heading),
        pitch: util.toDegrees(camera.pitch),
        roll: util.toDegrees(camera.roll)
    };

    // TODO - this should be triggered by a copy-to-clipboard button
    //let spec = `{ lat: ${util.fmax_4.format(lastCamera.lat)}, lon: ${util.fmax_4.format(lastCamera.lon)}, alt: ${Math.round(lastCamera.alt)} }`;
    //navigator.clipboard.writeText(spec);  // this is still experimental in browsers and needs to be enabled explicitly (for each doc?) for security reasons
    //console.log(spec);
}

export function zoomToHeight (height) {
    let cp = viewer.camera.position;
    let pNew = Cesium.Ellipsoid.WGS84.scaleToGeodeticSurface( cp, new Cesium.Cartesian3());
    let ds = Cesium.Cartesian3.magnitude(pNew);
    let a = (ds + height) / ds;
    Cesium.Cartesian3.multiplyByScalar( pNew, a, pNew);

    zoomTo(pNew);
}

export function zoomTo(cameraPos) {
    saveCamera();

    viewer.camera.flyTo({
        destination: cameraPos,
        orientation: centerOrientation
    });
}

function setInitialView () {
    let initPos = initPosition ? initPosition : homePosition;
    setCamera( initPos);
}

export function setHomeView() {
    setCamera(homePosition);
}

export function setCamera(camera) {
    saveCamera();

    viewer.selectedEntity = undefined;
    viewer.trackedEntity = undefined;
    viewer.camera.flyTo({
        destination: Cesium.Cartesian3.fromDegrees(camera.lon, camera.lat, camera.alt),
        orientation: centerOrientation
    });
}

function setCameraFromSelection(event){
    let p = ui.getSelectedListItem(positionsView);
    if (p) {
        setCamera(p);
        isSelectedView = true;
    }
}

function setCameraName(event) {
    let node = ui.getSelectedTreeNode( positionsView);
    if (node) {
        let path = node.collectNamesUp('/');
        ui.setField( cameraName, path);
    } 
}

var minCameraHeight = 50000;

export function setDownView() {

    // use the position we are looking at, not the current camera position
    const canvas = viewer.scene.canvas;
    const center = new Cesium.Cartesian2(canvas.clientWidth / 2.0, canvas.clientHeight / 2.0);
    const ellipsoid = viewer.scene.globe.ellipsoid;
    let wc = viewer.camera.pickEllipsoid(center,ellipsoid);
    let pos = Cesium.Cartographic.fromCartesian(wc);

    //let pos = viewer.camera.positionCartographic;
    if (pos.height < minCameraHeight) pos = new Cesium.Cartographic(pos.longitude,pos.latitude,minCameraHeight);

    viewer.trackedEntity = undefined;

    viewer.camera.flyTo({
        destination: Cesium.Cartographic.toCartesian(pos),
        orientation: centerOrientation
    });
}

export function restoreCamera() {
    if (lastCamera) {
        let last = lastCamera;
        saveCamera();
        setCamera(last);
    }
}


export function toggleFullScreen(event) {
    ui.toggleFullScreen();
}

function setFrameRate(event) {
    let v = ui.getSliderValue(event.target);
    setTargetFrameRate(v);
}

//--- module layers


function setLayerOrderCb(le) {
    let cb = ui.createCheckBox(le.show, toggleShowLayer);
    le.layerOrderCb = cb;
    return cb;
}

function initLayerHierarchyView() {
    let v = ui.getList("layer.hierarchy");
    if (v) {

    }
    return v;
}

function toggleShowLayer(event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let le = ui.getListItemOfElement(cb);
        if (le) le.setVisible(ui.isCheckBoxSelected(cb));
    }
}

// called after all modules have loaded
function initModuleLayerViewData() {
    ui.setListItems(layerOrderView, layerOrder);
}

// called by layer modules during their init - the panel is in the respective module window
export function initLayerPanel(wid, conf, showAction) {
    if (conf && conf.layer) {
        let phe = document.getElementById(wid + ".layer-header");
        if (phe) {
            let le = new LayerEntry(wid,conf.layer,showAction);

            phe.innerText = "layer: " + conf.layer.name.replaceAll('/', '╱'); // │
            let cb = ui.createCheckBox(conf.layer.show, (event) => {
                event.stopPropagation();
                le.setVisible(ui.isCheckBoxSelected(cb));
            });
            ui.positionRight(cb, 0);
            phe.appendChild(cb);
            le.modulePanelCb = cb;

            ui.setVarText(wid + '.layer-descr', conf.layer.description);

            layerOrder.push(le);
        }
    }
}

export function isLayerShowing(layerPath) {
    let le = layerOrder.find( le=> le.id == layerPath);
    return (le && le.show);
}

function raiseModuleLayer(event){
    let le = ui.getSelectedListItem(layerOrderView);
    console.log("TBD raise layer: " + le);
}

function lowerModuleLayer(event){
    let le = ui.getSelectedListItem(layerOrderView);
    console.log("TBD lower layer: " + le);
}

/* #region interactive geo input ********************************************/

/// the low level entry function - no handles, just cartesian points. Handles for subsequent editing
/// have to be added through the provided callbacks. This function does not create any
/// Cesium resources. We support a onMouseMove callback so that we don't have to redundantly calculate
/// the Cartesian3 mouse position. There is no onDelPoint since we can't delete points in enter
/// mode - we can't set the pointer position in Javascript
/// callbacks: { onEnter, onCancel, onAddPoint, onDelPoint, onMouseMove }

export function enterPolyline (points, maxPoints, callbacks) {
    let cp = new Cesium.Cartesian3(); // cached point to save allocs
    let dblClickAction = getInputAction( Cesium.ScreenSpaceEventType.LEFT_DOUBLE_CLICK);

    points.push( new Cesium.Cartesian3()); // add the mover point
  
    function onMouseMove(event) { // update the last point position (will redraw polyline using points)    
        let idx = points.length-1;
        let p = points[idx];
    
        getCartesian3MousePosition(event, cp);
        p.x = cp.x;   p.y = cp.y;   p.z = cp.z;
    
        if (callbacks.onMouseMove) { callbacks.onMouseMove( points, idx); }
    }
  
    function onClick(event) {
        if (event.detail == 2) { // double click -> done entering
            clearSelectedEntity(); // Cesium likes to zoom in on double clicks
            event.preventDefault(); 
            resetEnterPolyline();
    
            if (points.length >= 1) {
                if (points.length > 1) points.pop();  // remove the mover
                if (callbacks.onEnter) callbacks.onEnter();
            }
  
        } else if (event.detail == 1) { // single click (but also before double click)
            getCartesian3MousePosition(event, cp);
    
            let p = points[points.length-1];
            p.x = cp.x;   p.y = cp.y;   p.z = cp.z;
    
            if (callbacks.onAddPoint) { callbacks.onAddPoint(p); }
    
            if (maxPoints && points.length >= maxPoints) {
                resetEnterPolyline();
                if (callbacks.onEnter) callbacks.onEnter();
            } else {
                let pMover = { ...p };
                points.push( pMover); 
            }
        }
    }
  
    function resetEnterPolyline() {
        setDefaultCursor();
        releaseMouseClickHandler( onClick);
        releaseMouseMoveHandler( onMouseMove);
        releaseKeyDownHandler( onKeyDown);

        // we have to defer this or an ending double click still selects the handle entity 
        setTimeout( ()=> setInputAction( dblClickAction, Cesium.ScreenSpaceEventType.LEFT_DOUBLE_CLICK), 200);
    }
  
    function onKeyDown(event) {
        if (event.target === document.body) { // otherwise this wasn't for us
            if (event.code == "Delete" || event.code == "Backspace") {
                let idx = points.length-2;
                let p = points[idx];
                points.splice( idx, 1);
                if (callbacks.onDelPoint) callbacks.onDelPoint( idx, p);

            } else if (event.code == "Escape") { // exit edit alltogether
                resetEnterPolyline();
                if (callbacks.onCancel) callbacks.onCancel();
            }
        }
    }
  
    setCursor( "copy");
    registerMouseClickHandler( onClick);
    registerMouseMoveHandler( onMouseMove);
    registerKeyDownHandler( onKeyDown);

    removeInputAction( Cesium.ScreenSpaceEventType.LEFT_DOUBLE_CLICK); // avoid selection on double click

    return resetEnterPolyline; // so that we can cancel from the outside
}

export function enterGeoPoint (processResult) {
    let points = [];
    enterPolyline( points, 1, { onEnter: ()=> {
        let geo = Cesium.Cartographic.fromCartesian(points[0]);
        processResult( main.GeoPoint.fromLonLatRadians( geo.longitude, geo.latitude));
    } });
}

export function enterGeoLine (processResult, onMouseMove=undefined) {
    let points = [];
    let polyEntity = undefined;

    function onAddPoint (idx, p) {
        if (polyEntity === undefined) {
            setRequestRenderMode(false); // we track the mouse
            polyEntity = new Cesium.Entity( {
                polyline: polylineOpts( points),
                selectable: false
            });
            viewer.entities.add( polyEntity);
        }
    }

    function onEnter () {
        cleanUpEnter(polyEntity);

        if (points.length == 2) {
            let c0 = Cesium.Cartographic.fromCartesian(points[0]);
            let c1 = Cesium.Cartographic.fromCartesian(points[1]);

            let start = main.GeoPoint.fromLonLatRadians( c0.longitude, c0.latitude);
            let end = main.GeoPoint.fromLonLatRadians( c1.longitude, c1.latitude);

            processResult( new main.GeoLine(start, end));
        }
    }

    function onCancel () { cleanUpEnter(polyEntity); }

    enterPolyline( points, 2, {onEnter, onCancel, onAddPoint, onMouseMove});
}

function cleanUpEnter(entity) {
    if (entity) viewer.entities.remove(entity);
    setRequestRenderMode(true);
    requestRender();
}

export function enterGeoLineString (processResult) {
    let points = [];
    let polyEntity = undefined;

    function onAddPoint (idx, p) {
        if (polyEntity === undefined) {
            setRequestRenderMode(false); // we track the mouse
            polyEntity = new Cesium.Entity( {
                polyline: polylineOpts( points),
                selectable: false
            });
            viewer.entities.add( polyEntity);
        }
    }

    function onEnter () {
        cleanUpEnter(polyEntity);

        let pts = points.map( (p)=> {
            let c = Cesium.Cartographic.fromCartesian(p);
            return main.GeoPoint.fromLonLatRadians( c.longitude, c.latitude);
        });
        processResult( new main.GeoLineString(pts));
    }

    function onCancel () { cleanUpEnter(polyEntity); }

    enterPolyline( points, 0, {onEnter, onCancel, onAddPoint});
}

export function enterGeoPolygon (processResult) {
    let points = [];
    let polyEntity = undefined;

    function onAddPoint (idx, p) {
        if (polyEntity === undefined) {
            setRequestRenderMode(false); // we track the mouse
            polyEntity = new Cesium.Entity( {
                polyline: polylineOpts( points),
                polygon: polygonOpts( points),
                selectable: false
            });
            viewer.entities.add( polyEntity);
        }
    }

    function onEnter () {
        cleanUpEnter(polyEntity);

        let pts = points.map( (p)=> {
            let c = Cesium.Cartographic.fromCartesian(p);
            return main.GeoPoint.fromLonLatRadians( c.longitude, c.latitude);
        });
        processResult( new main.GeoPolygon(pts));
    }

    function onCancel () { cleanUpEnter(polyEntity); }

    enterPolyline( points, 0, {onEnter, onCancel, onAddPoint});
}

// minimal environment to enter a GeoRect with outline&fill rendering (no editing/handles)
export function enterGeoRect (processResult) {
    let rect = new Cesium.Rectangle();
    let points = [];
    let rectEntity = undefined;

    function onAddPoint (pGeo) {
        cleanUpEnter(rectEntity);

        if (rectEntity === undefined) {
            setRequestRenderMode(false); // we track the mouse
            rectEntity = new Cesium.Entity( {
                polyline: polylineOpts( points),
                polygon: polygonOpts( points),
                selectable: false
            });
            viewer.entities.add( rectEntity);
        }
    }

    function onEnter () {
        if (rectEntity) viewer.entities.remove(rectEntity);
        processResult( main.GeoRect.fromWSENdeg( rect.west, rect.south, rect.east, rect.north));
    }

    function onCancel () { cleanUpEnter(rectEntity); }

    enterRect( rect, points, { onEnter, onCancel, onAddPoint });
}

function polylineOpts (points) {
    return {
        positions: new Cesium.CallbackProperty( () => points, false),
        clampToGround: true,
        width: 2,
        material: Cesium.Color.RED
    };
}

function polygonOpts (points) {
    return {
        hierarchy: new Cesium.CallbackProperty( () => new Cesium.PolygonHierarchy( points)),
        material: Cesium.Color.RED.withAlpha(0.2)
    };
}

/// low level Rectangle entry - no Cesium assets, just entering two points and updating a cartographic rectangle
/// and the 5 element cartesian point array.
/// callbacks { onEnter, onCancel, onAddPoint, onMouseMove }
export function enterRect (rect, points, callbacks) {
    let cp2 = new Cesium.Cartographic();
    let p0 = undefined;  // 1st corner of rect
    if (points === undefined) points = Cesium.Cartesian3.fromDegreesArray([0,0, 0,0, 0,0, 0,0, 0,0]);
    if (rect == undefined) rect = new Cesium.Rectangle();

    function onMouseMove(event) {
        let p = getCartographicMousePosition(event, cp2);
        if (p0) {
            setRectFromCornerPoints( rect, p0, p);
            cartesian3ArrayFromRadiansRect(rect, points);
        }
        
        if (callbacks.onMouseMove) callbacks.onMouseMove( p);
    }

    function onClick(event) {
        let p = getCartographicMousePosition(event);
        if (p) { 
            if (event.detail == 1) { // ignore double click
                if (p0 === undefined) { // first corner
                    p0 = p;

                    setRectFromCornerPoints( rect, p0, p0);
                    cartesian3ArrayFromRadiansRect( rect, points);
                    if (callbacks.onAddPoint) callbacks.onAddPoint( p);

                } else { // 2nd corner - this terminates the entry
                    setRectFromCornerPoints( rect, p0, p);
                    cartesian3ArrayFromRadiansRect( rect, points);
                    if (callbacks.onAddPoint) callbacks.onAddPoint( p);

                    resetEnterRect();
                    if (callbacks.onEnter) callbacks.onEnter( util.rectToDegrees(rect));
                }
            }
        }
    }

    function onKeyDown(event) {
        if (event.code == "Escape") { // exit edit alltogether
            resetEnterRect();
            if (callbacks.onCancel) callbacks.onCancel();
        }
    }

    function resetEnterRect() {
        setDefaultCursor();
        releaseMouseMoveHandler( onMouseMove);
        releaseMouseClickHandler( onClick);
        releaseKeyDownHandler( onKeyDown);
    }

    setCursor("copy");
    registerMouseMoveHandler( onMouseMove);
    registerMouseClickHandler( onClick);
    registerKeyDownHandler( onKeyDown);

    return resetEnterRect;
}

export function setRectFromCornerPoints (rect, p0, p1) {
    rect.west = Math.min( p0.longitude, p1.longitude);
    rect.south = Math.min( p0.latitude, p1.latitude);
    rect.east = Math.max( p0.longitude, p1.longitude);
    rect.north = Math.max( p0.latitude, p1.latitude);
}

export function enterGeoCircle (processResult) {
    let pCenter = undefined;
    let radius = 0.0;

    let circleEntity = undefined;
    let points = [];

    function cleanUp() {
        if (circleEntity) viewer.entities.remove(circleEntity);
    }

    function onAddPoint (pGeo) {
        //cleanUpEnter(circleEntity);

        if (circleEntity === undefined) {
            pCenter = pGeo.clone();
            setRequestRenderMode(false); // we track the mouse

            circleEntity = new Cesium.Entity( {
                position: new Cesium.CallbackProperty( () => points[0], false),
                ellipse: {
                    semiMajorAxis: new Cesium.CallbackProperty( () => radius, false),
                    semiMinorAxis: new Cesium.CallbackProperty( () => radius, false),
                    fill: true,
                    material: Cesium.Color.RED.withAlpha(0.2),
                    //outline: true,
                    //outlineColor: Cesium.Color.RED,
                    //outlineWidth: 5,
                    //height: 0.0
                },
                polyline: polylineOpts( points),
                selectable: false
            });
            viewer.entities.add( circleEntity);


        } else {
            updateRadius(pGeo);  // update radius
        }
    }

    function updateRadius (pGeo){
        if (pCenter) {
            radius = util.gcDistanceBetweenECEF (points[0], points[1]);
            if (circleEntity) {
                circleEntity.ellipse.semiMajorAxis = radius;
                circleEntity.ellipse.semiMinorAxis = radius;
            }
        }
    }

    function onEnter () {
        cleanUp();
        let geo = util.ecefToGeo(pCenter.x, pCenter.y, pCenter.z);
        processResult( new main.GeoCircle( util.toDegrees(geo.lon), util.toDegrees(geo.lat), radius));
    }

    function onCancel () { cleanUp(); }

    enterPolyline( points, 2, { onEnter, onCancel, onAddPoint, onMouseMove: updateRadius });
}

/* #endregion geo entry */

export function cartesianToCartographicDegrees (p) {
    return cartographicToDegrees( Cesium.Cartographic.fromCartesian(p));
}

export function cartographicToDegrees (p) {
    return { latitude: Cesium.Math.toDegrees(p.latitude), longitude: Cesium.Math.toDegrees(p.longitude), height: p.height };
}

export function cartesian3ArrayFromRadiansRect (rect, arr=null) {
    let a = arr ? arr : new Array(5);

    a[0] = Cesium.Cartesian3.fromRadians( rect.west, rect.south);
    a[1] = Cesium.Cartesian3.fromRadians( rect.east, rect.south);
    a[2] = Cesium.Cartesian3.fromRadians( rect.east, rect.north);
    a[3] = Cesium.Cartesian3.fromRadians( rect.west, rect.north);
    a[4] = a[0];

    return a;
}

export function cartesian3ArrayFromDegreesRect (rect, arr=null) {
    let a = arr ? arr : new Array(5);

    a[0] = Cesium.Cartesian3.fromDegrees( rect.west, rect.south);
    a[1] = Cesium.Cartesian3.fromDegrees( rect.east, rect.south);
    a[2] = Cesium.Cartesian3.fromDegrees( rect.east, rect.north);
    a[3] = Cesium.Cartesian3.fromDegrees( rect.west, rect.north);
    a[4] = a[0];

    return a;
}

export function withinRect(latDeg, lonDeg, degRect) {
    return (lonDeg >= degRect.west) && (lonDeg <= degRect.east) && (latDeg >= degRect.south) && (latDeg <= degRect.north);
}

export function getHprFromQuaternion (qx, qy, qz, w) {
    let q = new Cesium.Quaternion( qx, qy, qz, w);
    return Cesium.HeadingPitchRoll.fromQuaternion(q);
}

export function getEnuRotFromQuaternion (qx, qy, qz, w) {
    let q = new Cesium.Quaternion( qx, qy, qz, w);
    let qRot = Cesium.Quaternion.inverse(q, new Cesium.Quaternion());
    return Cesium.Matrix3.fromQuaternion( qRot);
}

// center is Cartesian3 
export function circleOutline (center,radius) {
    //let axis = Cesium.Caresian3.normalize( center, new Cartesian3()); // the rotation axis unit vec

    // the rotation quaternion
    let q = Cesium.Quaternion.fromAxisAngle(center, util.toRadians(5)); // we approximate in 5deg steps (72 vertices)
    let b = new Cesium.Cartesian3( q.x, q.y, q.z); // the vector part of the rotation quaternion
    let b2 = b.x*b.x + b.y*b.y + b.z*b.z;
    let qwb2 = q.w * q.w - b2;
    let qw2 = q.w*2;

    // compute start point on circle (on parallel -> same z as center)
    // we assume the radius is the distance on the sphere
    // we have to divide by the radius of the parallel at the z position
    let a = radius / (Math.sqrt(util.meanEarthRadius*util.meanEarthRadius - center.z*center.z));
    let sin_a = Math.sin(a);
    let cos_a = Math.cos(a);

    let v = new Cesium.Cartesian3( 
        center.x * cos_a - center.y * sin_a,
        center.x * sin_a + center.y * cos_a,
        center.z
    );

    let r1 = new Cesium.Cartesian3();
    let r2 = new Cesium.Cartesian3();
    let r3 = new Cesium.Cartesian3();
    let r4 = new Cesium.Cartesian3();

    let vertices = [v];

    // now rotate p around center in 5 deg steps
    for (let i=0; i<72; i++) {
        let dot2 = Cesium.Cartesian3.dot( v, b) * 2;
        let cross = Cesium.Cartesian3.cross( b, v, r1);

        let v1 = Cesium.Cartesian3.multiplyByScalar( v, qwb2, r2);
        let v2 = Cesium.Cartesian3.multiplyByScalar( b, dot2, r3);
        let v3 = Cesium.Cartesian3.multiplyByScalar( cross, qw2, r4);

        v = new Cesium.Cartesian3(
            v1.x + v2.x + v3.x,
            v1.y + v2.y + v3.y,
            v1.z + v2.z + v3.z
        );

        vertices.push(v);
    }
    vertices.push( vertices[0]); // close polygon

    return vertices;
}



function setCesiumContainerVisibility (isVisible) {
    document.getElementById("cesiumContainer").style.visibility = isVisible;
}

// return object suitable to set a Point3D from the current camera position
function withCurrentCameraPosition (callback) {
    let pos = viewer.camera.positionCartographic;
    callback({
        lon: Math.round( Cesium.Math.toDegrees(pos.longitude) * 10000) / 10000, // round to 4 decimals
        lat: Math.round( Cesium.Math.toDegrees(pos.latitude) * 10000) / 10000,
        alt: Math.round(pos.height)
    });
}

// a hack to prevent rendering delays when reloading polyline workers
function keepPolylineWorkersAlive () {
    viewer.entities.add(
        new Cesium.Entity({
            name: 'dummy line to keep polyline web worker alive',
            polyline: {
                positions: [new Cesium.Cartesian3(0, 0, 0), new Cesium.Cartesian3(1, 1, 1)],
                arcType: Cesium.ArcType.NONE, // required, or runtime error
                material: new Cesium.PolylineDashMaterialProperty({color: Cesium.Color.CYAN}), // required, or else no worker!
            },
        }),
    );
}

/* #region share interface ***********************************************************************************/

function handleShareMessage (msg) {
    if (msg.SHARE_INITIALIZED) { // we get that no matter what the share implementation is
        updateSharedViewPositions();

    } else if (main.isShareInitialized()) { // otherwise we still get a SHARE_INITIALIZED
        if (msg.setShared) {
            let sharedItem = msg.setShared;
            if (sharedItem.key.match(VIEW_PATTERN)) {
                let sharedVal = sharedItem.value;
                if (sharedVal.type == "GeoPoint3") {
                    let p = sharedVal.data;
                    let newPos = new CameraPosition( sharedItem.key, p.lon, p.lat, p.alt, sharedItem.isLocal);
                    
                    positions.set( sharedItem.key, newPos);
                    ui.sortInTreeItem( positionsView, newPos, sharedItem.key);
                    //updatePositionsView();
                }
            }
        } else if (msg.removeShared) {
            let key = msg.removeShared;
            if (positions.has(key)) {
                positions.delete(key);
                ui.removeTreeItemPath( positionsView, key);
                //updatePositionsView();
            }
        }
    }
}

function handleSyncMessage (msg) {
    if (msg.updateCamera) {
        setCamera( msg.updateCamera);
    }
    //... and more to follow
}

/* #endregion share interface */

// executed after all modules have been loaded and initialized
export function postInitialize() {
    initModuleLayerViewData();    

    if (config.showTerrain) {
        switchToTopoTerrain();
    }

    const credit = new Cesium.Credit('<a href="https://openstreetmap.org/" target="_blank">OpenStreetMap</a>');
    viewer.creditDisplay.addStaticCredit(credit);

    setCesiumContainerVisibility(true);
    keepPolylineWorkersAlive();

    setRequestRenderTimer(); // set background render timer according to configured requestRenderMode

    console.log("odin_cesium.postInitialize complete.");
}

