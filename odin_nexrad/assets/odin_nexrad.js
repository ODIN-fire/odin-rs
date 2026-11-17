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

// display of NEXRAD Level II products (odin_nexrad::service::NexradService). Each product time step of a station is
// a WMS layer of our server. Stations are shown on the map and listed in the window, the "show" checkbox of a station
// determines if it contributes to the display. The product tabs list the time steps of all shown stations (newest first)
// and each station displays at most one time step at a time. What is displayed is determined by a time cursor:
//  - "show latest of selected": the cursor is the newest time step of the shown stations (following new imports)
//  - stepping (list controls): the cursor is the selected time step
// For a cursor T each station displays its newest time step <= T that is not older than T - maxAge (display panel).
// Where stations overlap the layer of the station that is closest to the view center can be put on top (display panel),
// which together with maxAge approximates a mosaic of the shown stations. Users can still exclude stations explicitly
// Explicitly checking/unchecking a time step (manual mode) only changes the respective station.
// Ages (station and value lists, station info labels) are relative to the time cursor (or the newest displayed time
// step in manual mode)

import { config } from "./odin_nexrad_config.js";

import * as util from "../odin_server/ui_util.js";
import * as ui from "../odin_server/ui.js";
import * as ws from "../odin_server/ws.js";
import * as odinCesium from "../odin_cesium/odin_cesium.js";

const MOD_PATH = "odin_nexrad::service::NexradService";
const WID = "nexrad"; // window id

ws.addWsHandler( MOD_PATH, handleWsMessages);

/* #region data *****************************************************************************************************/

// station id -> { id, name, lat, lon, extent:[w,s,e,n], products: { <product name>: [ entry,.. ] (newest first) }, entity, infoEntity }
// with entry: { station: <id>, product: <name>, date, layer }
var stations = new Map();
var selectedStation = undefined;
var activeProduct = config.product;

// station id -> { product, entry, layer: Cesium.ImageryLayer } of shown time steps
var shown = new Map();
// ids of stations the user has de-selected ("show" checkbox). Their time steps are kept but not displayed or queried
var disabledStations = new Set();

// time cursor: undefined in manual mode or if there are no time steps, otherwise epoch millis
var cursorTime = undefined;

var opacity = config.opacity;
var maxAge = config.maxAge; // [min] stations don't display time steps that are older than this at the time cursor
var isNearestOnTop = config.nearestOnTop; // order layers of overlapping stations by distance to view center
var isLayerVisible = config.layer.show; // layer panel visibility

var stationView = undefined;
var entryViews = {}; // product name -> list
var valueView = undefined;
var cursorText = undefined;
var showLatestCb = undefined;
var pickBtn = undefined;
var isPicking = false;
var nexradWindow = undefined;

var stationDataSource = odinCesium.createDataSource( "nexrad", config.layer.show);

/* #endregion data */

/* #region initialization *******************************************************************************************/

createIcon();
nexradWindow = createWindow();
initStationView();
config.products.forEach(p => initEntryView(p.name));
initValueView();

odinCesium.initLayerPanel( WID, config, showNexrad);
odinCesium.setEntitySelectionHandler( stationSelection);
odinCesium.registerMouseClickHandler( handleMouseClick);
odinCesium.registerKeyDownHandler( handleKeyDown);
odinCesium.viewerReadyPromise.then( viewer => viewer.camera.moveEnd.addEventListener( orderLayers));

console.log("odin_nexrad initialized");

function createIcon() {
    return ui.Icon("./asset/odin_nexrad/nexrad.svg", (e)=> ui.toggleWindow(e, WID), "NEXRAD weather radar");
}

