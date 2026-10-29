/*
 * Copyright (c) 2026, United States Government, as represented by the
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

import { config } from "./odin_elmfire_config.js";

import * as util from "../odin_server/ui_util.js";
import * as ui from "../odin_server/ui.js";
import * as code from "../odin_server/ui_code_input.js";
import * as ws from "../odin_server/ws.js";
import * as main from "../odin_server/main.js";
import * as odinCesium from "../odin_cesium/odin_cesium.js";
import * as wf from "../odin_wind/windfield.js";

const MOD_PATH = "odin_elmfire::service::ElmfireService";
const DEBOUNCE_SECS = 3;

// standard user entered barrier types we know about
const BARRIER_SEAT_DROP = 'SEAT-drop';
const BARRIER_LAT_DROP = 'LAT-drop';
const BARRIER_VLAT_DROP = 'VLAT-drop';
const BARRIER_DOZER_LINE = 'dozer-line';
const BARRIER_HAND_LINE = 'hand-line';
const BARRIER_FILE = 'file';

// ignition types (see odin_elmfire::Ignition)
const IGNITION_POINT = 'point';
const IGNITION_LINE = 'line';

/* #region ElmfireEntry ************************************************************************************/

class FireEntry {

    constructor (ef) {
        this.id = ef.id;
        this.ef = ef;

        //--- display assets
        let assets = {};
        Object.keys(config.render).forEach(key => assets[key] = null);
        this.assets = assets;

        //--- isochrone (time-of-arrival) display state. Entity visibility is only set in updateDisplay()
        this.isShown = false;              // == "show" checkbox in fireView
        this.shownTimeSteps = new Set();   // explicitly requested levels (secs) in mode B
        this.toaDs = undefined;            // the spread isochrone polylines/polygons (loaded on demand)
        this.toaLoading = undefined;       // Promise while loading toaDs (to avoid duplicate loads)
        this.toaGen = 0;                   // incremented whenever toaDs is reset so that we can discard outdated loads
        this.toaRender = undefined;        // toa render options of this run (set when the run gets selected)

        //--- wind field display state
        this.windFields = [];              // timestep index -> { animation, vector, contour } WindFieldVisualizations (created on demand)
    }

    // the toa render options for this run. Changes of display parameters only affect the selected run and
    // following selections (to make it easier to compare runs), i.e. each run keeps its own copy
    getToaRender() {
        return this.toaRender ? this.toaRender : render.toa;
    }

    // adopt the current display parameters (called when this run gets selected or display parameters are changed)
    setToaRender() {
        this.toaRender = { ...render.toa };
        this.updateToaRendering();
    }

    state() {
        return (this.ef.output) ? "✓" : "⧗";
    }

    rasterProducts() {
        let output = this.ef.output;
        return output ? output.rasterProducts : null;
    }

    // levels (secs since base date) of all timesteps of this run (as shown in timestepView)
    allLevels() {
        let timesteps = this.ef.output ? this.ef.output.timesteps : null;
        return timesteps ? timesteps.map( ts => getTimestepSeconds(this, ts)) : [];
    }

    // this is the only place where we set visibility of toa entities - it is computed from the current state:
    //  - outlines (polylines) are shown if the run is shown and we are in showSelectedEntryMode (only the selected
    //    run can be shown in this mode) or if the respective level was explicitly requested
    //  - only the selected timestep of the selectedEntry is filled, and only if its outline is visible
    updateDisplay() {
        let ds = this.toaDs;
        if (ds) {
            ds.show = layerShow;
            let isSel = (this === selectedEntry);
            for (const e of ds.entities.values) {
                let lvl = getPropValue(e.properties, "level");
                let showOutline = this.isShown && (showSelectedEntryMode || this.shownTimeSteps.has(lvl));
                if (e.polyline) e.polyline.show = showOutline;
                if (e.label) e.label.show = showOutline; // only label visible outlines
                if (e.polygon) e.polygon.fill = showOutline && isSel && (selectedLevel !== undefined) && (lvl == selectedLevel);
            }
            odinCesium.requestRender();
        }
    }

    // remove loaded toa data (outdated loads that are still in progress will be discarded)
    removeToa() {
        this.toaGen++;
        if (this.toaDs) odinCesium.removeDataSource(this.toaDs);
        this.toaDs = undefined;
        this.toaLoading = undefined;
    }

    resetDisplayState() {
        this.isShown = false;
        this.shownTimeSteps = new Set();
    }

    showImgOutputAsset(key) {
        if (!this.assets[key]) {
            this.removeImgOutputAssets();

            if (this.ef.output && this.ef.output.rasterProducts) {
                let fname = `${key}.png`;
                let prod = this.ef.output.rasterProducts.find((p) => p.file == fname);
                let prodRender = render[key];
                if (prod && prodRender) {
                    let entity = createRasterEntity(`elmfire-data/${this.ef.id}/outputs/${prod.file}`, prod.bbox, prodRender);
                    this.assets[key] = entity;
                    imgDataSource.entities.add(entity);
                }
            }
        }
    }

    removeImgOutputAssets() {
        let assets = this.assets;
        Object.keys(assets).forEach((key) => {
            imgDataSource.entities.remove(assets[key]);
            assets[key] = null;
        });
    }

    // get (or create) the WindFieldVisualization of the given display type for timestep index `idx`.
    // Returns undefined if this run has no wind field output for this timestep
    getWindField(idx, displayType) {
        let wfs = this.ef.output ? this.ef.output.windfields : undefined;
        if (!wfs || idx < 0 || idx >= wfs.length) return undefined;

        let fields = this.windFields[idx];
        if (!fields) {
            let files = wfs[idx];
            let url = (fname) => `elmfire-data/${this.ef.id}/outputs/${fname}`; // see ElmfireService data route
            fields = {};
            fields[wf.AnimDisplay] = new wf.AnimField( url(files.grid), windRender.animRender);
            fields[wf.VectorDisplay] = new wf.VectorField( url(files.vectors), windRender.vectorRender);
            fields[wf.ContourDisplay] = new wf.ContourField( url(files.contours), windRender.contourRender);
            this.windFields[idx] = fields;
        }
        return fields[displayType];
    }

    hasWindFields() {
        let wfs = this.ef.output ? this.ef.output.windfields : undefined;
        return (wfs && wfs.length > 0);
    }

    // release all wind field resources (they get re-loaded on demand)
    clearWindFields() {
        this.windFields.forEach( fields => Object.values(fields).forEach( f => f.clear()));
        this.windFields = [];
    }

    cleanUp() {
        this.removeImgOutputAssets();
        this.removeToa();
        this.clearWindFields();
    }

    getToaBounds() {
        // check if we have a "toa.png" raster product, and if so return its bbox
        if (this.ef.output && this.ef.output.rasterProducts) {
            let toaProd = this.ef.output.rasterProducts.find((p) => p.file == "toa.png");
            if (toaProd) {
                return toaProd.bbox;
            }
        }

        // as a fallback use the whole run bbox (which is likely too large)
        return this.ef.bbox;
    }

    // this does not affect visibility but only how entities are rendered if they are visible
    // visibility changes on areaEntry selection and explicit request via checkbox
    updateToaRendering() {
        let ds = this.toaDs;
        if (ds) {
            let renderOpts = this.getToaRender();
            let fillMaterial = new Cesium.ColorMaterialProperty(renderOpts.fill.withAlpha(renderOpts.alpha));

            ds.entities.values.forEach( entity => {
                if (entity.polygon) {
                    entity.polygon.material = fillMaterial;
                }
                if (entity.polyline) {
                    entity.polyline.width = renderOpts.strokeWidth;
                    entity.polyline.material = renderOpts.stroke;
                }
            });
            odinCesium.requestRender();
        }
    }

    // note - this should be kept in sync with odin_elmfire::ElmFire::get_id
    static getId(name, date, position) {
        return `${name}__${util.toYYYYMMDDhhmmZString(date,true,true)}__${main.GeoPoint.encode(position)}`;
    }
}

// note that this has to be kept compatible with odin_elmfire::Barrier
class Barrier {
    constructor(barrierType,name,value,width) {
        this.barrierType = barrierType;
        this.name = name;
        this.value = value; // main.GeoLine or main.GeoLineString
        this.width = width; // [ft]

        this.asset = undefined; // Cesium entity (only exists while this is one of the barrierEntries)
    }

    // only serialize the odin_elmfire::Barrier fields (not the Cesium asset)
    toJSON() {
        return { barrierType: this.barrierType, name: this.name, value: this.value, width: this.width };
    }

    positions() {
        let v = this.value;
        let pts = v.points ? v.points : (v.start && v.end) ? [v.start, v.end] : [];
        return pts.map( p => Cesium.Cartesian3.fromDegrees( p.lon, p.lat));
    }

    createAsset() {
        this.removeAsset();

        let positions = this.positions();
        if (positions.length >= 2) {
            let r = config.barrierRender;
            let clr = r.colors[this.barrierType] || r.defaultColor;

            this.asset = new Cesium.Entity({
                position: positions[0],
                polyline: {
                    positions,
                    clampToGround: true,
                    width: r.lineWidth,
                    material: clr
                },
                label: {
                    text: this.name,
                    font: config.labelFont,
                    fillColor: clr,
                    showBackground: true,
                    backgroundColor: config.labelBackground,
                    horizontalOrigin: Cesium.HorizontalOrigin.LEFT,
                    verticalOrigin: Cesium.VerticalOrigin.BOTTOM,
                    pixelOffset: config.labelOffset,
                    distanceDisplayCondition: config.labelDC,
                    heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
                    disableDepthTestDistance: Number.POSITIVE_INFINITY
                }
            });
            dataSource.entities.add( this.asset);
            odinCesium.requestRender();
        }
    }

