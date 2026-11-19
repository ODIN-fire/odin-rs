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
//  - "latest showing": the cursor is the newest time step of the shown stations (following new imports)
//  - stepping (list controls): the cursor is the selected time step
// For a cursor T each station displays its newest time step <= T that is not older than T - maxAge (display panel).
// Where stations overlap the layer of the station that is closest to the view center can be put on top (display panel),
// which together with maxAge approximates a mosaic of the shown stations. Users can still exclude stations explicitly
// Explicitly checking/unchecking a time step (manual mode) only changes the respective station.
// Ages (station and value lists, station info labels) are relative to the time cursor (or the newest displayed time
// step in manual mode)
// Alternatively the "overview" checkbox shows server computed mosaics of all stations (odin_nexrad::mosaic) as smoothed
// reflectivity band polygons instead of the station layers. Selecting it enables all stations. Mosaics are computed
// periodically from the latest time steps of all stations and have their own time steps (the newest scan time of their
// inputs), which replace the station time steps in the product tabs while in overview mode. "latest showing" and the
// list controls work the same way for mosaics. Station views and ages refer to the station time steps of the displayed
// mosaic. Switching between modes keeps the time (if "latest showing" is not set)

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

// product name -> [ mosaic,.. ] (newest first) with mosaic: { isMosaic: true, product, date, file, stations: [{id,date},..] }
var mosaics = {};
var isOverview = config.overview;
var overviewShown = undefined; // the displayed mosaic (of the active product)
var overviewDs = undefined;    // Cesium.GeoJsonDataSource of the displayed mosaic
var overviewFile = undefined;  // file of the displayed (or loading) mosaic