function createWindow() {
    let tabs = config.products.map( p =>
        ui.Tab( p.label, p.name === activeProduct, `${WID}.tab.${p.name}`, () => selectProductTab( p.name))(
            (entryViews[p.name] = ui.List( `${WID}.entries.${p.name}`, 8, null))
        )
    );

    return ui.Window("NEXRAD Radar", WID, "./asset/odin_nexrad/nexrad.svg")(
        ui.LayerPanel(WID),
        ui.Panel("stations", true)(
            (stationView = ui.List(WID + ".stations", 6, selectStation, null, null, zoomToStation)),
            ui.RowContainer()(
                (cursorText = ui.VarText( null, WID + ".cursor", "12rem")),
                (showLatestCb = ui.CheckBox("show latest of selected", toggleShowLatest, WID + ".showLatest", config.showLatest))
            ),
            ui.RowContainer("start")(
                ui.ColumnContainer("align_right")(
                    ui.TabbedContainer(WID + ".products")(...tabs),
                    // step the time cursor through the time steps of the active product tab.
                    // The list id is only required to initialize the controls, the actions use the active tab list
                    ui.ListControls( `${WID}.entries.${config.products[0].name}`,
                        () => stepEntry( ui.selectFirstListItem),
                        () => stepEntry( ui.selectPrevListItem),
                        () => stepEntry( ui.selectNextListItem),
                        () => stepEntry( ui.selectLastListItem),
                        clearEntry)
                ),
                ui.HorizontalSpacer(0.5),
                ui.ColumnContainer()(
                    (pickBtn = ui.Button("pick", togglePickMode)),
                    (valueView = ui.List( WID + ".values", 6))
                )
            )
        ),
        ui.Panel("display", false)(
            ui.CheckBox("nearest station on top", toggleNearestOnTop, WID + ".nearestOnTop", isNearestOnTop),
            ui.Slider("max age [min]", WID + ".maxAge", maxAgeChanged, { min: 5, max: 60, step: 5, fmt: util.f_0, value: maxAge }),
            ui.Slider("opacity", WID + ".opacity", opacityChanged, { min: 0, max: 1.0, step: 0.05, fmt: util.f_2, value: opacity })
        ),
    );
}

function initStationView() {
    ui.setListItemDisplayColumns( stationView, ["fit", "header"], [
        { name: "show", tip: "show station data", width: "2.5rem", attrs: [], map: s => ui.createCheckBox( isStationEnabled(s), toggleStationEnabled) },
        { name: "", tip: "displayed time step", width: "1.5rem", attrs: [], map: s => isDisplayed( s) ? "●" : "" },
        { name: "id", width: "3.5rem", attrs: [], map: s => s.id },
        { name: "name", width: "9rem", attrs: [], map: s => util.maxString( s.name, 26) },
        ui.listItemSpacerColumn(0.5),
        { name: "age", tip: "age of (displayed or available) time step at time cursor [min]", width: "2.5rem", attrs: ["fixed", "alignRight"], map: s => ageString( stationAgeEntry( s)) },
        ui.listItemSpacerColumn(0.5),
        { name: "latest", tip: "scan time of latest time step", width: "9rem", attrs: ["fixed", "alignRight"], map: s => latestDateString(s) }
    ]);
}

function initValueView() {
    ui.setListItemDisplayColumns( valueView, ["fit", "header"], [
        { name: "id", width: "3.5rem", attrs: [], map: v => v.id },
        ui.listItemSpacerColumn(0.5),
        { name: "age", tip: "age of time step at time cursor [min]", width: "2.5rem", attrs: ["fixed", "alignRight"], map: v => ageString( v.entry) },
        { name: "val", tip: "reflectivity value [dBZ]", width: "3.5rem", attrs: ["fixed", "alignRight"], map: v => v.value }
    ]);
}

function initEntryView (productName) {
    ui.setListItemDisplayColumns( entryViews[productName], ["fit", "header"], [
        { name: "show", tip: "show time step (resets 'show latest')", width: "2.5rem", attrs: [], map: e => ui.createCheckBox( isShown(e), toggleShowEntry) },
        { name: "id", width: "3.5rem", attrs: [], map: e => e.station },
        ui.listItemSpacerColumn(0.5),
        { name: "scan time", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString( e.date) }
    ]);
}