    removeAsset() {
        if (this.asset) {
            dataSource.entities.remove( this.asset);
            this.asset = undefined;
            odinCesium.requestRender();
        }
    }
}

// note that this has to be kept compatible with odin_elmfire::Ignition
class Ignition {
    constructor(id,ignitionType,date,value) {
        this.id = id;
        this.ignitionType = ignitionType; // IGNITION_POINT or IGNITION_LINE
        this.date = date;   // epoch millis
        this.value = value; // main.GeoPoint or main.GeoLineString

        this.asset = undefined; // Cesium entity (only exists while this is one of the ignitionEntries)
    }

    // only serialize the odin_elmfire::Ignition fields (not the Cesium asset)
    toJSON() {
        return { id: this.id, ignitionType: this.ignitionType, date: this.date, value: this.value };
    }

    isPoint() { return this.ignitionType === IGNITION_POINT; }

    nPoints() { return this.isPoint() ? 1 : this.value.points.length; }

    // point ignitions at the simulation center are not rendered since this is already shown by the center symbol
    isAtCenter() {
        let loc = ui.getLocation(locEntry);
        return loc && this.isPoint() && isSameLocation(loc, this.value);
    }

    createAsset() {
        this.removeAsset();
        if (this.isAtCenter()) return;

        if (this.isPoint()) {
            let p = this.value;
            this.asset = new Cesium.Entity({
                position: Cesium.Cartesian3.fromDegrees(p.lon, p.lat),
                point: {
                    pixelSize: config.ignPointSize,
                    color: config.ignPointColor,
                    outlineColor: config.ignPointOutlineColor,
                    outlineWidth: config.ignPointOutlineWidth,
                    heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
                    distanceDisplayCondition: config.ignPointDC,
                },
                label: this.createLabel()
            });

        } else {
            let positions = this.value.points.map( p => Cesium.Cartesian3.fromDegrees(p.lon, p.lat));
            if (positions.length < 2) return;

            this.asset = new Cesium.Entity({
                position: positions[0],
                polyline: {
                    positions,
                    clampToGround: true,
                    width: config.ignLineWidth,
                    material: config.ignLineColor
                },
                label: this.createLabel()
            });
        }

        dataSource.entities.add( this.asset);
        odinCesium.requestRender();
    }

    createLabel() {
        return {
            text: this.id.toString(),
            horizontalOrigin: Cesium.HorizontalOrigin.LEFT,
            verticalOrigin: Cesium.VerticalOrigin.TOP,
            font: config.labelFont,
            fillColor: config.labelColor,
            showBackground: true,
            backgroundColor: config.labelBackground,
            backgroundPadding: new Cesium.Cartesian2( 3,3),
            pixelOffset: config.labelOffset,
            distanceDisplayCondition: config.ignPointDC,
            heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
            disableDepthTestDistance: Number.MAX_SAFE_INTEGER,
        };
    }

    removeAsset() {
        if (this.asset) {
            dataSource.entities.remove( this.asset);
            this.asset = undefined;
            odinCesium.requestRender();
        }
    }
}

class FixedEntry {
    static choiceWidth = "3.4rem";

    constructor(type, uom, choiceItems, selIdx, minVal, maxVal, maxLen, values = []) {
        this.type = type;
        this.uom = uom;
        this.ch = ui.createChoice(null, FixedEntry.choiceWidth, choiceItems, selIdx, {numeric: true, pad: -3});
        this.minVal = minVal;
        this.maxVal = maxVal;
        this.maxLen = maxLen;
        this.values = values;
    }

    lastValue() {
        return (this.values.length == 0) ? null : this.values[this.values.length - 1];
    }

    pushValue() {
        let selChoice = ui.getSelectedChoiceItem(this.ch);
        let x = selChoice;
        if (this.values.length < this.maxLen) this.values.push(x);
    }

    popValue() {
        this.values.pop();
    }

    getHour(n) {
        let len = this.values.length;
        if (len == 0 || n >= len) return '';
        return this.values[n];
    }

    setHour(idx, v) {
        if (idx < this.maxLen) {
            let values = this.values;

            if (idx < values.length) {
                values[idx] = v;
            } else {
                let selChoice = ui.getSelectedChoiceItem(this.ch);
                for (let i = values.length; i < idx - 1; i++) {
                    values.push(selChoice);
                }
                values.push(v);
            }
        }
    }

    clear() {
        this.values = [];
    }
}

/* #endregion ElmfireEntry */

var render = util.levelOneCloneOf( config.render);

var fireView = undefined;
var timestepView = undefined;
var selCb = undefined;
var imgCh = undefined;
var wndCh = undefined;
var nameEntry = undefined;
var locEntry = undefined;
var pickNowBtn = undefined;
var dtgEntry = undefined;
var ignitionView = undefined;
var ignitionTypeCh = undefined;
var ignitionDelayEntry = undefined;
var addIgnBtn = undefined;
var fixedView = undefined;
var spottingSrc = undefined;
var barrierView = undefined;
var barrierBtn = undefined;
var barrierTypeCh = undefined;
var barrierWidthCh = undefined;
var barrierEntry = undefined;

var statsView = undefined;

var fireEntries = new Map();  // id -> FireEntry
var barrierEntries = [];
var ignitionEntries = []; // current ignitions (from selectedEntry or user input)
var fixedEntries = [];

// UI state
var showSelectedEntryMode = true;  // do we automatically show all isochrones of a run upon selecting it (or only requested timesteps)
var selectedEntry = undefined;
var selectedLevel = undefined;     // level (secs since base date) of selected timestepView entry (the only filled polygon)
var layerShow = config.layer.show;
var repopulatingFireView = false;  // to ignore transient selection changes while re-populating fireView

// the wind field we show (only one at a time): { entry: FireEntry, idx: timestep index } or undefined.
// The display type is selected with the "wnd" choice (wndCh)
var shownWind = undefined;
var windRender = new wf.WindFieldRenderControls( "elmfire.wind", config.windRender, windRenderChanged);

var curCenterAsset = undefined;

var spottingFileList = []; // the list of known spotting config files we get from the server
var barrierFileList = []; // the list of known barrier (GeoJSON) files we get from the server

createIcon();
createWindow();

initFireView();
initTimestepView();
initIgnitionView();
initFixedView();
initBarrierView();

setWindChoices();
setImgChoices();
setIncidentChoices();
initFixedEntries();
setBarrierChoices();

setInputDebouncers();
initDisplayControls();
windRender.initialize();

// z-order of our rendered entities (bottom to top): images (imgDataSource), toa isochrones (one FireEntry.toaDs per run),
// barrier/ignition/fire entities (dataSource). Since data sources are rendered in order of addition we create imgDataSource
// first, and raise dataSource to the top whenever we add a toaDs (see loadToa())
var imgDataSource = odinCesium.createDataSource("elmfire-img", config.layer.show);
var dataSource = odinCesium.createDataSource("elmfire", config.layer.show);

ws.addWsHandler( MOD_PATH, handleWsMessages);
odinCesium.setEntitySelectionHandler( fireSelection);
odinCesium.initLayerPanel("elmfire", config, showFires);
console.log("elmfire initialized");

//--- websocket message handling

function handleWsMessages(msgType, msg) {
    switch (msgType) {
        case "snapshot":
            handleSnapshotMessage(msg);
            return true;
        case "update":
            handleUpdateMessage(msg);
            return true;
        case "remove":
            handleRemoveMessage(msg);
            return true;
        case "spotting":
            handleSpottingMessage(msg);
            return true;
        case "barriers":
            handleBarriersMessage(msg);
            return true;
        default:
            return false;
    }
}

function handleSnapshotMessage(snapshot) {
    updateElmfires(snapshot.elmfires);
}

function handleUpdateMessage(update) {
    updateElmfires( [update]);
}

function updateElmfires(elmFires) {
    let reloads = [];

    for (let ef of elmFires) {
        let e = new FireEntry(ef);

        let oldEntry = fireEntries.get(ef.id);
        if (oldEntry) { // carry over display state, toa data gets re-loaded if this run is shown
            e.isShown = oldEntry.isShown;
            e.shownTimeSteps = oldEntry.shownTimeSteps;
            e.toaRender = oldEntry.toaRender;
            oldEntry.cleanUp();
            if (e.isShown) reloads.push(e);
            if (shownWind && shownWind.entry === oldEntry) shownWind = { entry: e, idx: shownWind.idx };
        }

        fireEntries.set(e.id, e);
    }

    updateFireView(); // if the selected run was replaced this calls selectEntry() for the new entry
    reloads.forEach( e => ensureToa(e));
    updateWindDisplay(); // in case the shown wind field belongs to a replaced run
}

function handleRemoveMessage(msg) {
    let id = msg.id;

    let e = fireEntries.get(id);
    if (e) {
        if (shownWind && shownWind.entry === e) shownWind = undefined;
        e.cleanUp(); // removes all display assets of this run
        fireEntries.delete(id);

        if (selectedEntry === e) {
            cleanUpCurAssets();

            selectedEntry = null;
            selectedLevel = undefined;
            updateStatsView();
            updateAreaView();
            updateErrorView();
        }
        updateFireView();
    }
}

function cleanUpCurAssets() {
    dataSource.entities.remove(curCenterAsset);
    curCenterAsset = null;

    clearIgnitions();
    odinCesium.requestRender();
}

