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

import { config } from "./odin_nfdrs_config.js";

import * as util from "../odin_server/ui_util.js";
import * as data from "../odin_server/ui_data.js";
import * as ui from "../odin_server/ui.js";
import * as ws from "../odin_server/ws.js";
import * as main from "../odin_server/main.js";
import * as odinCesium from "../odin_cesium/odin_cesium.js";

const MOD_PATH = "odin_nfdrs::service::NfdrsService";

// the NFDRS variables we show in the data panel tabs (tab label, odin_nfdrs::grid::NfdrsField name, list element id)
const FIELD_TABS = [
    { label: "1h",   field: "dfm_1h",    eid: "nfdrs.dfm1hView" },
    { label: "10h",  field: "dfm_10h",   eid: "nfdrs.dfm10hView" },
    { label: "100h", field: "dfm_100h",  eid: "nfdrs.dfm100hView" },
    { label: "lh",   field: "lfm_herb",  eid: "nfdrs.lfmHerbView" },
    { label: "lw", field: "lfm_woody", eid: "nfdrs.lfmWoodyView" },
    { label: "kbdi", field: "kbdi",      eid: "nfdrs.kbdiView" },
    { label: "ic",   field: "ic",        eid: "nfdrs.icView" },
    { label: "sc",   field: "sc", eid: "nfdrs.scView" },
    { label: "erc",  field: "erc",       eid: "nfdrs.ercView" },
    { label: "bi",   field: "bi",        eid: "nfdrs.biView" }
];

/* #region RunEntry ************************************************************************************************/

// the client side representation of a odin_nfdrs::grid::NfdrsRun (request + status + output) and its (PNG) products
class RunEntry {

    constructor (run) {
        this.run = run;
        this.products = this.createProducts(); // field -> [ hourly product items ]
    }

    get id() { return this.run.id; }

    isDone() { return this.run.status === "done" && this.run.output; }

    state() {
        switch (this.run.status) {
            case "pending": return "⧗";
            case "done": return this.run.update ? "✓↻" : "✓";
            case "failed": return "✗";
            default: return "?";
        }
    }

    // hourly stats of a field (or null if we don't have any)
    fieldStats (field) {
        if (this.isDone()) {
            let fs = this.run.output.stats.find( s => s.field === field);
            if (fs) return fs.hourly;
        }
        return null;
    }

    // min (or max) value of a field over the whole forecast duration
    fieldMin (field) {
        let hs = this.fieldStats(field);
        return hs ? minOf( hs.filter(s=>s).map( s => s.min)) : undefined;
    }

    fieldMax (field) {
        let hs = this.fieldStats(field);
        return hs ? maxOf( hs.filter(s=>s).map( s => s.max)) : undefined;
    }

    createProducts() {
        let products = new Map();
        let run = this.run;

        if (this.isDone()) {
            for (let fs of run.output.stats) {
                let items = fs.hourly.map( (stats, hour) => {
                    return {
                        entry: this,
                        field: fs.field,
                        hour,
                        date: run.date + hour * 3600000,
                        stats,
                        url: `nfdrs-data/${run.id}/${fs.field}_${hour}.png`,
                        show: false,
                        layer: null
                    };
                });
                products.set( fs.field, items);
            }
        }
        return products;
    }

    allProducts() {
        return [...this.products.values()].flat();
    }

    showingProducts() {
        return this.allProducts().filter( it => it.show);
    }

    // transfer the display state from a previous entry for the same run (e.g. on update)
    takeOverDisplayState (oldEntry) {
        for (let oldItem of oldEntry.showingProducts()) {
            let items = this.products.get( oldItem.field);
            let item = items ? items[oldItem.hour] : undefined;
            if (item) {
                item.show = true;
                item.layer = oldItem.layer;
            } else {
                removeProductLayer(oldItem);
            }
        }
    }

    cleanUp() {
        for (let item of this.showingProducts()) {
            removeProductLayer(item);
        }
    }
}

function minOf (vs) { return (vs.length > 0) ? Math.min(...vs) : undefined; }
function maxOf (vs) { return (vs.length > 0) ? Math.max(...vs) : undefined; }

/* #endregion RunEntry */