function latestDateString (s) {
    let latest = Object.values( s.products).map( es => es[0]).filter( e => e).map( e => e.date);
    return (latest.length > 0) ? util.toLocalMDHMSString( Math.max( ...latest)) : "-";
}

/* #endregion initialization */

/* #region stations *************************************************************************************************/

function createStationEntity (s) {
    let entity = new Cesium.Entity({
        id: `nexrad-${s.id}`,
        position: Cesium.Cartesian3.fromDegrees( s.lon, s.lat),
        point: {
            pixelSize: config.station.pointSize,
            color: hasData(s) ? config.station.color : config.station.noDataColor,
            outlineColor: config.station.outlineColor,
            outlineWidth: 1,
            heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
            disableDepthTestDistance: Number.POSITIVE_INFINITY
        },
        label: {
            text: s.id,
            font: config.station.labelFont,
            fillColor: config.station.labelColor,
            showBackground: true,
            backgroundColor: config.station.labelBackground,
            horizontalOrigin: Cesium.HorizontalOrigin.LEFT,
            verticalOrigin: Cesium.VerticalOrigin.CENTER,
            pixelOffset: config.station.labelOffset,
            distanceDisplayCondition: config.station.labelDC,
            heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
            disableDepthTestDistance: Number.POSITIVE_INFINITY
        }
    });
    entity._uiNexradStation = s; // backlink for selection
    stationDataSource.entities.add( entity);
    return entity;
}

// additional station info that is only shown when zoomed in (config.station.infoDC)
function createStationInfoEntity (s) {
    let entity = new Cesium.Entity({
        id: `nexrad-${s.id}-info`,
        position: Cesium.Cartesian3.fromDegrees( s.lon, s.lat),
        label: {
            text: stationInfoText( s),
            font: config.station.infoFont,
            fillColor: config.station.labelColor,
            showBackground: true,
            backgroundColor: config.station.labelBackground,
            horizontalOrigin: Cesium.HorizontalOrigin.LEFT,
            verticalOrigin: Cesium.VerticalOrigin.TOP,
            pixelOffset: config.station.infoOffset,
            distanceDisplayCondition: config.station.infoDC,
            heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
            disableDepthTestDistance: Number.POSITIVE_INFINITY
        }
    });
    stationDataSource.entities.add( entity);
    return entity;
}

function stationInfoText (s) {
    let lines = [];
    let v = shown.get( s.id);
    if (v) {
        let product = productLabel(v.product);
        lines.push(product);
        lines.push(util.toLocalHMTimeString(v.entry.date));
        let age = ageString(v.entry);
        lines.push(age !== "-" ? age + 'm' : "-");
    }
    if (!isStationEnabled( s)) lines.push( "not shown");
    return lines.join("\n");
}

function productLabel (productName) {
    return (config.products.find( p => p.name === productName) || {}).label || productName;
}

function hasData (s) {
    return Object.values( s.products).some( es => es.length > 0);
}

function updateStationEntity (s) {
    if (s.entity) s.entity.point.color = hasData(s) ? config.station.color : config.station.noDataColor;
    if (s.infoEntity) s.infoEntity.label.text = stationInfoText( s);
}

function updateStationEntities() {
    for (let s of stations.values()) updateStationEntity( s);
}

// list selection: scroll the active entry view to the displayed (or newest) time step of the station
function selectStation (event) {
    selectedStation = ui.getSelectedListItem( stationView);
    if (selectedStation) {
        let v = shown.get( selectedStation.id);
        let e = (v && v.product === activeProduct) ? v.entry : (selectedStation.products[activeProduct] || [])[0];
        let ie = e ? ui.getElementOfListItem( entryViews[activeProduct], e) : null;
        if (ie) ie.scrollIntoView( {behavior: "smooth", block: "nearest"});
    }
}

// map -> list
function stationSelection() {
    let sel = odinCesium.getSelectedEntity();
    if (sel && sel._uiNexradStation && sel._uiNexradStation !== selectedStation) {
        ui.setSelectedListItem( stationView, sel._uiNexradStation);
    }
}