function handleSpottingMessage(newSpottingList) {
    spottingFileList = ["-", ...newSpottingList]; // prepend a none-choice
    updateSpottingsChoices();
}

function handleBarriersMessage(newBarrierList) {
    barrierFileList = newBarrierList;
    updateBarrierChoices();
}

//--- view initialization

function createIcon() {
    return ui.Icon("./asset/odin_elmfire/fires-icon.svg", (e)=> ui.toggleWindow(e,'elmfire'), "elmfire data");
}

function createWindow() {
    let view = ui.Window("elmfire", "elmfire", "./asset/odin_elmfire/fires-icon.svg")(
        ui.LayerPanel("elmfire", toggleShowFires),
        ui.Panel("simulations", true)(
            ui.RowContainer()(
                (selCb = ui.CheckBox("show sel", toggleShowSelectedEntry, "elmfire.showSel", true)),
                ui.HorizontalSpacer(12),
                (imgCh = ui.Choice("img", "elmfire.showImg", selectImgOutput, "4rem")),
                (wndCh = ui.Choice("wnd", "elmfire.showWind", selectWndOutput, "4rem")),
                ui.HorizontalSpacer(2),
                ui.Button("∅", clearAllAssets, "3rem")
            ),
            (fireView = ui.ListWithPopup("elmfire.fires", 5, selectEntry, null, null, zoomToSelection)(
                ui.MenuItem("remove", removeSelectedFire)
            )),
            ui.RowContainer("end")(
                (statsView = ui.KvTable("elmfire.stats", 8, "15.7rem", "15.7rem")),
                (timestepView = ui.List("elmfire.timesteps", 8, selectTimeStep))
            ),
            ui.VarText(null, "elmfire.error", "30rem")
        ),
        ui.Panel("input", false)(
            ui.RowContainer('start')(
                ui.ColumnContainer('start')(
                    (nameEntry = ui.TextInput("name", "elmfire.name", "14.9rem", { placeHolder: "event name", labelWidth: "4rem" })),
                    (dtgEntry = ui.DTGEntry("date", "elmfire.date", {labelWidth: "4rem"})),
                    (locEntry = ui.LocationEntry("loc", "elmfire.location", pickLocation, {labelWidth: "4rem"}))
                ),
                ui.HorizontalSpacer(0.3),
                ui.ColumnContainer('end')(
                    ui.CheckBox( "comp wind", null, "elmfire.computeWind"),
                    ui.Choice("hours", "elmfire.nHours", null, "3rem"),
                    ui.Choice("grid [km]", "elmfire.size", null, "3rem")
                ),
                ui.HorizontalSpacer(0.3),
                ui.ColumnContainer()(
                    (pickNowBtn = ui.Button("pick", pickNow, "4rem")),
                    ui.Button("run", createRequest, "4rem"),
                    ui.Button("clear", clearIncident, "4rem")
                )
            ),

            ui.VerticalSpacer(0.3),
            ui.TabbedContainer()(
                ui.Tab("ignitions", true)(
                    ui.RowContainer("end")(
                        (ignitionView = ui.List("elmfire.ignitions", 6, selectIgnition)),
                        ui.HorizontalSpacer(0.2),
                        ui.ColumnContainer("center")(
                            ui.RowContainer()(
                                ui.Button("del", deleteIgnition, "4rem"),
                                ui.Button("clear", clearIgnitions, "4rem")
                            )
                        )
                    ),
                    ui.RowContainer()(
                        (ignitionTypeCh = ui.Choice(null, "elmfire.ignitionType", null, "6.5rem")),
                        (ignitionDelayEntry = ui.HMEntry(null, "elmfire.ignitionDelay")),
                        ui.HorizontalSpacer(0.2),
                        ui.Button("edit", editIgnition, "4rem"),
                        (addIgnBtn = ui.Button("pick", addIgnition, "4rem"))
                    )
                ),
                ui.Tab("fixed wx", false)(
                    (fixedView = ui.List("elmfire.fixed", 8)),
                    ui.RowContainer()(
                        ui.Button("clear", clearFixed),
                        ui.HorizontalSpacer(1),
                        ui.Button("－", popFixedColumn, "2.2rem"),
                        ui.Button("＋", pushFixedColumn, "2.2rem")
                    )
                ),
                ui.Tab("spotting", false)(
                    (spottingSrc = code.CodeInput("elmfire.spottingSrc", "nml-highlight", "fortran-namelist", "35rem", "11lh")),
                    ui.RowContainer()(
                        ui.Button("clear", clearSpotting),
                        ui.HorizontalSpacer(1.0),
                        ui.Choice("select", "elmfire.savedSpottings", selectSpotting),
                        ui.TextInput("name", "elmfire.spottingName", "8rem"),
                        ui.Button("save", saveSpotting)
                    )
                ),
                ui.Tab("barriers", false)(
                    ui.RowContainer("end")(
                        (barrierView = ui.List("elmfire.barriers", 6, selectBarrier)),
                        ui.ColumnContainer("center")(
                            ui.RowContainer()(
                                ui.Button("del", deleteBarrier, "4rem"),
                                ui.Button("clear", clearBarriers, "4rem")
                            )
                        )
                    ),
                    ui.RowContainer()(
                        (barrierEntry = ui.TextInput("", "elmfire.barrierName", "9.9rem")),
                        (barrierTypeCh = ui.Choice(null, "elmfire.barrierChoice", selectBarrierChoice, "9.1rem")),
                        (barrierWidthCh = ui.Choice(null, "elmfire.barrierWidth", null, "3.1rem")),
                        ui.HorizontalSpacer(0.8),
                        ui.Button("edit", editBarrier, "4rem"),
                        (barrierBtn = ui.Button("pick", addBarrier, "4rem"))
                    )
                )
            )
        ),
        ui.Panel("toa display", false)(
            ui.RowContainer()(
                ui.ColorField("toa stroke", "fires.toa.stroke_color", true, "2rem", { changeAction: toaStrokeColorChanged }),
                ui.initSlider("width", "fires.toa.stroke_width", toaStrokeWidthChanged, 1, 3, 1, util.f_0, 1)
            ),
            ui.RowContainer()(
                ui.ColorField("toa fill", "fires.toa.fill_color", true, "2rem", { changeAction: toaFillColorChanged }),
                ui.initSlider("alpha", "fires.toa.fill_alpha", toaFillAlphaChanged, 0, 1, 0.1, util.f_1, 0.7)
            ),
            ui.Button("reset all", resetRendering)
        ),
        windRender.displayPanel("wind display")
    );

    spottingSrc.placeholder = "Elmfire spotting input (NML format)";

    return view;
}

//--- listview initializations

function removeSelectedFire(event) {
    if (selectedEntry) {
        if (window.confirm(`remove fire simulation '${selectedEntry.ef.name}'`)) {
            let id = selectedEntry.id;
            ws.sendWsMessage(MOD_PATH, "removeElmfire", { id });

            // global asset cleanup happens upon receiving server response msg
        }
    }
}

function imgChoices(e) {
    let keys = ["-"];
    let prods = e.rasterProducts();
    if (prods) {
        prods.forEach((rp) => {
            let fname = util.stripExtension(rp.file);
            keys.push(fname)
        });
    }

    return keys;
}