var stationView = undefined;
var entryViews = {}; // product name -> list
var valueView = undefined;
var cursorText = undefined;
var showLatestCb = undefined;
var overviewCb = undefined;
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
                (showLatestCb = ui.CheckBox("latest showing", toggleShowLatest, WID + ".showLatest", config.showLatest)),
                ui.HorizontalSpacer(1),
                (overviewCb = ui.CheckBox("overview", toggleOverview, WID + ".overview", isOverview))
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
        { name: "show", tip: "show time step (resets 'latest showing')", width: "2.5rem", attrs: [], map: e => ui.createCheckBox( isShown(e), toggleShowEntry) },
        { name: "id", tip: "station (or 'ovw' for overview mosaics)", width: "3.5rem", attrs: [], map: e => e.isMosaic ? "ovw" : e.station },
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
    let v = isOverview ? overviewStationEntry( s) : shown.get( s.id);
    if (v) {
        let product = productLabel(v.product) + (isOverview ? " (overview)" : "");
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

// a station has a visible time step (shown and enabled, or used in the displayed mosaic)
function isDisplayed (s) {
    return isOverview ? overviewStationEntry( s) !== undefined : shown.has( s.id) && isStationEnabled( s);
}

// visibility of the station layer (if shown)
function isStationLayerVisible (id) {
    return isLayerVisible && !isOverview && !disabledStations.has( id);
}

function toggleStationEnabled (event) {
    let cb = ui.getCheckBox( event.target);
    if (cb) {
        let s = ui.getListItemOfElement( cb);
        if (s) {
            let isEnabled = ui.isCheckBoxSelected( cb);
            if (!isEnabled && isOverview) setOverview( false); // mosaics always include all stations
            setStationEnabled( s, isEnabled);
        }
    }
}

// de-selected stations keep their shown time step (if any, and still updated by the time cursor) as a hidden layer so
// that it re-appears once the station is selected again. Their time steps are removed from the entry views
function setStationEnabled (s, isEnabled) {
    if (isEnabled) disabledStations.delete( s.id); else disabledStations.add( s.id);

    let v = shown.get( s.id);
    if (v) v.layer.show = isStationLayerVisible( s.id);

    updateEntryViews();
    if (isShowLatest()) showLatestOfAll(); // the newest time step of the shown stations might have changed

    clearValues();
    updateViews();
    odinCesium.requestRender();
}

// the entry views list the time steps of all shown stations or (in overview mode) the mosaics (newest first). This
// preserves the selection
function updateEntryViews() {
    config.products.forEach( p => {
        let list = entryViews[p.name];
        let sel = ui.getSelectedListItem( list);
        ui.setListItems( list, isOverview ? (mosaics[p.name] || []) : mergedEntries( p.name));
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
    let t = overviewShown ? overviewShown.date : undefined;
    activeProduct = productName;
    if (isOverview) {
        if (isShowLatest()) showLatestMosaic();
        else if (t !== undefined) showMosaic( mosaicAt( activeProduct, t));
    }
    if (isShowLatest()) showLatestOfAll();
    else if (cursorTime !== undefined) showTimeSlice( cursorTime);
}

function isShowLatest() {
    return ui.isCheckBoxSelected( showLatestCb);
}

// this applies to both station time steps and mosaics (the mode we don't display keeps up to date)
function toggleShowLatest (event) {
    if (isShowLatest()) {
        Object.values( entryViews).forEach( list => ui.clearSelectedListItem( list));
        showLatestOfAll();
        if (isOverview) showLatestMosaic();
    } else {
        cursorTime = undefined;
        hideAll();
        if (isOverview) showMosaic( undefined);
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
    if (e.isMosaic) return e === overviewShown;
    let v = shown.get( e.station);
    return v ? v.entry === e : false;
}

// manual mode: only change the display of the station of the entry
function toggleShowEntry (event) {
    let cb = ui.getCheckBox( event.target);
    let e = cb ? ui.getListItemOfElement( cb) : null;
    if (e && e.isMosaic) {
        ui.setCheckBox( showLatestCb, false);
        showMosaic( ui.isCheckBoxSelected(cb) ? e : undefined);
        return;
    }
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

    // if nothing is selected yet start from the current time cursor or displayed mosaic (if any)
    if (!ui.getSelectedListItem( list)) {
        let e = isOverview ? overviewShown
                : (cursorTime !== undefined) ? mergedEntries( activeProduct).find( e => e.date <= cursorTime) : undefined;
        if (e) ui.setSelectedListItem( list, e);
    }

    selectFn( list);
    let e = ui.getSelectedListItem( list);
    if (e) {
        ui.setCheckBox( showLatestCb, false); // explicit selection resets "latest showing"
        if (e.isMosaic) showMosaic( e); else showTimeSlice( e.date);
    }
}

// list controls: clear the time step selection and hide all time steps
function clearEntry() {
    Object.values( entryViews).forEach( list => ui.clearSelectedListItem( list));
    ui.setCheckBox( showLatestCb, false);
    cursorTime = undefined;
    hideAll();
    if (isOverview) showMosaic( undefined);
    clearValues();
    updateCursor();
}

// the reference time for ages: the time cursor, or the newest displayed time step in manual mode
function referenceTime() {
    if (isOverview) return overviewShown ? overviewShown.date : undefined;
    if (cursorTime !== undefined) return cursorTime;
    let dates = [...shown.values()].map( v => v.entry.date);
    return dates.length > 0 ? Math.max( ...dates) : undefined;
}

// the entry of a station for which we report the age: the displayed one, or the one that would be displayed at the
// time cursor (if the station is not shown)
function stationAgeEntry (s) {
    if (isOverview) { let v = overviewStationEntry( s); return v ? v.entry : undefined; }
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
    if (isOverview) {
        text = overviewShown ? util.toLocalMDHMSString( overviewShown.date) + " (overview)"
                             : ((mosaics[activeProduct] || []).length > 0 ? "-" : "no overview");
    } else if (cursorTime !== undefined) {
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
    layer.show = isStationLayerVisible( s.id);
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
    if (overviewDs) setOverviewColors( overviewDs);
    odinCesium.requestRender();
}

function showNexrad (cond) { // layer panel
    isLayerVisible = cond;
    stationDataSource.show = cond;
    for (let [id,v] of shown.entries()) v.layer.show = isStationLayerVisible( id);
    if (overviewDs) overviewDs.show = cond;
    odinCesium.requestRender();
}

/* #endregion time step display */

/* #region overview *************************************************************************************************/

function toggleOverview (event) {
    setOverview( ui.isCheckBoxSelected( overviewCb));
}

// switch between station layers and server mosaics of the active product. Station layers are kept (hidden). Turning
// the overview on enables all stations (mosaics include them) and shows the latest mosaic (if "latest showing" is set)
// or the one at the current time. Turning it off shows the station time steps at the time of the displayed mosaic
// (if "latest showing" is not set)
function setOverview (cond) {
    if (cond === isOverview) return;
    let t = isShowLatest() ? undefined : referenceTime();
    isOverview = cond;
    ui.setCheckBox( overviewCb, cond);
    Object.values( entryViews).forEach( list => ui.clearSelectedListItem( list));

    if (cond) {
        for (let id of [...disabledStations]) { let s = stations.get( id); if (s) setStationEnabled( s, true); }
        if (isShowLatest()) showLatestMosaic(); else showMosaic( t !== undefined ? mosaicAt( activeProduct, t) : undefined);
    } else {
        if (!isShowLatest() && t !== undefined) showTimeSlice( t);
        showMosaic( undefined);
    }
    for (let [id,v] of shown.entries()) v.layer.show = isStationLayerVisible( id);

    updateEntryViews();
    exitPickMode();
    clearValues();
    updateCursor();
    odinCesium.requestRender();
}

// the newest mosaic of a product with a date <= t (undefined if none)
function mosaicAt (productName, t) {
    return (mosaics[productName] || []).find( m => m.date <= t);
}

function showLatestMosaic() {
    showMosaic( (mosaics[activeProduct] || [])[0]);
}

// set the displayed mosaic (undefined hides the overview)
function showMosaic (m) {
    overviewShown = m;
    if (m && isOverview) loadOverview( m); else hideOverview();
    clearValues();
    updateCursor();
}

// the station time step that is used in the displayed mosaic: { product, entry }
function overviewStationEntry (s) {
    let m = overviewShown;
    let ms = (m && s) ? m.stations.find( x => x.id === s.id) : undefined;
    if (!ms) return undefined;
    let e = (s.products[m.product] || []).find( e => e.date === ms.date);
    return { product: m.product, entry: e || { station: s.id, product: m.product, date: ms.date } };
}

// load the GeoJSON of a mosaic if it is not displayed (or loading) yet. The previous data source is only removed once
// the new one is loaded (no flicker)
async function loadOverview (m) {
    if (m.file === overviewFile) return;

    overviewFile = m.file;
    try {
        // mosaics can be replaced by ones with the same date (and file), i.e. we always revalidate
        let response = await fetch( `${config.overviewUrl}/${m.file}`, { cache: "no-cache" });
        if (!response.ok) throw new Error( response.statusText);
        let data = await response.json();
        let ds = await Cesium.GeoJsonDataSource.load( data, { clampToGround: true });
        if (!isOverview || overviewFile !== m.file) return; // superseded while loading

        setOverviewColors( ds);
        ds.show = isLayerVisible;
        let prev = overviewDs;
        overviewDs = ds;
        await odinCesium.addDataSource( ds);
        if (prev) odinCesium.removeDataSource( prev);
        console.log("NEXRAD overview", m.file, ":", ds.entities.values.length, "polygons");
    } catch (err) {
        console.log("failed to load NEXRAD overview", m.file, err);
        if (overviewFile === m.file) overviewFile = undefined;
    }
    updateViews();
    odinCesium.requestRender();
}

function hideOverview() {
    if (overviewDs) odinCesium.removeDataSource( overviewDs);
    overviewDs = undefined;
    overviewFile = undefined;
    odinCesium.requestRender();
}

// fill color of a band: the configured color with the highest dBZ <= the lower band bound, or the server default color
function bandColor (e) {
    let lo = e.properties && e.properties.lo ? e.properties.lo.getValue() : undefined;
    let cc = (lo !== undefined) ? (config.overviewColors || []).filter( c => c[0] <= lo).pop() : undefined;
    if (cc) return cc[1];
    return e.properties && e.properties.color ? e.properties.color.getValue() : "#00ff00";
}

// band polygons are clamped to ground (no height)
function setOverviewColors (ds) {
    for (let e of ds.entities.values) {
        if (e.polygon) {
            e.polygon.material = new Cesium.ColorMaterialProperty( Cesium.Color.fromCssColorString( bandColor( e)).withAlpha( opacity));
            e.polygon.outline = false;
        }
    }
}

// value lookup in overview mode: the reflectivity band at the screen position
function lookupOverviewAt (x, y) {
    let viewer = odinCesium.viewer;
    let picked = viewer.scene.drillPick( new Cesium.Cartesian2(x, y));
    let e = picked.map( p => p.id).find( e => e && overviewDs && overviewDs.entities.contains( e));
    let value = "-";
    if (e && e.properties) {
        let lo = e.properties.lo.getValue();
        let hi = e.properties.hi ? e.properties.hi.getValue() : null;
        value = (hi !== null && hi !== undefined) ? `${lo}-${hi}` : `>${lo}`;
    }
    ui.setListItems( valueView, [{ id: "ovw", entry: overviewShown, value }]);
}

/* #endregion overview */

/* #region value lookup *********************************************************************************************/

function togglePickMode() {
    if (isPicking) {
        exitPickMode();
    } else if (isOverview ? overviewDs !== undefined : displayedLayers().size > 0) {
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
        if (isOverview) lookupOverviewAt( event.clientX, event.clientY); else lookupAt( event.clientX, event.clientY);
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
        case "mosaic": handleMosaic( msg); break;
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

    let shownMosaic = overviewShown;
    mosaics = {};
    (msg.mosaics || []).forEach( m => addMosaic( m));

    selectedStation = undefined;
    ui.setListItems( stationView, [...stations.values()]);
    updateEntryViews();
    let s = stations.get( selectedId);
    if (s) ui.setSelectedListItem( stationView, s);

    if (isShowLatest()) showLatestOfAll();
    else if (cursorTime !== undefined) showTimeSlice( cursorTime);
    else updateCursor();
    if (isOverview) {
        if (isShowLatest()) showLatestMosaic();
        else showMosaic( shownMosaic ? (mosaics[activeProduct] || []).find( m => m.date === shownMosaic.date) : undefined);
    }
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

// { product, date, file, stations: [{id,date},..], removed: [date,..] } - a new server mosaic and the dates of the ones
// it replaced. A mosaic with the same date as a previous one replaces it (and its file). We only load the GeoJSON if it
// is displayed
function handleMosaic (msg) {
    let shownDate = overviewShown ? overviewShown.date : undefined;
    let m = addMosaic( msg);

    if (isOverview && m.product === activeProduct) {
        if (isShowLatest()) {
            if (shownDate === m.date) overviewFile = undefined; // force reload of replaced file
            showLatestMosaic();
        } else if (shownDate === m.date) {
            overviewFile = undefined;
            showMosaic( m);
        } else if (shownDate !== undefined && msg.removed.includes( shownDate)) {
            showMosaic( undefined);
        }
    }
    updateEntryViews();
    updateViews();
}

// add a server mosaic to the (newest first) list of its product, replacing one with the same date and dropping removed ones
function addMosaic (msg) {
    let m = { isMosaic: true, product: msg.product, date: msg.date, file: msg.file, stations: msg.stations };
    let removed = msg.removed || [];
    let list = (mosaics[m.product] || []).filter( x => x.date !== m.date && !removed.includes( x.date));
    list.push( m);
    list.sort( (a,b) => b.date - a.date);
    mosaics[m.product] = list;
    return m;
}

/* #endregion websocket messages */