function zoomToStation (event) {
    let s = ui.getSelectedListItem( stationView);
    if (s) {
        odinCesium.zoomTo( Cesium.Cartesian3.fromDegrees( s.lon, s.lat, 800000));
        if (s.entity) odinCesium.setSelectedEntity( s.entity);
    }
}

function isStationEnabled (s) {
    return !disabledStations.has( s.id);
}

// a station has a visible time step (shown and enabled)
function isDisplayed (s) {
    return shown.has( s.id) && isStationEnabled( s);
}

function toggleStationEnabled (event) {
    let cb = ui.getCheckBox( event.target);
    if (cb) {
        let s = ui.getListItemOfElement( cb);
        if (s) setStationEnabled( s, ui.isCheckBoxSelected( cb));
    }
}

// de-selected stations keep their shown time step (if any, and still updated by the time cursor) as a hidden layer so
// that it re-appears once the station is selected again. Their time steps are removed from the entry views
function setStationEnabled (s, isEnabled) {
    if (isEnabled) disabledStations.delete( s.id); else disabledStations.add( s.id);

    let v = shown.get( s.id);
    if (v) v.layer.show = isLayerVisible && isEnabled;

    updateEntryViews();
    if (isShowLatest()) showLatestOfAll(); // the newest time step of the shown stations might have changed

    clearValues();
    updateViews();
    odinCesium.requestRender();
}

// the entry views list the time steps of all shown stations (newest first). This preserves the selection
function updateEntryViews() {
    config.products.forEach( p => {
        let list = entryViews[p.name];
        let sel = ui.getSelectedListItem( list);
        ui.setListItems( list, mergedEntries( p.name));
        if (sel && ui.getElementOfListItem( list, sel)) ui.setSelectedListItem( list, sel);
    });
}

function mergedEntries (productName) {
    let es = [];
    for (let s of stations.values()) {
        if (isStationEnabled( s)) es.push( ...(s.products[productName] || []));
    }
    return es.sort( (a,b) => (b.date - a.date) || a.station.localeCompare( b.station));
}

/* #endregion stations */

/* #region time step display ****************************************************************************************/

function selectProductTab (productName) {
    activeProduct = productName;
    if (isShowLatest()) showLatestOfAll();
    else if (cursorTime !== undefined) showTimeSlice( cursorTime);
}

function isShowLatest() {
    return ui.isCheckBoxSelected( showLatestCb);
}

function toggleShowLatest (event) {
    if (isShowLatest()) {
        Object.values( entryViews).forEach( list => ui.clearSelectedListItem( list));
        showLatestOfAll();
    } else {
        cursorTime = undefined;
        hideAll();
        updateCursor();
    }
}

// set the time cursor to the newest time step of the active product of all shown stations
function showLatestOfAll() {
    let latest = mergedEntries( activeProduct)[0];
    if (latest) showTimeSlice( latest.date); else { cursorTime = undefined; hideAll(); updateCursor(); }
}

// display the newest time step <= t (and not older than config.maxAge) of the active product for each station
function showTimeSlice (t) {
    cursorTime = t;
    for (let s of stations.values()) {
        let e = entryAt( s, activeProduct, t);
        if (e) show( s, activeProduct, e); else hide( s);
    }
    clearValues();
    updateCursor();
}

// newest time step of a station product at time t (undefined if there is none or it is older than maxAge)
function entryAt (s, productName, t) {
    let e = (s.products[productName] || []).find( e => e.date <= t);
    return (e && (t - e.date) <= maxAge * 60000) ? e : undefined;
}

function isShown (e) {
    let v = shown.get( e.station);
    return v ? v.entry === e : false;
}

// manual mode: only change the display of the station of the entry
function toggleShowEntry (event) {
    let cb = ui.getCheckBox( event.target);
    let e = cb ? ui.getListItemOfElement( cb) : null;
    let s = e ? stations.get( e.station) : null;
    if (s) {
        ui.setCheckBox( showLatestCb, false); // explicit selection resets "show latest" (other stations keep their time steps)
        cursorTime = undefined;
        if (ui.isCheckBoxSelected(cb)) show( s, e.product, e); else hide( s);
        clearValues();
        updateCursor();
    }
}