var runEntries = new Map(); // id -> RunEntry
var selectedEntry = undefined;
var colorMaps = new Map();  // field -> { field, descr, unit, entries: [{color,code,descr}] }

var area = undefined;       // the selected area (main.GeoRect)
var areaAsset = undefined;

var sharedAreasView = undefined;
var runView = undefined;
var shownItem = undefined;  // the (single) product item that is currently displayed
var selectedHour = 0;       // forecast hour index we keep when switching tabs or runs
var fieldViews = new Map(); // field -> ui.List of hourly products
var colorMapView = undefined;
var colorMapInfoText = undefined;
var dateEntry = undefined;

var showLayers = config.layer.show;

createIcon();
createWindow();

initRunView();
initFieldViews();
initColorMapView();
initRequestChoices();

ws.addWsHandler( MOD_PATH, handleWsMessages);
main.addShareHandler( handleShareMessage);
odinCesium.initLayerPanel("nfdrs", config, showNfdrs);
console.log("odin_nfdrs initialized");

//--- end init

/* #region view initialization *************************************************************************************/

function createIcon() {
    return ui.Icon("./asset/odin_nfdrs/nfdrs-icon.svg", (e)=> ui.toggleWindow(e,'nfdrs'), "NFDRS");
}

function createWindow() {
    return ui.Window("NFDRS", "nfdrs", "./asset/odin_nfdrs/nfdrs-icon.svg")(
        ui.LayerPanel("nfdrs", toggleShowNfdrs),
        ui.Panel("input", false)(
            (sharedAreasView = ui.TreeList("nfdrs.areas", 8, "32rem", setAreaFromSelection)),
            ui.RowContainer()(
                ui.TextInput("area", "nfdrs.bounds", "20rem", { placeHolder: "enter lat,lon bounds (WSEN order)", changeAction: setAreaFromInput, isFixed: true }),
                ui.Button("pick", pickArea),
                ui.Button("clear", clearArea),
                ui.Button("zoom", zoomToArea)
            ),
            ui.RowContainer()(
                ui.TextInput("name", "nfdrs.name", "15rem", { placeHolder: "name of NFDRS run", isFixed: true }),
                ui.CheckBox("show", showInputBounds, "nfdrs.showArea"),
                ui.HorizontalSpacer(2.3),
                ui.Button("compute", requestRun)
            ),
            ui.RowContainer()(
                (dateEntry = ui.DTGEntry("date", "nfdrs.date")),
                ui.Choice("hours", "nfdrs.hours", null, "3rem"),
                ui.CheckBox("update", null, "nfdrs.update"),
                ui.HorizontalSpacer(1),
            )
        ),
        ui.Panel("data", true)(
            (runView = ui.ListWithPopup("nfdrs.runs", 6, selectRun, null, null, zoomToRun)(
                ui.MenuItem("remove", removeSelectedRun)
            )),
            ui.RowContainer("start")(
                ui.ColumnContainer()(
                    ui.Button("hide all", hideAllProducts),
                    ui.CheckBox("show", toggleShowImg, "nfdrs.showImg")
                ),
                ui.HorizontalSpacer(1.0),
                ui.TabbedContainer("nfdrs.fieldTabs")(
                    ...FIELD_TABS.map( (ft,i) => ui.Tab( ft.label, i == 0, null, selectFieldTab)(
                        ui.List( ft.eid, 8, selectFieldProduct)
                    ))
                )
            )
        ),
        ui.Panel("color map", false)(
            ui.RowContainer("start")(
                (colorMapInfoText = ui.VarText(null, "nfdrs.cm.info", "5rem", "6rem")),
                (colorMapView = ui.List("nfdrs.cm.list", 9))
            )
        )
    );
}

function showInputBounds(event) {
    updateAreaAsset();
}