function initFireView() {
    ui.setListItemDisplayColumns(fireView, ["fit", "header"], [
        { name: "", tip: "processing state", width: "1.5rem", attrs: [], map: e => e.state() },
        { name: "name", tip: "fire name", width: "11rem", attrs: [], map: e => util.maxString(e.ef.name, 20) },
        { name: "date", tip: "local date of update", width: "10rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalDateHMTimeString(e.ef.date) },
        { name: "h", tip: "length of simulation in [h]", width: "2rem", attrs: ["fixed", "alignRight"], map: e => e.ef.nHours },
        { name: "area", tip: "area [ac]", width: "5rem", attrs: ["fixed", "alignRight"], map: e => totalArea(e) },
        { name: "in", tip: "input types (wind, fixed, spotting, barriers)", width: "3.5rem", attrs: ["fixed", "alignRight"], map: e => inputTypes(e) },
        ui.listItemSpacerColumn(0.8),
        { name: "⏿", tip: "show 30min time-of-arrival isochrones", width: "1.5rem", attrs: [], map: e => ui.createCheckBox(e.isShown, toggleShowToa) }
    ]);
}

function totalArea(e) {
    return (e.ef.output) ? util.formatFloat(e.ef.output.stats[0].totalArea, 1) : '?';
}

function initTimestepView() {
    ui.setListItemDisplayColumns(timestepView, ["fit", "header"], [
        { name: "dt", tip: "hours from start", width: "2.5rem", attrs: ["fixed", "alignRight"], map: e=> areaItemDt(e) },
        { name: "time", tip: "", width: "4rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalHMTimeString(e.date) },
        { name: "𝚫", tip: "growth in %", width: "3.5rem", attrs: ["fixed", "alignRight"], map: e => e.growth },
        { name: "area", tip: "area [ac]", width: "5rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.area, 1) },
        ui.listItemSpacerColumn(0.8),
        { name: "wnd", tip: "show wind field", width: "2rem", attrs:[], map: e => ui.createCheckBox(isShownWind(e), toggleShowWind) },
        { name: "⏿", tip: "show isochrone", width: "1.5rem", attrs: [], map: e => ui.createCheckBox(isRequestedTimeStep(e), toggleShowIso) }
    ]);
}

function initIgnitionView() {
    ui.setListItemDisplayColumns(ignitionView, ["fit", "header"], [
        { name: "id", tip: "ignition id (shown as label)", width: "2rem", attrs: ["fixed", "alignRight"], map: e => e.id },
        { name: "date", tip: "time of ignition", width: "11rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalDateHMTimeString(e.date) },
        ui.listItemSpacerColumn(1.4),
        { name: "type", tip: "ignition type (point or line)", width: "4rem", attrs: [], map: e => e.ignitionType },
        ui.listItemSpacerColumn(1.3),
        { name: "Δt", tip: "hours and minutes after first ignition", width: "3.5rem", attrs: ["fixed", "alignRight"], map: e => ignitionDelayString(e) },
        ui.listItemSpacerColumn(0.8),
    ]);
}

function ignitionDelayString(ign) {
    let t0 = ui.getDTGEntryEpochMillis(dtgEntry);
    let t1 = ign.date;
    let d = t1 - t0;
    let dh = Math.floor(d / 3600000);
    let dm = Math.round((d % 3600000) / 60000);

    let hm = `${dh < 10 ? '0' + dh : dh}:${dm < 10 ? '0' + dm : dm}`;
    return hm;
}

function initFixedView() {
    ui.setListItemDisplayColumns(fixedView, ["fit", "header"], [
        { name: "type", tip: "fixed input type", width: "3rem", attrs: [], map: e => e.type },
        { name: "1h", tip: "1st hour", width: "2.8rem", attrs: ['fixed', 'alignRight'], map: e => e.getHour(0), edit: (evt)=> editHour(0, evt)},
        { name: "2h", tip: "2nd hour", width: "2.8rem", attrs:['fixed','alignRight'], map: e => e.getHour(1), edit: (evt)=> editHour(1, evt)},
        { name: "3h", tip: "3rd hour", width: "2.8rem", attrs:['fixed','alignRight'], map: e => e.getHour(2), edit: (evt)=> editHour(2, evt)},
        { name: "4h", tip: "4th hour", width: "2.8rem", attrs:['fixed','alignRight'], map: e => e.getHour(3), edit: (evt)=> editHour(3, evt)},
        { name: "5h", tip: "5th hour", width: "2.8rem", attrs:['fixed','alignRight'], map: e => e.getHour(4), edit: (evt)=> editHour(4, evt)},
        { name: "6h", tip: "6th hour", width: "2.8rem", attrs:['fixed','alignRight'], map: e => e.getHour(5), edit: (evt)=> editHour(5, evt)},
        { name: "7h", tip: "7th hour", width: "2.8rem", attrs:['fixed','alignRight'], map: e => e.getHour(6), edit: (evt)=> editHour(6, evt)},
        { name: "8h", tip: "8th hour", width: "2.8rem", attrs: ['fixed', 'alignRight'], map: e => e.getHour(7), edit: (evt) => editHour(7, evt) },
        ui.listItemSpacerColumn(0.8),
        { name: "values", tip: "value choices", width: FixedEntry.choiceWidth, attrs: [], map: e => e.ch },
        ui.listItemSpacerColumn(0.1),
        { name: '', tip: null, width:"2rem", attrs: ["small"], map: e => e.uom },
        { name: '', tip: null, width: "1.5rem", attrs: ['alignCenter'], map: e => ui.createInlineButton("-", () => popFixed(e)) },
        { name: '', tip: null, width: "1.5rem", attrs: ['alignCenter'], map: e => ui.createInlineButton("+", () => pushFixed(e)) }
    ]);
}

function initBarrierView() {
    ui.setListItemDisplayColumns(barrierView, ["fit", "header"], [
        { name: "name", tip: "fire name", width: "10rem", attrs: [], map: e => util.maxString(e.name, 20) },
        ui.listItemSpacerColumn(1.4),
        { name: "type", tip: "barrier type (hand-/dozer-line, seat-/lat-/vlat-drop)", width: "7rem", attrs: [], map: e => e.barrierType },
        ui.listItemSpacerColumn(1.4),
        { name: "width", tip: "barrier line width [ft]", width: "3rem", attrs:["fixed","alignRight"], map: e=> e.width }
    ]);
}

function inputTypes(e) {
    let t = "";
    if (e.ef.computeWind) t += 'w';
    if (e.ef.fixed) t += 'f';
    if (e.ef.spotting) t += 's';
    if (e.ef.barriers && e.ef.barriers.length > 0) t += 'b';

    return t;
}

function isShownWind(ae) {
    return shownWind !== undefined && shownWind.entry === selectedEntry && timestepIndex(selectedEntry, ae) == shownWind.idx;
}

function timestepIndex(e, ae) {
    return (e && e.ef.output) ? e.ef.output.timesteps.indexOf(ae) : -1;
}

function isRequestedTimeStep(ae) {
    return (selectedEntry !== undefined && selectedEntry !== null) && selectedEntry.shownTimeSteps.has( getTimestepSeconds(selectedEntry, ae));
}

function initDisplayControls() {
    let e = ui.getField("fires.toa.stroke_color");
    ui.setField(e, render.toa.stroke.toCssHexString());

    e = ui.getField("fires.toa.fill_color");
    ui.setField(e, render.toa.fill.toCssHexString());

    e = ui.getSlider('fires.toa.stroke_width');
    ui.setSliderRange(e, 0, 5.0, 0.5, util.f_1);
    ui.setSliderValue(e, render.toa.strokeWidth);

    e = ui.getSlider('fires.toa.fill_alpha');
    ui.setSliderRange(e, 0, 1.0, 0.1, util.f_1);
    ui.setSliderValue(e, render.toa.alpha);
}

function selectImgOutput() {
    if (selectedEntry) {
        selectedEntry.removeImgOutputAssets();
        let imgType = ui.getSelectedChoiceValue(imgCh);
        if (imgType && imgType !== "-") {
            selectedEntry.showImgOutputAsset(imgType);
        }
    }

    odinCesium.requestRender();
}

// the "wnd" choice selects the display type of the shown wind field ("-" hides it)
function selectWndOutput() {
    if (shownWind) {
        if (selectedWindDisplay()) {
            updateWindDisplay();
        } else {
            hideWind();
            ui.updateListItems(timestepView);
        }
    }
}

function selectedWindDisplay() {
    switch (ui.getSelectedChoiceValue(wndCh)) {
        case "anim": return wf.AnimDisplay;
        case "vec": return wf.VectorDisplay;
        case "cntr": return wf.ContourDisplay;
        default: return undefined;
    }
}

// show the wind field of shownWind for the selected display type (and hide the other display types).
// This also applies the current render options since they might have changed while the field was not showing
function updateWindDisplay() {
    if (shownWind) {
        let displayType = selectedWindDisplay();
        for (let dt of [wf.AnimDisplay, wf.VectorDisplay, wf.ContourDisplay]) {
            let field = shownWind.entry.getWindField(shownWind.idx, dt);
            if (field) {
                if (dt === displayType && layerShow) {
                    field.setRenderOpts( windRenderOpts(dt));
                    field.setVisible(true);
                } else {
                    field.setVisible(false);
                }
            }
        }
    }
}

function hideWind() {
    if (shownWind) {
        let fields = shownWind.entry.windFields[shownWind.idx];
        if (fields) Object.values(fields).forEach( f => f.setVisible(false));
        shownWind = undefined;
    }
}

function windRenderOpts(displayType) {
    return windRender.renderOpts(displayType);
}

// called by windRender when the user changed render options of the given display type
function windRenderChanged(displayType) {
    if (shownWind && Object.is( selectedWindDisplay(), displayType)) {
        let field = shownWind.entry.getWindField(shownWind.idx, displayType);
        if (field) field.setRenderOpts( windRenderOpts(displayType));
    }
}

// remove all visible outlines, fills and image assets of all runs. This also clears the fireView and timestepView
// selections but does not change the display mode (showSelectedEntryMode)
function clearAllAssets() {
    ui.selectChoiceItemIndex(imgCh, 0);
    ui.selectChoiceItemIndex(wndCh, 0);

    shownWind = undefined;
    fireEntries.forEach( e => {
        e.resetDisplayState(); // clears shown runs and requested timesteps
        e.cleanUp();           // removes loaded toa data, wind fields (re-loaded on demand) and image assets
    });

    ui.clearSelectedListItem(fireView); // -> selectEntry() clears selectedEntry, selectedLevel and the timestepView
    ui.updateListItems(fireView);       // reset "show" checkboxes (in case there was no selection)
    odinCesium.requestRender();
}

function areaItemDt(e) {
    if (selectedEntry && selectedEntry.ef) {
        return util.formatFloat((e.date - selectedEntry.ef.date) / 3600000.0, 1);
    } else {
        return ""
    }
}

function setWindChoices() {
    ui.setChoiceItems("elmfire.showWind", ["-", "anim", "vec", "cntr"], 0);
}

function setImgChoices() {
    ui.setChoiceItems("elmfire.showImg", ["-", "toa", "flin", "vs", "cf", "fl"], 0);
}

function setIncidentChoices() {
    // this is in [km]
    ui.setChoiceItems("elmfire.nHours", [3, 4, 5, 6, 7, 8], 1);
    ui.setChoiceItems("elmfire.size", [24, 30, 42, 60], 0);
    ui.setChoiceItems(ignitionTypeCh, [IGNITION_POINT, IGNITION_LINE], 0);
}

function initFixedEntries() {
    let maxHours = 8;
    fixedEntries.push(new FixedEntry("ws", "mph", config.fixedWs, config.defaultWs, 0, 200, maxHours));
    fixedEntries.push(new FixedEntry("wd", "°", config.fixedWd, config.defaultWd, 0, 359, maxHours));
    fixedEntries.push(new FixedEntry("m1", "%", config.fixedM1, config.defaultM1, 0, 100, maxHours));
    fixedEntries.push(new FixedEntry("m10", "%", config.fixedM10, config.defaultM10, 0, 100, maxHours));
    fixedEntries.push(new FixedEntry("m100", "%", config.fixedM100, config.defaultM100, 0, 100, maxHours));
    fixedEntries.push(new FixedEntry("lh", "%", config.fixedLh, config.defaultLh, 0, 100, maxHours));
    fixedEntries.push(new FixedEntry("lw", "%", config.fixedLw, config.defaultLw, 0, 100, maxHours));
    fixedEntries.push(new FixedEntry("t2", "°F", config.fixedT2, config.defaultT2, 0, 130, maxHours));
    fixedEntries.push(new FixedEntry("tcc", "%", config.fixedTcc, config.defaultTcc, 0, 100, maxHours));

    ui.setListItems(fixedView, fixedEntries);
}

// are there some fixedEntries that still need values
// Valid cases are either all have at least one value or none has any value. Report types of all other ones
function getMissingFixed() {
    let missing = [];
    fixedEntries.forEach(fe => {
        if (fe.values.length == 0) missing.push(fe.type);
    });
    return missing.length < fixedEntries.length ? missing : [];
}

// the fixed wx input as an object with value arrays for each type ({ws: [..], wd: [..], ..} - see odin_elmfire::FixedInput),
// or undefined if no fixed values were entered
function getFixed() {
    if (fixedEntries.every( fe => fe.values.length == 0)) return undefined;

    let fixed = {};
    fixedEntries.forEach( fe => fixed[fe.type] = fe.values);
    return fixed;
}

function updateSpottingsChoices() {
    ui.setChoiceItems("elmfire.savedSpottings", spottingFileList, 0)
}

function setBarrierChoices() {
    let barrierChoices = [BARRIER_HAND_LINE,BARRIER_DOZER_LINE,"---", BARRIER_SEAT_DROP,BARRIER_LAT_DROP,BARRIER_VLAT_DROP]; // the standard user-creatable entries
    if (barrierFileList.length > 0) {
        barrierChoices.push("---");
        barrierChoices.push(...barrierFileList);
    }
    ui.setChoiceItems("elmfire.barrierChoice", barrierChoices, 0);
    ui.setChoiceItems("elmfire.barrierWidth", config.barrierWidths, 0);
}

function setInputDebouncers() {
    locEntry.addEventListener('input', util.debounce(processLocationChange, DEBOUNCE_SECS));
    nameEntry.addEventListener('input', util.debounce(processNameChange, DEBOUNCE_SECS));
}

function processLocationChange() {
    let loc = ui.getLocation(locEntry);
    if (!loc) {
        if (curCenterAsset) {
            dataSource.entities.remove(curCenterAsset);
            curCenterAsset = null;
            odinCesium.requestRender();
        }
    } else {
        if (!curCenterAsset) {
            let name = ui.getFieldValue(nameEntry);
            if (!name) name = "?";
            curCenterAsset = createSymEntity(name, loc);
            dataSource.entities.add(curCenterAsset);

        } else {
            let pos = Cesium.Cartesian3.fromDegrees(loc.lon, loc.lat);
            curCenterAsset.position = pos;
        }
        odinCesium.requestRender();
    }

    ignitionEntries.forEach( ign => ign.createAsset()); // visibility of point ignitions depends on center location
}

function processNameChange() {
    let name = ui.getFieldValue(nameEntry);
    if (!name) name = "?";

    if (curCenterAsset) {
        curCenterAsset.label.text = name;
        odinCesium.requestRender();
    }
}

function showError(msg) {
    if (msg) {
        ui.setVarText("elmfire.error", msg);
    } else {
        ui.clearVarText("elmfire.error");
    }
}

// these have to be used if we change the content programmatically

function setName(name) {
    ui.setField(nameEntry, name);
    processNameChange();
}

function setLocation(loc) {
    ui.setLocation(locEntry, loc);
    processLocationChange();
}

//--- ignitions

// the date for new ignitions: dtg + selected delay. If there is no dtg yet we set it to now
function newIgnitionDate() {
    let t = ui.getDTGEntryEpochMillis(dtgEntry);
    if (!t) {
        let t = Math.round(Date.now() / 60000) * 60000; // round to full minute
        ui.setDTGEntryEpochMillis(dtgEntry, t);
        return t;
    } else {
        let delay = ui.getHMValue(ignitionDelayEntry) || new ui.HMValue(0, 0);
        return t + delay.milliseconds();
    }
}

function nextIgnitionId() {
    return ignitionEntries.reduce( (maxId, ign) => Math.max(maxId, ign.id), 0) + 1;
}

function roundedGeoPoint(p) {
    return new main.GeoPoint( util.roundToDecimals(p.lon, 4), util.roundToDecimals(p.lat, 4));
}

function isSameLocation(p1, p2) {
    return Math.abs(p1.lon - p2.lon) < 0.00005 && Math.abs(p1.lat - p2.lat) < 0.00005; // locations are rounded to 4 decimals
}

function editIgnition() {
    // TBD
}

function addIgnition() {
    let ignType = ui.getSelectedChoiceValue(ignitionTypeCh);
    switch (ignType) {
        case IGNITION_POINT : addIgnitionPoint(); break;
        case IGNITION_LINE: addIgnitionLine(); break;
    }
}

// interactively pick a point ignition. This sets the location and dtg if they are not yet set
function addIgnitionPoint() {
    ui.setButtonActive(addIgnBtn, true);
    odinCesium.enterGeoPoint((loc) => {
        if (loc) {
            let p = roundedGeoPoint(loc);
            let date = newIgnitionDate();
            if (!ui.getLocation(locEntry)) setLocation(p);
            addIgnitionEntry( new Ignition( nextIgnitionId(), IGNITION_POINT, date, p));
        }
        ui.setButtonActive(addIgnBtn, false);
    });
}

// interactively pick a line ignition (a line string that gets broken up into lines for Elmfire)
function addIgnitionLine() {
    ui.setButtonActive(addIgnBtn, true);
    odinCesium.enterGeoLineString((geoLineString) => {
        if (geoLineString && geoLineString.points.length >= 2) {
            let pts = geoLineString.points.map( p => roundedGeoPoint(p));
            let date = newIgnitionDate();
            if (!ui.getLocation(locEntry)) setLocation(pts[0]);
            addIgnitionEntry( new Ignition( nextIgnitionId(), IGNITION_LINE, date, new main.GeoLineString(pts)));
        }
        ui.setButtonActive(addIgnBtn, false);
    });
}

function addIgnitionEntry(ign) {
    ign.createAsset();
    ignitionEntries.push(ign);
    ui.setListItems(ignitionView, ignitionEntries);
}

function selectIgnition() {
    let ign = ui.getSelectedListItem(ignitionView);
    if (ign) {
        let t0 = ui.getDTGEntryEpochMillis(dtgEntry);
        let t1 = ign.date.getTime();
        let delay = ui.HMValue.fromMillis( t1 - t0);
        ui.setHMValue(ignitionDelayEntry, delay);
    }
}

// "del" button: remove the selected ignition (and its asset)
function deleteIgnition() {
    let ign = ui.getSelectedListItem(ignitionView);
    if (ign) {
        ign.removeAsset();
        ignitionEntries = ignitionEntries.filter( e => !Object.is(e, ign));
        ui.removeListItem(ignitionView, ign);
    } else {
        alert("please select ignition to delete");
    }
}

// "clear" button: remove all ignitionEntries (and their assets)
function clearIgnitions() {
    ignitionEntries.forEach( ign => ign.removeAsset());
    ignitionEntries = [];
    ui.setListItems(ignitionView, ignitionEntries);
}

// replace ignitionEntries with the ignitions of a received (or locally created) Elmfire run (see odin_elmfire::Ignition)
function setIgnitions(ignitions) {
    ignitionEntries.forEach( ign => ign.removeAsset());
    ignitionEntries = ignitions ? ignitions.map( ign => new Ignition( ign.id, ign.ignitionType, ign.date, toGeoValue(ign.value))) : [];
    ignitionEntries.forEach( ign => ign.createAsset());
    ui.setListItems(ignitionView, ignitionEntries);
}

function pushFixedColumn() {
    fixedEntries.forEach((e) => e.pushValue());
    ui.updateListItems(fixedView);
}

function popFixedColumn() {
    fixedEntries.forEach((e) => e.popValue());
    ui.updateListItems(fixedView);
}

function clearFixed() {
    fixedEntries.forEach((e) => e.clear());
    ui.updateListItems(fixedView);
}

function pushFixed(fe) {
    fe.pushValue();
    ui.updateListItem(fixedView, fe);
}

function editHour(n, event) {
    ui.editListItemField(event.target, (item, newValue) => {
        item.setHour(n, newValue);
    });
}

function popFixed(fe) {
    fe.popValue();
    ui.updateListItem(fixedView, fe);
}

function saveSpotting() {
    // TBD
}

function clearSpotting() {
    code.setCode(spottingSrc, "");
}

function selectBarrierChoice(event) {
    let e = ui.getChoice(event.target);
    if (e) {
        let bType = ui.getSelectedChoiceValue(e);
        if (bType) {
            switch (bType) {
                case BARRIER_SEAT_DROP:
                    ui.setButtonValue(barrierBtn, "pick");
                    ui.selectChoiceItem(barrierWidthCh, config.seatDropWidth);
                    break;
                case BARRIER_LAT_DROP:
                    ui.setButtonValue(barrierBtn, "pick");
                    ui.selectChoiceItem(barrierWidthCh, config.latDropWidth);
                    break;
                case BARRIER_VLAT_DROP:
                    ui.setButtonValue(barrierBtn, "pick");
                    ui.selectChoiceItem(barrierWidthCh, config.vlatDropWidth);
                    break;
                case BARRIER_DOZER_LINE:
                    ui.setButtonValue(barrierBtn, "pick");
                    ui.selectChoiceItem(barrierWidthCh, config.dozerLineWidth);
                    break;
                case BARRIER_HAND_LINE:
                    ui.setButtonValue(barrierBtn, "pick");
                    ui.selectChoiceItem(barrierWidthCh, config.handLineWidth);
                    break;
                default:
                    ui.setButtonValue(barrierBtn, "add");
            }
        }
    }
}

function addBarrier() {
    let barrierType = ui.getSelectedChoiceValue(barrierTypeCh);

    if (isUserBarrier(barrierType)) { // we need a name and then interactively pick positions
        let barrierName = ui.getFieldValue(barrierEntry);
        if (!barrierName || barrierName.length == 0) {
            alert("please enter name for new " + barrierType + " barrier");
            return;
        }
        let barrierWidth = ui.getSelectedChoiceValue(barrierWidthCh);

        if (barrierType === BARRIER_SEAT_DROP || barrierType === BARRIER_LAT_DROP || barrierType === BARRIER_VLAT_DROP) {
            ui.setButtonActive(barrierBtn, true);
            odinCesium.enterGeoLine((geoLine) => {
                ui.setButtonActive(barrierBtn, false);
                if (geoLine) addBarrierEntry( new Barrier(barrierType, barrierName, geoLine, barrierWidth));
            });
        } else if (barrierType === BARRIER_DOZER_LINE || barrierType === BARRIER_HAND_LINE) {
            ui.setButtonActive(barrierBtn, true);
            odinCesium.enterGeoLineString((geoLineString) => {
                ui.setButtonActive(barrierBtn, false);
                if (geoLineString) addBarrierEntry(new Barrier(barrierType, barrierName, geoLineString, barrierWidth));
            });
        }

    } else { // file selection, no name required
        // TBD - for file selections the selected type is used as the name and the value, and the type is set to BARRIER_FILE
    }
}

function editBarrier() {
    let be = ui.getSelectedListItem(barrierView);
    if (be) {
        // TBD - only lines and drops can be edited
    }
}

// add (or replace) a user entered barrier. Note that barriers are identified by name
function addBarrierEntry(be) {
    be.createAsset();

    let idx = barrierEntries.findIndex((e) => e.name == be.name);
    if (idx >= 0) {
        barrierEntries[idx].removeAsset();
        barrierEntries[idx] = be;
    } else {
        barrierEntries.push(be);
    }
    ui.setListItems(barrierView, barrierEntries);
}

function isUserBarrier(barrier) {
    return (barrier === BARRIER_SEAT_DROP || barrier === BARRIER_LAT_DROP || barrier === BARRIER_VLAT_DROP ||
        barrier === BARRIER_DOZER_LINE || barrier === BARRIER_HAND_LINE);
}

// "del" button: remove the selected barrier (and its asset)
function deleteBarrier() {
    let be = ui.getSelectedListItem(barrierView);
    if (be) {
        be.removeAsset();
        barrierEntries = barrierEntries.filter( e => !Object.is(e, be));
        ui.removeListItem(barrierView, be);

        if (ui.getFieldValue(barrierEntry) == be.name) ui.clearField(barrierEntry);
    } else {
        alert("please select barrier to delete");
    }
}

function selectBarrier() {
    let selBarrier = ui.getSelectedListItem(barrierView);
    if (selBarrier) {
        ui.setField(barrierEntry, selBarrier.name);
        ui.selectChoiceItem(barrierTypeCh, selBarrier.barrierType);
        ui.selectChoiceItem(barrierWidthCh, selBarrier.width);
    }
}

// "clear" button: remove all barrierEntries (and their assets)
function clearBarriers() {
    barrierEntries.forEach( be => be.removeAsset());
    barrierEntries = [];
    ui.setListItems(barrierView, barrierEntries);
    ui.clearField(barrierEntry);
}

// replace barrierEntries with the barriers of a received (or locally created) Elmfire run (see odin_elmfire::Barrier).
// Only the current barrierEntries are rendered
function setBarriers(barriers) {
    barrierEntries.forEach( be => be.removeAsset());
    barrierEntries = barriers ? barriers.map( b => new Barrier( b.barrierType, b.name, toGeoValue(b.value), b.width)) : [];
    barrierEntries.forEach( be => be.createAsset());
    ui.setListItems(barrierView, barrierEntries);
}

// turn deserialized barrier/ignition values back into main.GeoPoint, main.GeoLine or main.GeoLineString objects
function toGeoValue(v) {
    let geoPoint = (p) => new main.GeoPoint( p.lon, p.lat);
    if (v.points) {
        return new main.GeoLineString( v.points.map( geoPoint));
    } else if (v.start && v.end) {
        return new main.GeoLine( geoPoint(v.start), geoPoint(v.end));
    } else if (v.lon !== undefined && v.lat !== undefined) {
        return geoPoint(v);
    } else {
        return v;
    }
}

function clearIncident() {
    ui.clearField(nameEntry);

    setLocation(null);
    ui.clearDTGEntry( dtgEntry);
    clearIgnitions();
    clearFixed();
    clearSpotting();
    clearBarriers();
}

// interactively pick a point ignition at the current time, which also sets location and dtg
function pickNow() {
    ui.setButtonActive(pickNowBtn, true);
    odinCesium.enterGeoPoint((loc) => {
        if (loc) {
            let p = roundedGeoPoint(loc);
            let date = Date.now();
            ui.setDTGEntryEpochMillis(dtgEntry, date);
            date = ui.getDTGEntryEpochMillis(dtgEntry); // might be rounded
            setLocation(p);
            addIgnitionEntry( new Ignition( nextIgnitionId(), IGNITION_POINT, date, p));
            ui.updateListItems(ignitionView); // dtg changed
        }
        ui.setButtonActive(pickNowBtn, false);
    });
}

function pickLocation() {
    odinCesium.enterGeoPoint( (loc) => {
        if (loc) {
            loc.lat = util.roundToDecimals(loc.lat, 4);
            loc.lon = util.roundToDecimals(loc.lon, 4);
            setLocation(loc)
        }
    });
}

//--- entry data display toggles

// the only way to trigger loading of toa data. Note that callers don't need to wait for completion since
// loadToa() applies the then current display state of the entry
function ensureToa(e) {
    if (e.toaDs) return Promise.resolve(e.toaDs);
    if (!e.toaLoading) e.toaLoading = loadToa(e);
    return e.toaLoading;
}

function loadToa(e) {
    let gen = e.toaGen;
    let url = `elmfire-data/${e.ef.id}/outputs/toa.geojson`;
    let toaRender = e.getToaRender();
    let renderOpts = {
        stroke: toaRender.stroke,
        strokeWidth: toaRender.strokeWidth,
        fill: toaRender.fill.withAlpha(toaRender.alpha)
    };

    return Cesium.GeoJsonDataSource.load(url, renderOpts).then( (ds) => {
        if (gen !== e.toaGen) return undefined; // entry was reset or replaced while we were loading

        addPolygons(e, ds);
        e.toaDs = ds;
        e.toaLoading = undefined;
        e.updateDisplay(); // apply current display state before it gets added
        // isochrones have to be rendered below our barrier/ignition/fire entities. Note this has to be done after ds was added
        odinCesium.addDataSource(ds).then( () => odinCesium.raiseDataSourceToTop(dataSource));
        console.log("loaded ", url);
        return ds;

    }).catch( (reason) => {
        if (gen === e.toaGen) e.toaLoading = undefined;
        console.log("failed to load: ", url, ", reason: ", reason);
        return undefined;
    });
}

function addPolygons(fireEntry, ds) {
    let toaRender = fireEntry.getToaRender();
    let fillMaterial = new Cesium.ColorMaterialProperty(toaRender.fill.withAlpha(toaRender.alpha));
    let removed = [];

    for (const e of ds.entities.values) {
        if (e.polyline) {
            let nPoints = e.polyline.positions._value.length;
            if (nPoints > config.contourThreshold) {
                if (!e.polygon) {
                    e.polygon = {
                        hierarchy: e.polyline.positions._value,
                        material: fillMaterial,
                        outline: false,
                        //heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
                        fill: false // set when we select a time step
                    };
                    e.addProperty('polygon');
                }
            } else { // remove small contour artifacts (can't do this while we iterate over entities)
                removed.push(e);
            }
        }
    }

    for (const e of removed) {
        ds.entities.remove(e);
    }

    addTimeStepLabels(fireEntry, ds);
}

// add a label with the timestep in hours (corresponding to the timestepView "dt" column) to the largest outline
// of each isochrone level. Labels are placed on the vertex with the widest gap to the neighboring isochrones so that
// they don't end up between narrowly spaced outlines
function addTimeStepLabels(fireEntry, ds) {
    let dt0 = (fireEntry.ef.date % 3600000) / 1000;  // secs between full hour and first ignition (see getTimestepSeconds)

    //--- group outline entities by level
    let levels = new Map(); // level -> [entity]
    for (const e of ds.entities.values) {
        let lvl = getPropValue(e.properties, "level");
        if (e.polyline && lvl !== undefined) {
            let es = levels.get(lvl);
            if (es) es.push(e); else levels.set(lvl, [e]);
        }
    }
    let sortedLevels = [...levels.keys()].sort((a,b) => a - b);

    //--- per level vertices for the gap computation (subsampled for large outlines)
    let levelPts = sortedLevels.map( lvl => subsample( levels.get(lvl).flatMap( e => outlinePositions(e)), config.labelMaxNeighborPts || 400));

    sortedLevels.forEach( (lvl, i) => {
        // label only the largest outline of this level (there can be several, e.g. from spot fires)
        let e = levels.get(lvl).reduce( (a,b) => outlinePositions(a).length >= outlinePositions(b).length ? a : b);
        let pos = getLabelPosition( outlinePositions(e), levelPts[i-1], levelPts[i+1]);

        if (pos) {
            let dt = (lvl - dt0) / 3600.0;
            e.position = pos;
            e.label = new Cesium.LabelGraphics({
                text: util.formatFloat(dt, 1),
                font: config.smallLabelFont,
                fillColor: config.labelColor,
                showBackground: true,
                backgroundColor: config.labelBackground,
                horizontalOrigin: Cesium.HorizontalOrigin.CENTER,
                verticalOrigin: Cesium.VerticalOrigin.CENTER,
                distanceDisplayCondition: config.labelDC,
                heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
                disableDepthTestDistance: Number.POSITIVE_INFINITY,
                show: false // set in FireEntry.updateDisplay() according to outline visibility
            });
        }
    });
}

function outlinePositions(e) {
    return e.polyline.positions.getValue(Cesium.JulianDate.now()) || [];
}

function subsample(pts, maxPts) {
    if (pts.length <= maxPts) return pts;
    let stride = Math.ceil(pts.length / maxPts);
    return pts.filter( (p,i) => i % stride == 0);
}

// pick the candidate vertex with the largest minimum distance to the inner and outer neighbor isochrones
function getLabelPosition(pts, innerPts, outerPts) {
    if (pts.length == 0) return undefined;

    let step = Math.max(1, Math.floor(pts.length / (config.labelCandidates || 24)));
    let best = undefined;
    let bestGap = -1;

    for (let i = 0; i < pts.length; i += step) {
        let p = pts[i];
        let gap = Math.min( minDistance(p, innerPts), minDistance(p, outerPts));
        if (gap > bestGap) {
            bestGap = gap;
            best = p;
        }
    }
    return best;
}

function minDistance(p, pts) {
    if (!pts || pts.length == 0) return Number.MAX_VALUE; // no neighbor on this side (first or last isochrone)
    let dMin = Number.MAX_VALUE;
    for (const q of pts) {
        let d = Cesium.Cartesian3.distanceSquared(p, q); // squared is enough for comparison
        if (d < dMin) dMin = d;
    }
    return dMin;
}

function createRasterEntity (url, bbox, renderOpts) {
    return new Cesium.Entity({
        rectangle: {
            coordinates: Cesium.Rectangle.fromDegrees(bbox.west, bbox.south, bbox.east, bbox.north),
            material: new Cesium.ImageMaterialProperty({
                image: url,
                //color: renderOpts.color
            }),
            classificationType: Cesium.ClassificationType.TERRAIN
        }
    });
}

// "show sel" checkbox - explicitly switches modes
function toggleShowSelectedEntry(event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        if (ui.isCheckBoxSelected(cb)) { // -> showSelectedEntryMode: clear all requested timesteps but show all of the selected run
            showSelectedEntryMode = true;
            fireEntries.forEach( e => e.resetDisplayState());
            if (selectedEntry) {
                selectedEntry.isShown = true;
                ensureToa(selectedEntry);
            }

        } else { // -> explicit timestep mode: keep what is showing by turning all timesteps of the selected run into requests
            showSelectedEntryMode = false;
            if (selectedEntry && selectedEntry.isShown) {
                selectedEntry.shownTimeSteps = new Set( selectedEntry.allLevels());
            }
        }

        updateAllDisplays();
    }
}

// set mode without user interaction (from timestep request)
function setExplicitTimeStepMode() {
    showSelectedEntryMode = false;
    ui.setCheckBox(selCb, false);
    fireEntries.forEach( e => e.resetDisplayState());
}

function updateAllDisplays() {
    fireEntries.forEach( e => e.updateDisplay());
    ui.updateListItems(fireView);
    ui.updateListItems(timestepView);
    odinCesium.requestRender();
}

// show/hide selected fire entry
function toggleShowFires(event) {
    showFires( ui.isCheckBoxSelected(event.target));
}

// "show" checkbox of a fireView item. Note this is executed before the item gets selected (i.e. before selectEntry)
// so we have to use the item of the checkbox and not selectedEntry
function toggleShowToa(event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let e = ui.getListItemOfElement(cb);
        if (e) {
            if (ui.isCheckBoxSelected(cb)) {
                e.isShown = true;
                if (!showSelectedEntryMode) e.shownTimeSteps = new Set( e.allLevels()); // shortcut to request all timesteps
                ensureToa(e);

            } else { // remove all outlines and fills of this run, regardless of mode
                e.resetDisplayState();
                if (e === selectedEntry) {
                    selectedLevel = undefined;
                    ui.clearSelectedListItem(timestepView);
                }
            }

            e.updateDisplay();
            if (e === selectedEntry) ui.updateListItems(timestepView);
        }
    }
}

// timestepView item checkbox - requests a timestep. Note this is executed before the item gets selected (i.e. before
// selectTimeStep). Requesting a timestep implicitly switches to explicit timestep mode
function toggleShowIso(event) {
    let cb = ui.getCheckBox(event.target);
    if (cb && selectedEntry) {
        let ae = ui.getListItemOfElement(cb);
        if (ae) {
            let lvl = getTimestepSeconds(selectedEntry, ae);
            let isSelected = ui.isCheckBoxSelected(cb);

            if (showSelectedEntryMode) {
                setExplicitTimeStepMode(); // this clears all previous requests and shown runs
                updateAllDisplays();       // .. which might reset our checkbox
            }

            if (isSelected) {
                selectedEntry.shownTimeSteps.add(lvl);
            } else {
                selectedEntry.shownTimeSteps.delete(lvl);
            }
            selectedEntry.isShown = selectedEntry.shownTimeSteps.size > 0;

            ui.updateListItem(fireView, selectedEntry);
            ui.updateListItem(timestepView, ae);
            selectedEntry.updateDisplay();
            if (selectedEntry.isShown) ensureToa(selectedEntry);
        }
    }
}

// timestepView item "wnd" checkbox - shows the wind field of this timestep. Only one wind field can be shown at a time,
// i.e. this replaces a previously shown wind field (of any run). The display type is set by the "wnd" choice (wndCh).
// Note this is executed before the item gets selected, i.e. we have to use the item of the checkbox
function toggleShowWind(event) {
    let cb = ui.getCheckBox(event.target);
    if (cb && selectedEntry) {
        let ae = ui.getListItemOfElement(cb);
        if (ae) {
            hideWind();

            if (ui.isCheckBoxSelected(cb)) {
                let idx = timestepIndex(selectedEntry, ae);
                if (selectedEntry.hasWindFields() && idx >= 0) {
                    if (!selectedWindDisplay()) ui.selectChoiceItemIndex(wndCh, 1); // default to animation
                    shownWind = { entry: selectedEntry, idx };
                    updateWindDisplay();
                } else {
                    alert("no wind field output for this simulation");
                }
            }

            ui.updateListItems(timestepView); // reset other "wnd" checkboxes
        }
    }
}

//--- button click handlers

function createRequest() {
    let nHours = ui.getSelectedChoiceValue("elmfire.nHours");
    let size = ui.getSelectedChoiceValue("elmfire.size");

    let name = ui.getFieldValue( nameEntry);
    if (name == "") {
        alert("no name");
        return;
    }

    let pos = ui.getLocation(locEntry);
    if (!pos) {
        alert("no center location");
        return;
    }

    let date = ui.getDTGEntryEpochMillis(dtgEntry);
    if (!date) {
        alert("no date");
        return;
    }

    // note - this should be kept in sync with odin_elmfire::ElmFire::new()
    let id = FireEntry.getId(name, date, pos);

    if (ignitionEntries.length == 0) {
        alert("no ignitions");
        return;
    }
    // ignitions have to be within the simulated time window (starting at the full hour of date)
    let tBase = date - (date % 3600000);
    let tEnd = date + nHours * 3600000;
    let invalid = ignitionEntries.find( ign => ign.date < tBase || ign.date > tEnd);
    if (invalid) {
        alert(`ignition ${invalid.id} is outside of simulation time window`);
        return;
    }
    let ignitions = ignitionEntries.map( ign => ign.toJSON()); // without Cesium assets

    let missingFixed = getMissingFixed();
    if (missingFixed.length > 0) {
        alert(`missing "fixed wx entries: ${missingFixed.join()}`);
        return;
    }

    let computeWind = ui.isCheckBoxSelected("elmfire.computeWind");
    let elmfire = { id, name, nHours, size, date, pos, computeWind, ignitions };

    //--- now set the optional elmfire inputs

    let fixed = getFixed();
    if (fixed) {
        elmfire.fixed = fixed;
    }

    let spotting = code.getCode(spottingSrc);
    if (spotting.length > 0) {
        elmfire.spotting = spotting;
    }

    let barriers = barrierEntries.map( be => be.toJSON()); // possibly empty, without Cesium assets
    elmfire.barriers = barriers;

    updateElmfires( [elmfire], elmfire);
    // set list selection

    ws.sendWsMessage( MOD_PATH, "elmfire", elmfire);
}

//--- view update

function updateFireView() {
    let entries = Array.from(fireEntries.values()).sort((a, b) => util.defaultCompare(a.id, b.id));
    let selEntry = selectedEntry ? fireEntries.get(selectedEntry.id) : undefined; // selectedEntry might have been replaced

    repopulatingFireView = true; // setListItems() resets the selection, which should not change our state
    ui.setListItems(fireView, entries);
    repopulatingFireView = false;

    if (selEntry) ui.setSelectedListItem(fireView, selEntry); // no-op in selectEntry() if this is still the same object
}

function zoomToSelection(event) {
    let sel = ui.getSelectedListItem(event);
    if (sel) {
        let area = sel.getToaBounds();
        odinCesium.zoomToOffsetRect(area, 0.1, -0.1, 0.0);
    }
}

// select an Elmfire run from the fireView
// note this is also called if the item was already selected (re-select or re-populated list), which has to be a no-op
function selectEntry() {
    if (repopulatingFireView) return;

    let prevEntry = selectedEntry;
    let entry = ui.getSelectedListItem(fireView);
    if (entry === prevEntry) return;

    selectedEntry = entry;
    selectedLevel = undefined; // timestepView gets re-populated

    if (showSelectedEntryMode) {
        fireEntries.forEach( e => { if (e !== entry) e.isShown = false; });
        if (entry) entry.isShown = true;
    }

    if (prevEntry) {
        prevEntry.removeImgOutputAssets(); // the "img" choice only applies to the selected run
        prevEntry.updateDisplay(); // removes its fill (and in showSelectedEntryMode its outlines)
    }
    if (entry) {
        entry.setToaRender(); // following selections use the current display parameters
        entry.updateDisplay();
        if (entry.isShown) ensureToa(entry);
    }
    ui.updateListItems(fireView);

    selectImgOutput();
    updateStatsView();
    updateAreaView();
    updateErrorView();
    if (entry) { // don't clear input when de-selecting (user might be entering a new run)
        updateInput();
        setIgnitions( entry.ef.ignitions); // replaces (and renders) ignitionEntries
        setBarriers( entry.ef.barriers); // replaces (and renders) barrierEntries
    }
}

// select a timestep from the timestepView. Only the selected timestep of the selectedEntry is filled (if its outline is visible)
function selectTimeStep() {
    let ae = ui.getSelectedListItem(timestepView);
    selectedLevel = (selectedEntry && ae) ? getTimestepSeconds(selectedEntry, ae) : undefined;
    if (selectedEntry) selectedEntry.updateDisplay();
}

function getTimestepSeconds(e, ae) {
    let dt0 = (e.ef.date % 3600000) / 1000;
    let t = ((ae.date - e.ef.date) / 1000) + dt0;
    return t;
}

function getPropValue(props,key) {
    let p = props[key];
    return p ? p._value : undefined;
}

function updateStatsView() {
    let e = selectedEntry;
    if (e && e.ef.output) {
        ui.setKvList(statsView, getKvList(e.ef));
    } else {
        ui.clearKvList(statsView);
    }
}

function updateAreaView() {
    let e = selectedEntry;
    if (e && e.ef.output) {
        ui.setListItems(timestepView, e.ef.output.timesteps);
    } else {
        ui.clearList(timestepView);
    }
}

function updateErrorView() {
    let e = selectedEntry;
    showError( e ? e.ef.error : null); // clear stale errors if there is no selection
}

function updateInput() {
    let e = selectedEntry;
    if (e) {
        let ef = e.ef;
        setName( ef.name);
        setLocation( ef.pos);
        ui.setDTGEntryEpochMillis(dtgEntry, ef.date);
        ui.setCheckBox("elmfire.computeWind", ef.computeWind);

        updateFixedEntries(ef.fixed);
        code.setCode(spottingSrc, ef.spotting);

    } else {
        ui.clearField(nameEntry);
        setLocation( null);
        ui.clearDTGEntry(dtgEntry);
        ui.setCheckBox("elmfire.computeWind", false);

        clearIgnitions();
        clearFixed();
        code.clearCode(spottingSrc);
    }
}

function updateFixedEntries(efFixed) {
    fixedEntries.forEach(fe => fe.clear());

    if (efFixed) {
        Object.keys(efFixed).forEach( t => {
            let fe = fixedEntries.find(fe => fe.type == t);
            if (fe) fe.values = [...efFixed[t]]; // copy so that editing entries does not change the run
        })
    }

    ui.updateListItems(fixedView);
}

function getKvList(ef) {
    let stats = ef.output.stats[0];
    let compStats = ef.output.compStats;
    return [
        ["date", util.toLocalDateString(stats.date)],
        ["time", util.toLocalHMTimeString(stats.date)],

        ["t-stop [h]", ef.nHours],
        ["total area [ac]", stats.totalArea],
        ["crown area [ac]", stats.crownArea],
        ["volume [ac-ft]", stats.volume],
        ["HRR peak [MW}", stats.hrrPeak],
        ["PM2.5 release [ug]", stats.pm25],

        ["max flin [kW/m]", compStats.flinMax],
        ["max fl [ft]", compStats.flMax],
        ["max vs [ft/min]", compStats.vsMax]
        //... and more to follow
    ];
}

// entity -> list
function fireSelection() {
    let sel = odinCesium.getSelectedEntity();
    if (sel && sel._uiElmFire) {
        ui.setSelectedListItem(fireView, sel._uiElmFire);
    }
}

function showFires(cond) {
    layerShow = cond;
    dataSource.show = cond;
    imgDataSource.show = cond;
    fireEntries.forEach( e => e.updateDisplay());
    updateWindDisplay(); // wind fields are not part of our dataSource
    odinCesium.requestRender();
}

//--- spotting  and wx specs

function selectSpotting() {
    let name = ui.getSelectedChoiceValue("elmfire.savedSpottings");
    if (name) {
        loadSpec("spotting", name, spottingSrc);
        ui.setField("elmfire.spottingName", name);
    }
}

function storeSpotting() {
    storeSpec(spottingSrc, "elmfire.spottingName", "storeSpotting");
}

function delSpotting() {
    delSpec("elmfire.spottingName", "delSpotting");
}

function updateSpottingView() {
    ui.setListItems(spottingListView, spottingFileList);
}


function storeSpec(tgtTextArea, nameField, msgType) {
    let source = ui.getTextAreaContent(tgtTextArea);
    if (!source || source.length == 0) {
        alert("no source");
        return;
    }

    let name = ui.getFieldValue( nameField);
    if (!name || name.length == 0) {
        alert("no name");
        return;
    }

    ws.sendWsMessage( MOD_PATH, msgType, { name, source });
}

function delSpec(nameField, msgType) {
    let name = ui.getFieldValue(nameField);
    if (!name || name.length == 0) {
        alert("no name");
        return;
    }

    ws.sendWsMessage( MOD_PATH, msgType, { name });
}

async function loadSpec(specType, name, tgtTextArea) {
    let url = `elmfire-${specType}/${name}`;

    fetch(url).then((response) => {
        if (response.ok) {
            let txt = response.text();
            if (txt) {
                txt.then((src) => {
                    //ui.setTextAreaContent(tgtTextArea, src);
                    //code.setCode(tgtTextArea, src);
                    ui.setValue(tgtTextArea, src); // works for both code-input and textarea
                });
            }
        } else console.log("request failed: ", url);
    }, (reason) => console.log("failed to retrieve: ", url, ", reason: ", reason));
}

//--- interactive display parameters

// display parameter changes only affect the selected run and following selections (other runs keep their rendering)
function resetRendering() {
    render = util.levelOneCloneOf(config.render);
    initDisplayControls();
    if (selectedEntry) selectedEntry.setToaRender();
}

function toaStrokeWidthChanged(event) {
    render.toa.strokeWidth = ui.getSliderValue(event.target);
    if (selectedEntry) selectedEntry.setToaRender();
}

function toaStrokeColorChanged(event) {
    let clrSpec = event.target.value;
    if (clrSpec) {
        render.toa.stroke = Cesium.Color.fromCssColorString(clrSpec);
        if (selectedEntry) selectedEntry.setToaRender();
    }
}

function toaFillColorChanged(event) {
    let clrSpec = event.target.value;
    if (clrSpec) {
        render.toa.fill = Cesium.Color.fromCssColorString(clrSpec);
        if (selectedEntry) selectedEntry.setToaRender();
    }
}

function toaFillAlphaChanged(event) {
    render.toa.alpha = ui.getSliderValue(event.target);
    if (selectedEntry) selectedEntry.setToaRender();
}

function createSymEntity( name, pos) {
    let entity = new Cesium.Entity({
        position: Cesium.Cartesian3.fromDegrees( pos.lon, pos.lat),
        billboard: {
            image: "./asset/odin_elmfire/fire.png",
            distanceDisplayCondition: config.billboardDC,
            color: config.billboardColor,
            heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
            disableDepthTestDistance: Number.MAX_SAFE_INTEGER,  // otherwise symbol might get clipped by terrain
            alignedAxis: Cesium.Cartesian3.UNIT_Z,
        },
        label: {
            text: name,
            //scale: 0.8,
            horizontalOrigin: Cesium.HorizontalOrigin.LEFT,
            verticalOrigin: Cesium.VerticalOrigin.TOP,
            font: config.labelFont,
            fillColor: config.labelColor,
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
            color: config.pointColor,
            outlineColor: config.pointOutlineColor,
            outlineWidth: config.pointOutlineWidth,
            //heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
            //disableDepthTestDistance:Number.MAX_SAFE_INTEGER,
            distanceDisplayCondition: config.pointDC,
        }
    });

    return entity
}