// list controls: move the time cursor to the time step selected by `selectFn` in the active product tab
function stepEntry (selectFn) {
    let list = entryViews[activeProduct];
    if (!list) return;

    // if nothing is selected yet start from the current time cursor (if any)
    if (!ui.getSelectedListItem( list) && cursorTime !== undefined) {
        let e = mergedEntries( activeProduct).find( e => e.date <= cursorTime);
        if (e) ui.setSelectedListItem( list, e);
    }

    selectFn( list);
    let e = ui.getSelectedListItem( list);
    if (e) {
        ui.setCheckBox( showLatestCb, false); // explicit selection resets "show latest"
        showTimeSlice( e.date);
    }
}

// list controls: clear the time step selection and hide all time steps
function clearEntry() {
    Object.values( entryViews).forEach( list => ui.clearSelectedListItem( list));
    ui.setCheckBox( showLatestCb, false);
    cursorTime = undefined;
    hideAll();
    clearValues();
    updateCursor();
}

// the reference time for ages: the time cursor, or the newest displayed time step in manual mode
function referenceTime() {
    if (cursorTime !== undefined) return cursorTime;
    let dates = [...shown.values()].map( v => v.entry.date);
    return dates.length > 0 ? Math.max( ...dates) : undefined;
}

// the entry of a station for which we report the age: the displayed one, or the one that would be displayed at the
// time cursor (if the station is not shown)
function stationAgeEntry (s) {
    let v = shown.get( s.id);
    if (v) return v.entry;
    return (cursorTime !== undefined) ? (s.products[activeProduct] || []).find( e => e.date <= cursorTime) : undefined;
}

function ageString (e) {
    let t = referenceTime();
    return (e && t !== undefined) ? Math.round( (t - e.date) / 60000).toString() : "-";
}

function updateCursor() {
    let text;
    if (cursorTime !== undefined) {
        text = util.toLocalMDHMSString( cursorTime) + (isShowLatest() ? " (latest)" : "");
    } else {
        text = (shown.size > 0) ? "manual" : "-";
    }
    ui.setVarText( cursorText, text);
    updateViews();
}

// replace the shown time step of a station (if any)
async function show (s, productName, e) {
    let prev = shown.get( s.id);
    if (prev && prev.entry === e) return;

    let viewer = await odinCesium.viewerReadyPromise;
    let layer = new Cesium.ImageryLayer( wmsProvider( s, e.layer), {
        magnificationFilter: Cesium.TextureMagnificationFilter.NEAREST, // codes can't be interpolated
        minificationFilter: Cesium.TextureMinificationFilter.NEAREST
    });
    layer.alpha = opacity;
    layer.show = isLayerVisible && isStationEnabled( s);
    viewer.imageryLayers.add( layer);

    prev = shown.get( s.id); // might have changed while waiting
    shown.set( s.id, { product: productName, entry: e, layer });
    if (prev) viewer.imageryLayers.remove( prev.layer, true);
    orderLayers(); // new layers are added on top

    updateViews();
    odinCesium.requestRender();
}

function hide (s) {
    let v = shown.get( s.id);
    if (v) {
        odinCesium.viewer.imageryLayers.remove( v.layer, true);
        shown.delete( s.id);
        updateViews();
        odinCesium.requestRender();
    }
}

function hideAll() {
    for (let s of stations.values()) hide( s);
}

function wmsProvider (s, layerName) {
    let opts = {
        url: "nexrad/wms",
        layers: layerName,
        parameters: { format: "image/png", transparent: true },
        maximumLevel: config.maximumLevel,
        enablePickFeatures: true, // used for value lookup (WMS GetFeatureInfo)
        getFeatureInfoFormats: [ new Cesium.GetFeatureInfoFormat( "json", "application/json") ]
    };
    if (s.extent) opts.rectangle = Cesium.Rectangle.fromDegrees( ...s.extent); // don't request tiles outside of the station range
    return new Cesium.WebMapServiceImageryProvider( opts);
}