function initRunView() {
    if (runView) {
        let fmt = (v) => (v === undefined) ? "" : util.f_1.format(v);
        let fmtInt = (v) => (v === undefined) ? "" : util.f_0.format(v);

        ui.setListItemDisplayColumns( runView, ["fit", "header"], [
            { name: "", tip: "run status (⧗ pending, ✓ done, ↻ update, ✗ failed)", width: "1.5rem", attrs: [], map: e => e.state() },
            { name: "name", tip: "name of NFDRS run", width: "7rem", attrs: [], map: e => util.maxString(e.run.name, 16) },
            { name: "base date", tip: "local base date of forecast", width: "7rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMString(e.run.date) },
            { name: "hrs", tip: "number of forecast hours", width: "2.5rem", attrs: ["fixed", "alignRight"], map: e => e.run.nHours },
            ui.listItemSpacerColumn(0.5),
            { name: "1h", tip: "min 1h dead fuel moisture [%] over forecast duration", width: "3.5rem", attrs: ["fixed", "alignRight"], map: e => fmt(e.fieldMin("dfm_1h")) },
            { name: "10h", tip: "min 10h dead fuel moisture [%] over forecast duration", width: "3.5rem", attrs: ["fixed", "alignRight"], map: e => fmt(e.fieldMin("dfm_10h")) },
            { name: "kbdi", tip: "max Keetch-Byram drought index over forecast duration", width: "3rem", attrs: ["fixed", "alignRight"], map: e => fmtInt(e.fieldMax("kbdi")) },
            { name: "ic", tip: "max ignition component over forecast duration", width: "3.5rem", attrs: ["fixed", "alignRight"], map: e => fmt(e.fieldMax("ic")) }
        ]);
    }
}

function initFieldViews() {
    let fmt = (v) => util.f_1.format(v);

    for (let ft of FIELD_TABS) {
        let view = ui.getList( ft.eid);
        if (view) {
            ui.setListItemDisplayColumns( view, ["fit", "header"], [
                { name: "date", tip: "local date of forecast hour", width: "8rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMString(e.date) },
                ui.listItemSpacerColumn(),
                { name: "min", tip: "min value in area", width: "4rem", attrs: ["fixed", "alignRight"], map: e => e.stats ? fmt(e.stats.min) : "-" },
                { name: "max", tip: "max value in area", width: "4rem", attrs: ["fixed", "alignRight"], map: e => e.stats ? fmt(e.stats.max) : "-" },
                { name: "avg", tip: "average value in area", width: "4rem", attrs: ["fixed", "alignRight"], map: e => e.stats ? fmt(e.stats.avg) : "-" }
            ]);
            fieldViews.set( ft.field, view);
        }
    }
}

function initColorMapView() {
    if (colorMapView) {
        ui.setListItemDisplayColumns( colorMapView, ["fit", "header"], [
            { name: "color", width: "2rem", attrs: [], map: e => ui.createColorBox(e.color) },
            { name: "range", width: "8rem", attrs: ["alignRight"], map: e => e.code },
            ui.listItemSpacerColumn(),
            { name: "description", width: "12rem", attrs: [], map: e => e.descr }
        ]);
    }
}

function initRequestChoices() {
    ui.setChoiceItems( "nfdrs.hours", config.hours, config.defaultHours);
    ui.setDTGEntryEpochMillis( dateEntry, Date.now());
}

/* #endregion view initialization */

/* #region websocket message handling *******************************************************************************/

function handleWsMessages(msgType, msg) {
    switch (msgType) {
        case "colorMaps":
            handleColorMapsMessage(msg);
            return true;
        case "snapshot":
            handleSnapshotMessage(msg);
            return true;
        case "update":
            handleUpdateMessage(msg);
            return true;
        case "remove":
            handleRemoveMessage(msg);
            return true;
        case "error":
            handleErrorMessage(msg);
            return true;
        default:
            return false;
    }
}

function handleColorMapsMessage (cms) {
    colorMaps.clear();
    for (let cm of cms) {
        colorMaps.set( cm.field, cm);
    }
}

function handleSnapshotMessage (runs) {
    runEntries.forEach( e => e.cleanUp());
    runEntries.clear();
    shownItem = undefined;
    selectedEntry = undefined;

    for (let run of runs) {
        runEntries.set( run.id, new RunEntry(run));
    }
    updateRunView();
}

function handleUpdateMessage (run) {
    let entry = new RunEntry(run);
    let oldEntry = runEntries.get(run.id);
    if (oldEntry) {
        entry.takeOverDisplayState(oldEntry);
        if (shownItem && shownItem.entry === oldEntry) shownItem = entry.showingProducts()[0]; // layer was transferred
    }

    runEntries.set( run.id, entry);
    if (selectedEntry && selectedEntry.id === run.id) selectedEntry = entry;
    updateRunView();

    if (run.status === "failed") {
        alert(`NFDRS run ${run.name} failed: ${run.error}`);
    }
}

function handleRemoveMessage (msg) {
    let entry = runEntries.get(msg.id);
    if (entry) {
        entry.cleanUp();
        runEntries.delete(msg.id);
        if (selectedEntry === entry) selectedEntry = undefined;
        if (shownItem && shownItem.entry === entry) shownItem = undefined;
        updateRunView();
        odinCesium.requestRender();
    }
}

function handleErrorMessage (msg) {
    alert(msg.msg);
}

/* #endregion websocket message handling */

/* #region data view ************************************************************************************************/

function updateRunView() {
    let entries = [...runEntries.values()].sort( (a,b) => b.run.date - a.run.date); // most recent first
    ui.setListItems( runView, entries);

    if (selectedEntry) {
        ui.setSelectedListItem( runView, selectedEntry); // this triggers selectRun
    } else {
        updateFieldViews();
    }
}

function selectRun (event) {
    let prevId = selectedEntry ? selectedEntry.id : undefined;
    selectedEntry = ui.getSelectedListItem(runView);
    let curId = selectedEntry ? selectedEntry.id : undefined;
    if (prevId !== curId) setAreaFromRun(selectedEntry); // don't overwrite input edits on run updates

    updateFieldViews( prevId !== curId); // keep the color map if this is just an update of the selected run
}

function updateFieldViews (clearColorMap = true) {
    for (let [field, view] of fieldViews) {
        let items = selectedEntry ? selectedEntry.products.get(field) : null;
        if (items) {
            ui.setListItems( view, items);
            ui.setSelectedListItemIndex( view, Math.min( selectedHour, items.length-1)); // keep the hour in all tabs
        } else {
            ui.clearList( view);
        }
    }

    if (clearColorMap) {
        if (selectedEntry && selectedEntry.isDone()) {
            let ft = showingFieldTab();
            if (ft) setColorMap( ft.field);
        } else {
            ui.clearList( colorMapView);
            ui.setVarText( colorMapInfoText, null);
        }
    }

    syncShownProduct();
}

// the FIELD_TABS entry of the currently showing tab
function showingFieldTab() {
    let label = ui.getShowingTabName("nfdrs.fieldTabs");
    return FIELD_TABS.find( ft => ft.label === label);
}

// the hourly product list of the currently showing tab
function showingFieldView() {
    let ft = showingFieldTab();
    return ft ? fieldViews.get( ft.field) : undefined;
}

// tab selection callback - keep color map and displayed image in sync with the showing field tab
function selectFieldTab (event, tabContainer) {
    let ft = FIELD_TABS.find( ft => ft.label === tabContainer.dataset.label);
    if (ft && selectedEntry && selectedEntry.isDone()) setColorMap( ft.field);
    syncShownProduct();
}

function zoomToRun (event) {
    let e = ui.getSelectedListItem(runView);
    if (e) zoomToBounds( e.run.bbox);
}

// hour selection in one of the field tab lists
function selectFieldProduct (event) {
    let view = event.target;
    if (view === showingFieldView()) { // ignore programmatic selections in hidden tabs
        let item = ui.getSelectedListItem(view);
        if (item) {
            selectedHour = item.hour;
            setColorMap( item.field);
            syncShownProduct();
        }
    }
}

function setColorMap (field) {
    let cm = colorMaps.get(field);
    if (cm) {
        ui.setListItems( colorMapView, cm.entries);
        ui.setVarText( colorMapInfoText, cm.descr);
    } else {
        ui.clearList( colorMapView);
        ui.setVarText( colorMapInfoText, null);
    }
}

function toggleShowImg (event) {
    syncShownProduct();
}

// the product item that should be displayed (selected hour in showing tab of selected run), or undefined
function selectedProduct() {
    let view = showingFieldView();
    return view ? ui.getSelectedListItem(view) : undefined;
}

// make sure we display exactly the selected product if "show" is checked, or nothing otherwise
function syncShownProduct() {
    let target = ui.isCheckBoxSelected("nfdrs.showImg") ? selectedProduct() : undefined;

    if (target !== shownItem) {
        if (shownItem) {
            shownItem.show = false;
            removeProductLayer(shownItem);
        }
        if (target) {
            target.show = true;
            addProductLayer(target);
            setColorMap( target.field);
        }
        shownItem = target;
        odinCesium.requestRender();
    }
}

function hideAllProducts (event) {
    ui.setCheckBox("nfdrs.showImg", false);
    syncShownProduct();

    runEntries.forEach( entry => { // there should not be any others but be paranoid
        for (let item of entry.showingProducts()) {
            item.show = false;
            removeProductLayer(item);
        }
    });
    odinCesium.requestRender();
}

function removeSelectedRun (event) {
    let entry = ui.getSelectedListItem(runView);
    if (entry) {
        if (entry.run.status === "pending") {
            alert("cannot remove pending NFDRS run");
        } else if (confirm(`remove NFDRS run ${entry.run.name} ?`)) {
            ws.sendWsMessage( MOD_PATH, "remove", { id: entry.id });
        }
    } else {
        alert("no NFDRS run selected");
    }
}

// we use imagery layers (instead of rectangle entities) since we want to see the grid cells (no texture interpolation)
async function addProductLayer (item) {
    let b = item.entry.run.bbox;
    try {
        let provider = await Cesium.SingleTileImageryProvider.fromUrl( item.url, {
            rectangle: Cesium.Rectangle.fromDegrees( b.west, b.south, b.east, b.north)
        });

        if (item.show && !item.layer) { // user might have toggled while we were loading
            let layer = new Cesium.ImageryLayer( provider, {
                alpha: config.alpha,
                show: showLayers,
                magnificationFilter: Cesium.TextureMagnificationFilter.NEAREST,
                minificationFilter: Cesium.TextureMinificationFilter.NEAREST
            });
            odinCesium.viewer.imageryLayers.add(layer);
            item.layer = layer;
            odinCesium.requestRender();
        }
    } catch (e) {
        console.error("failed to load NFDRS product", item.url, e);
    }
}

function removeProductLayer (item) {
    if (item.layer) {
        odinCesium.viewer.imageryLayers.remove( item.layer, true);
        item.layer = null;
    }
}

function allProductLayers() {
    return [...runEntries.values()].flatMap( e => e.showingProducts()).filter( it => it.layer).map( it => it.layer);
}

/* #endregion data view */

/* #region area selection and requests ******************************************************************************/

function requestRun (event) {
    if (!area) {
        alert("no area selected");
        return;
    }

    let nHours = ui.getSelectedChoiceValue("nfdrs.hours");
    if (!nHours) {
        alert("please select number of forecast hours");
        return;
    }

    let date = ui.getDTGEntryEpochMillis(dateEntry);
    if (!date) {
        ui.setDTGEntryEpochMillis(dateEntry, Date.now());
        date = ui.getDTGEntryEpochMillis(dateEntry);
    }

    let request = {
        name: getRunName(),
        bbox: { west: area.west, south: area.south, east: area.east, north: area.north },
        date: date,     // server rounds to full hour
        nHours: nHours,
        update: ui.isCheckBoxSelected("nfdrs.update") // (future) recurring computation for new forecasts
    };
    ws.sendWsMessage( MOD_PATH, "request", request);
}

function getRunName() {
    let name = ui.getFieldValue("nfdrs.name");
    return (name && name.trim().length > 0) ? name.trim() : "area";
}

function clearArea() {
    ui.setField("nfdrs.bounds", null);
    area = undefined;
    clearAreaAsset();
}

function pickArea (event) { // mouse selection
    odinCesium.enterGeoRect( (rect) => {
        ui.setField("nfdrs.bounds", util.degreesToString([rect.west, rect.south, rect.east, rect.north], util.fmax_3));
        setArea(rect);
    });
}

function setAreaFromInput (event) { // text field input (WSEN)
    let input = event.target.value;
    if (input && input.length > 0) {
        let a = input.split(',');
        let west = parseFloat(a[0]);
        let south = parseFloat(a[1]);
        let east = parseFloat(a[2]);
        let north = parseFloat(a[3]);

        if (isNaN(west) || isNaN(south) || isNaN(east) || isNaN(north) || west >= east || south >= north) {
            alert("invalid input (need west,south,east,north as comma separated degrees)");
        } else {
            setArea( new main.GeoRect( west, south, east, north));
        }
    }
}

function setAreaFromRun(e) {
    if (e) {
        let rect = e.run.bbox;
        ui.setField("nfdrs.bounds", util.degreesToString([rect.west, rect.south, rect.east, rect.north], util.fmax_3));
        ui.setDTGEntryDate("nfdrs.date", e.run.date);
        ui.setField("nfdrs.name", e.run.name); // default name for this area (can be edited)

        setArea( new main.GeoRect( rect.west, rect.south, rect.east, rect.north));
    }
    // if no run is selected we keep the current input values
}

function setAreaFromSelection (event) {
    let sharedItem = ui.getSelectedListItem(sharedAreasView);
    if (sharedItem) {
        let rect = sharedItem.value.data;
        ui.setField("nfdrs.bounds", util.degreesToString([rect.west, rect.south, rect.east, rect.north], util.fmax_3));
        ui.setField("nfdrs.name", sharedItem.key); // default name for this area (can be edited)
        setArea(rect);
    } else {
        clearArea();
    }
}

// this is called whenever the area input changes (text input, pick, shared area or run selection)
function setArea (rect) {
    area = rect;
    updateAreaAsset();
}

// (re-)create or remove the area asset according to the current area and the "show" checkbox
function updateAreaAsset () {
    clearAreaAsset();

    if (area && ui.isCheckBoxSelected("nfdrs.showArea")) {
        areaAsset = new Cesium.Entity({
            polyline: {
                positions: odinCesium.cartesian3ArrayFromDegreesRect(area),
                clampToGround: true,
                width: config.areaWidth,
                material: config.areaColor
            },
            show: showLayers,
            selectable: false
        });
        odinCesium.addEntity(areaAsset);
        odinCesium.requestRender();
    }
}

function clearAreaAsset() {
    if (areaAsset) {
        odinCesium.removeEntity(areaAsset);
        areaAsset = undefined;
        odinCesium.requestRender();
    }
}

function zoomToArea (event) {
    if (area) zoomToBounds(area);
}

function zoomToBounds (b) {
    let rect = Cesium.Rectangle.fromDegrees( b.west, b.south, b.east, b.north);
    let cameraPos = odinCesium.viewer.camera.getRectangleCameraCoordinates(rect);
    odinCesium.zoomTo(cameraPos);
}

/* #endregion area selection and requests */

/* #region layer visibility *****************************************************************************************/

function toggleShowNfdrs (event) {
    // the module panel checkbox is handled through odinCesium.initLayerPanel / showNfdrs
}

function showNfdrs (cond) {
    showLayers = cond;
    for (let layer of allProductLayers()) {
        layer.show = cond;
    }
    if (areaAsset) areaAsset.show = cond;
    odinCesium.requestRender();
}

/* #endregion layer visibility */

/* #region shared items *********************************************************************************************/

function handleShareMessage (msg) {
    if (msg.SHARE_INITIALIZED) { // we get that no matter what the share implementation is
        updateSharedAreas();

    } else if (main.isShareInitialized()) {
        if (msg.setShared) {
            let sharedItem = msg.setShared;
            if (sharedItem.key.match(BBOX_PATTERN)) {
                updateSharedAreas();
            }
        }
    }
}

function updateSharedAreas() {
    let areas = getSharedAreas();
    let tree = data.ExpandableTreeNode.from( areas, e=>e.key );
    ui.setTree( sharedAreasView, tree);
}

const BBOX_PATTERN = util.glob2regexp("{**/rect/**,**/rect}"); // any pathname with 'rect' in it

function getSharedAreas() {
    let areas = [];
    let items = main.getAllMatchingSharedItems( BBOX_PATTERN);
    for (let item of items) {
        if (item.value.type == "GeoRect") {
            areas.push( item);
        }
    }
    return areas;
}

/* #endregion shared items */
