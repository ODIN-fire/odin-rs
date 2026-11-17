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
// a WMS layer of our server. Stations are shown on the map and listed in the window - selecting a station shows its
// time steps in one tab per product. Each station shows at most one time step at a time

import { config } from "./odin_nexrad_config.js";

import * as util from "../odin_server/ui_util.js";
import * as ui from "../odin_server/ui.js";
import * as ws from "../odin_server/ws.js";
import * as odinCesium from "../odin_cesium/odin_cesium.js";

const MOD_PATH = "odin_nexrad::service::NexradService";
const WID = "nexrad"; // window id

ws.addWsHandler( MOD_PATH, handleWsMessages);

/* #region data *****************************************************************************************************/

// station id -> { id, name, lat, lon, extent:[w,s,e,n], products: { <product name>: [ {date,layer},.. ] (newest first) }, entity }
var stations = new Map();
var selectedStation = undefined;
var activeProduct = config.product;

// station id -> { product, entry, layer: Cesium.ImageryLayer } of shown time steps
var shown = new Map();
// ids of stations the user has de-selected ("show" checkbox). Their time steps are kept but not displayed or queried
var disabledStations = new Set();
var opacity = config.opacity;
var isLayerVisible = config.layer.show; // layer panel visibility

var stationView = undefined;
var entryViews = {}; // product name -> list
var valueView = undefined;
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
            (showLatestCb = ui.CheckBox("show latest of all stations", toggleShowLatest, WID + ".showLatest", config.showLatest)),
            ui.RowContainer("start")(
                ui.TabbedContainer(WID + ".products")(...tabs),
                ui.HorizontalSpacer(0.5),
                ui.ColumnContainer()(
                    (pickBtn = ui.Button("pick", togglePickMode)),
                    (valueView = ui.List( WID + ".values", 6))
                )
            )
        ),
        ui.Panel("display", false)(
            ui.Slider("opacity", WID + ".opacity", opacityChanged, { min: 0, max: 1.0, step: 0.05, fmt: util.f_2, value: opacity })
        ),
    );
}

function initStationView() {
    ui.setListItemDisplayColumns( stationView, ["fit", "header"], [
        { name: "show", tip: "show station data", width: "2.5rem", attrs: [], map: s => ui.createCheckBox( isStationEnabled(s), toggleStationEnabled) },
        { name: "", tip: "shown time step", width: "1.5rem", attrs: [], map: s => isDisplayed( s) ? "●" : "" },
        { name: "id", width: "3.5rem", attrs: [], map: s => s.id },
        { name: "name", width: "12rem", attrs: [], map: s => util.maxString( s.name, 26) },
        ui.listItemSpacerColumn(),
        { name: "latest", tip: "scan time of latest time step", width: "8rem", attrs: ["fixed", "alignRight"], map: s => latestDateString(s) }
    ]);
}

function initValueView() {
    ui.setListItemDisplayColumns( valueView, ["fit", "header"], [
        { name: "id", width: "3.5rem", attrs: [], map: s => s.id },
        ui.listItemSpacerColumn(),
        { name: "val", tip: "reflectivity value [dBZ]", width: "4rem", attrs: ["fixed", "alignRight"], map: s => s.value }
    ]);
}

function initEntryView (productName) {
    ui.setListItemDisplayColumns( entryViews[productName], ["fit", "header"], [
        { name: "show", tip: "show time step (resets 'show latest')", width: "2.5rem", attrs: [], map: e => ui.createCheckBox( isShown(e), toggleShowEntry) },
        ui.listItemSpacerColumn(),
        { name: "scan time", width: "10rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString( e.date) }
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
            fillColor: config.station.color,
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

function hasData (s) {
    return Object.values( s.products).some( es => es.length > 0);
}

function updateStationEntity (s) {
    if (s.entity) s.entity.point.color = hasData(s) ? config.station.color : config.station.noDataColor;
}

// list -> time steps
function selectStation (event) {
    let s = ui.getSelectedListItem( stationView);
    if (s !== selectedStation) {
        selectedStation = s;
        updateEntryViews();
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

// de-selected stations keep their shown time step (if any, and still updated by "show latest") as a hidden layer so
// that it re-appears once the station is selected again
function setStationEnabled (s, isEnabled) {
    if (isEnabled) disabledStations.delete( s.id); else disabledStations.add( s.id);

    let v = shown.get( s.id);
    if (v) v.layer.show = isLayerVisible && isEnabled;

    clearValues();
    ui.updateListItems( stationView);
    odinCesium.requestRender();
}

function updateEntryViews() {
    config.products.forEach( p => {
        let es = selectedStation ? (selectedStation.products[p.name] || []) : [];
        ui.setListItems( entryViews[p.name], es);
    });
}

/* #endregion stations */

/* #region time step display ****************************************************************************************/

function selectProductTab (productName) {
    activeProduct = productName;
    if (isShowLatest()) showLatestOfAll();
}

function isShowLatest() {
    return ui.isCheckBoxSelected( showLatestCb);
}

function toggleShowLatest (event) {
    if (isShowLatest()) showLatestOfAll(); else hideAll();
}

// show the newest time step of the active product for all stations
function showLatestOfAll() {
    for (let s of stations.values()) showLatest( s);
}

function showLatest (s) {
    let latest = (s.products[activeProduct] || [])[0];
    if (latest) show(s, activeProduct, latest); else hide(s);
    clearValues();
}

function isShown (e) {
    for (let v of shown.values()) { if (v.entry === e) return true; }
    return false;
}

function toggleShowEntry (event) {
    let cb = ui.getCheckBox( event.target);
    if (cb && selectedStation) {
        let e = ui.getListItemOfElement( cb);
        let productName = Object.keys( selectedStation.products).find( p => selectedStation.products[p].includes( e));
        if (e && productName) {
            ui.setCheckBox( showLatestCb, false); // explicit selection resets "show latest" (other stations keep their time steps)
            if (ui.isCheckBoxSelected(cb)) {
                if (!isStationEnabled( selectedStation)) setStationEnabled( selectedStation, true); // explicit show implies station display
                show(selectedStation, productName, e);
            } else {
                hide(selectedStation);
            }
            clearValues();
        }
    }
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
            return { id: v.id, value: getValue( f.data.properties.name) };
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
        st.products.forEach( p => s.products[p.name] = p.entries);
        s.entity = createStationEntity( s);
        stations.set( s.id, s);
    });

    selectedStation = undefined;
    ui.setListItems( stationView, [...stations.values()]);
    let s = stations.get( selectedId) || stations.values().next().value;
    if (s) ui.setSelectedListItem( stationView, s); else updateEntryViews();

    if (isShowLatest()) showLatestOfAll();
    odinCesium.requestRender();
}

// { station, added: [{product,date,layer},..], removed: [layer,..] }
function handleUpdate (msg) {
    let s = stations.get( msg.station);
    if (!s) return;

    for (let p in s.products) s.products[p] = s.products[p].filter( e => !msg.removed.includes( e.layer));
    msg.added.forEach( a => {
        let es = s.products[a.product] || (s.products[a.product] = []);
        es.push( { date: a.date, layer: a.layer });
        es.sort( (a,b) => b.date - a.date); // newest first
    });

    let v = shown.get( s.id);
    if (isShowLatest()) {
        showLatest( s);
    } else if (v && msg.removed.includes( v.entry.layer)) {
        hide( s);
    }

    if (s === selectedStation) updateEntryViews();
    updateStationEntity( s);
    ui.updateListItems( stationView);
    odinCesium.requestRender();
}

/* #endregion websocket messages */