function updateViews() {
    ui.updateListItems( stationView);
    Object.values( entryViews).forEach( v => ui.updateListItems( v));
    ui.updateListItems( valueView); // ages might have changed
    updateStationEntities();
}

function toggleNearestOnTop (event) {
    isNearestOnTop = ui.isCheckBoxSelected( event.target);
    orderLayers();
}

function maxAgeChanged (event) {
    maxAge = ui.getSliderValue( event.target);
    if (isShowLatest()) showLatestOfAll();
    else if (cursorTime !== undefined) showTimeSlice( cursorTime);
}

// if isNearestOnTop, order the shown layers so that the station closest to the view center is on top. This is called
// after camera moves and when layers are added. Note that we only re-stack (raise) our own layers if their order has
// changed, which does not require tile requests
function orderLayers() {
    if (!isNearestOnTop || shown.size < 2) return;
    let viewer = odinCesium.viewer;
    let center = viewCenter( viewer);
    if (!center) return;

    let layers = viewer.imageryLayers;
    let entries = [...shown.entries()].map( ([id,v]) => {
        let s = stations.get( id);
        let d = s ? Cesium.Cartesian3.distance( center, Cesium.Cartesian3.fromDegrees( s.lon, s.lat)) : Number.MAX_VALUE;
        return { layer: v.layer, d, idx: layers.indexOf( v.layer) };
    });
    let current = [...entries].sort( (a,b) => a.idx - b.idx);  // bottom to top
    let desired = [...entries].sort( (a,b) => b.d - a.d);      // farthest at bottom, nearest on top

    if (desired.some( (e,i) => e !== current[i])) {
        desired.forEach( e => layers.raiseToTop( e.layer));
        odinCesium.requestRender();
    }
}

// ellipsoid point at the center of the canvas, or below the camera if the center does not hit the globe
function viewCenter (viewer) {
    let canvas = viewer.scene.canvas;
    let p = viewer.camera.pickEllipsoid( new Cesium.Cartesian2( canvas.clientWidth / 2, canvas.clientHeight / 2));
    if (!p) {
        let c = viewer.camera.positionCartographic;
        p = Cesium.Cartesian3.fromRadians( c.longitude, c.latitude);
    }
    return p;
}

function opacityChanged (event) {
    opacity = ui.getSliderValue( event.target);
    for (let v of shown.values()) v.layer.alpha = opacity;
    odinCesium.requestRender();
}

function showNexrad (cond) { // layer panel
    isLayerVisible = cond;
    stationDataSource.show = cond;
    for (let [id,v] of shown.entries()) v.layer.show = cond && !disabledStations.has( id);
    odinCesium.requestRender();
}

/* #endregion time step display */

/* #region value lookup *********************************************************************************************/

function togglePickMode() {
    if (isPicking) {
        exitPickMode();
    } else if (displayedLayers().size > 0) {
        isPicking = true;
        ui.setElementColors( pickBtn, ui.getRootVar("--selected-data-color"), ui.getRootVar("--selection-background"));
        odinCesium.setCursor("crosshair");
    }
}

// Cesium.ImageryLayer -> { id, product, entry, layer } of shown time steps of enabled stations
function displayedLayers() {
    return new Map( [...shown.entries()].filter( ([id,v]) => !disabledStations.has( id)).map( ([id,v]) => [v.layer, { id, ...v }]));
}

function exitPickMode() {
    if (isPicking) {
        isPicking = false;
        ui.resetElementColors( pickBtn);
        odinCesium.setDefaultCursor();
    }
}

function handleKeyDown (event) {
    if (isPicking && event.key === "Escape") {
        exitPickMode();
    }
}

function handleMouseClick (event) {
    if (isPicking || (event.shiftKey && ui.isTopWindow( nexradWindow))) {
        exitPickMode(); // picks are single shot
        lookupAt( event.clientX, event.clientY);
    }
}

// value lookup of all displayed time steps (shown, of enabled stations) through WMS GetFeatureInfo (does not depend
// on rendering parameters such as opacity)
function lookupAt (x, y) {
    let layers = displayedLayers();
    if (layers.size == 0 || !isLayerVisible) return;

    let viewer = odinCesium.viewer;
    let ray = viewer.camera.getPickRay( new Cesium.Cartesian2(x, y));
    // Cesium only picks shown imagery layers, i.e. disabled stations are not queried
    let promise = ray ? viewer.imageryLayers.pickImageryLayerFeatures( ray, viewer.scene) : undefined;
    if (!promise) { clearValues(); return; }

    promise.then( (features) => {
        // features of other pickable layers are ignored. Stations can overlap, i.e. we can have several values
        let valueEntries = (features || []).filter( f => layers.has( f.imageryLayer) && f.data && f.data.properties).map( f => {
            let v = layers.get( f.imageryLayer);
            return { id: v.id, entry: v.entry, value: getValue( f.data.properties.name) };
        });
        ui.setListItems(valueView, valueEntries);
    }).catch( (err) => {
        console.log("NEXRAD value lookup failed: ", err);
        clearValues();
    });
}

function getValue(propName) {
    if (propName === "nodata") {
        return "-";
    } else {
        let s = propName.split(' ')[0];
        let n = parseFloat(s);
        return (isNaN(n)) ? '?' : util.f_1.format(n);
    }
}

function clearValues() {
    ui.clearList(valueView);
}

/* #endregion value lookup */

/* #region websocket messages ***************************************************************************************/

function handleWsMessages (msgType, msg) {
    switch (msgType) {
        case "snapshot": handleSnapshot( msg); break;
        case "update": handleUpdate( msg); break;
    }
}

// { products: [{name,title},..], stations: [{id,name,lat,lon,extent,products:[{name,entries:[{date,layer},..]},..]},..] }
// we get this for each (re-)connect
function handleSnapshot (msg) {
    let selectedId = selectedStation ? selectedStation.id : undefined;

    hideAll();
    stationDataSource.entities.removeAll();
    stations = new Map();
    msg.stations.forEach( st => {
        let s = { id: st.id, name: st.name, lat: st.lat, lon: st.lon, extent: st.extent, products: {} };
        st.products.forEach( p => {
            s.products[p.name] = p.entries.map( e => ({ station: s.id, product: p.name, date: e.date, layer: e.layer }));
        });
        s.entity = createStationEntity( s);
        s.infoEntity = createStationInfoEntity( s);
        stations.set( s.id, s);
    });

    selectedStation = undefined;
    ui.setListItems( stationView, [...stations.values()]);
    updateEntryViews();
    let s = stations.get( selectedId);
    if (s) ui.setSelectedListItem( stationView, s);

    if (isShowLatest()) showLatestOfAll();
    else if (cursorTime !== undefined) showTimeSlice( cursorTime);
    else updateCursor();
    odinCesium.requestRender();
}

// { station, added: [{product,date,layer},..], removed: [layer,..] }
function handleUpdate (msg) {
    let s = stations.get( msg.station);
    if (!s) return;

    for (let p in s.products) s.products[p] = s.products[p].filter( e => !msg.removed.includes( e.layer));
    msg.added.forEach( a => {
        let es = s.products[a.product] || (s.products[a.product] = []);
        es.push( { station: s.id, product: a.product, date: a.date, layer: a.layer });
        es.sort( (a,b) => b.date - a.date); // newest first
    });
    updateEntryViews();

    let v = shown.get( s.id);
    if (isShowLatest()) {
        showLatestOfAll(); // the new time step might move the cursor
    } else if (v && msg.removed.includes( v.entry.layer)) {
        hide( s);
    }

    updateViews();
    odinCesium.requestRender();
}

/* #endregion websocket messages */
