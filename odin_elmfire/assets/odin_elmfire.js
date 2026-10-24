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
import * as data from "../odin_server/ui_data.js";
import * as ui from "../odin_server/ui.js";
import * as code from "../odin_server/ui_code_input.js";
import * as edit from "../odin_cesium/editor.js";
import * as ws from "../odin_server/ws.js";
import * as main from "../odin_server/main.js";
import * as odinCesium from "../odin_cesium/odin_cesium.js";
import * as wf from "../odin_wind/windfield.js";

const MOD_PATH = "odin_elmfire::service::ElmfireService";
const DEBOUNCE_SECS = 3;

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
                    dataSource.entities.add(entity);
                }
            }
        }
    }

    removeImgOutputAssets() {
        let assets = this.assets;
        Object.keys(assets).forEach((key) => {
            dataSource.entities.remove(assets[key]);
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

/* #endregion ElmfireEntry */

var render = util.levelOneCloneOf( config.render);

var fireView = undefined;
var timestepView = undefined;
var selCb = undefined;
var imgCh = undefined;
var wndCh = undefined;
var nameEntry = undefined;
var locEntry = undefined;
var dtgEntry = undefined;
var ignitionSrc = undefined;
var fixedSrc = undefined;
var spottingSrc = undefined;
var barrierSrc = undefined;

var statsView = undefined;

var fireEntries = new Map();  // id -> FireEntry

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
var curIgnitionAssets = [];

var spottingList = []; // the list of known spotting config files we get from the server

createIcon();
createWindow();

initFireView();
initTimestepView();

setWindChoices();
setImgChoices();
setIncidentChoices();
setFixedWxChoices();
setBarrierChoices();

setInputDebouncers();
initDisplayControls();
windRender.initialize();

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

    curIgnitionAssets.forEach((a) => dataSource.entities.remove(a));
    curIgnitionAssets = [];
    odinCesium.requestRender();
}

function handleSpottingMessage(newSpottingList) {
    spottingList = ["-", ...newSpottingList]; // prepend a none-choice
    updateSpottingsChoices();
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
            ui.RowContainer()(
                ui.Button("clear all", clearIncident),
                ui.HorizontalSpacer(2),
                ui.Button("pick now", pickNow),
                ui.Button("simulate", createRequest)
            ),
            ui.RowContainer()(
                (nameEntry = ui.TextInput("name", "elmfire.name", "11rem", { placeHolder: "event name" })),
                ui.HorizontalSpacer(0.5),
                ui.Choice("hours", "elmfire.nHours", null, "3rem"),
                ui.HorizontalSpacer(0.5),
                ui.Choice("grid [km]", "elmfire.size", null, "3rem"),
                ui.HorizontalSpacer(0.5),
                ui.CheckBox( "comp wind", null, "elmfire.computeWind")
            ),

            ui.RowContainer()(
                (locEntry = ui.LocationEntry("loc", "elmfire.location", pickLocation)),
                ui.HorizontalSpacer(0.5),
                (dtgEntry = ui.DTGEntry("dtg", "elmfire.date"))
            ),

            ui.VerticalSpacer(0.3),
            ui.TabbedContainer()(
                ui.Tab("ignitions", true)(
                    (ignitionSrc = code.CodeInput("elmfire.ignitionSrc", "nml-highlight", "fortran-namelist", "35rem", "11lh")),
                    ui.RowContainer()(
                        ui.Button("clear", clearIgnitions),
                        ui.HorizontalSpacer(1.0),
                        ui.Button("add point", addIgnitionPoint),
                        ui.Button("add line", addIgnitionLine),
                        ui.Button("check", checkIgnitions)
                    )
                ),
                ui.Tab("fixed wx", false)(
                    (fixedSrc = code.CodeInput("elmfire.fixedSrc", "nml-highlight", "fortran-namelist", "35rem", "11lh")),
                    ui.RowContainer()(
                        ui.Choice("wd", "elmfire.fixedWd", null, "3.5rem", "3rem"),
                        ui.Choice("ws", "elmfire.fixedWs", null, "3.5rem", "3rem"),
                        ui.Choice("t2", "elmfire.t2", null, "3.5rem", "3rem"),
                        ui.Choice("tcc", "elmfire.tcc", null, "3.5rem", "3rem"),
                        ui.HorizontalSpacer(1.0),
                        ui.Button("create", createFixedSrc, "4rem")
                    ),
                    ui.RowContainer()(
                        ui.Choice("m1", "elmfire.fixedM1", null, "3.5rem", "3rem"),
                        ui.Choice("m10", "elmfire.fixedM10", null, "3.5rem", "3rem"),
                        ui.Choice("m100", "elmfire.fixedM100", null, "3.5rem", "3rem"),
                        ui.Choice("lfm", "elmfire.lfMoisture", null, "3.5rem", "3rem"),
                        ui.HorizontalSpacer(1.0),
                        ui.Button("clear", clearFixed, "4rem")
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
                    (barrierSrc = code.CodeInput("elmfire.barrierSrc", "nml-highlight", "fortran-namelist", "35rem", "11lh")),
                    ui.RowContainer()(
                        ui.Button("clear", clearBarriers),
                        ui.HorizontalSpacer(1.0),
                        ui.Button("line", pickBarrierLine),
                        ui.Choice("width [ft]", "elmfire.barrierWidth", null, "3rem")
                    )
                )
            )
        ),
        ui.Panel("toa display", false)(
            ui.RowContainer()(
                ui.ColorField("toa stroke", "fires.toa.stroke_color", true, toaStrokeColorChanged, "2rem"),
                ui.initSlider("width", "fires.toa.stroke_width", toaStrokeWidthChanged, 1, 3, 1, util.f_0, 1)
            ),
            ui.RowContainer()(
                ui.ColorField("toa fill", "fires.toa.fill_color", true, toaFillColorChanged, "2rem"),
                ui.initSlider("alpha", "fires.toa.fill_alpha", toaFillAlphaChanged, 0, 1, 0.1, util.f_1, 0.7)
            ),
            ui.Button("reset all", resetRendering)
        ),
        windRender.displayPanel("wind display")
    );

    ignitionSrc.placeholder = "Elmfire ignition input (NML format)";
    fixedSrc.placeholder = "Elmfire fixed wind and fuel moisture input (NML format)";
    spottingSrc.placeholder = "Elmfire spotting input (NML format)";
    barrierSrc.placeholder = "Elmfire barrier input (NML format)";

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

function inputTypes(e) {
    let t = "";
    if (e.ef.computeWind) t += 'w';
    if (e.ef.fixed) t += 'f';
    if (e.ef.spotting) t += 's';
    if (e.ef.barriers) t += 'b';

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
}

function setFixedWxChoices() {
    // initialize from config so that we can override based on configured location
    ui.setChoiceItems("elmfire.fixedWs", config.fixedWs, config.defaultWs);
    ui.setChoiceItems("elmfire.fixedWd", config.fixedWd, config.defaultWd);
    ui.setChoiceItems("elmfire.fixedM1", config.fixedM1, config.defaultM1);
    ui.setChoiceItems("elmfire.fixedM10", config.fixedM10, config.defaultM10);
    ui.setChoiceItems("elmfire.fixedM100", config.fixedM100, config.defaultM100);
    ui.setChoiceItems("elmfire.lfMoisture", [...config.lfmMap.keys()], config.defaultLfm);
    ui.setChoiceItems("elmfire.t2", config.fixedT2, config.defaultT2);
    ui.setChoiceItems("elmfire.tcc", config.fixedTcc, config.defaultTcc);
}

function updateSpottingsChoices() {
    ui.setChoiceItems("elmfire.savedSpottings", spottingList, 0)
}

function setBarrierChoices() {
    ui.setChoiceItems("elmfire.barrierWidth", [15,20,25,30,40], 0)
}

function setInputDebouncers() {
    ignitionSrc.addEventListener('input', util.debounce(processIgnitionChange, DEBOUNCE_SECS));
    locEntry.addEventListener('input', util.debounce(processLocationChange, DEBOUNCE_SECS));
    nameEntry.addEventListener('input', util.debounce(processNameChange, DEBOUNCE_SECS));
}

function processIgnitionChange() {
    let igns = getIgnitions();
    if (curIgnitionAssets) removeIgnitionAssets();

    if (igns) {
        curIgnitionAssets = createIgnitionEntities(igns);
        curIgnitionAssets.forEach((a) => dataSource.entities.add(a));
    }

    odinCesium.requestRender();
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

function setIgnitionCode(src) {
    code.setCode(ignitionSrc, src);
    processIgnitionChange();
}

function addIgnitionLine() {
    // TBD
}

function addIgnitionPoint() {
    odinCesium.enterGeoPoint((loc) => {
        if (loc) {
            loc.lat = util.roundToDecimals(loc.lat, 4);
            loc.lon = util.roundToDecimals(loc.lon, 4);

            let t = ui.getDTGEntryEpochMillis(dtgEntry);
            if (!t) {
                ui.setDTGEntryEpochMillis(dtgEntry, Date.now());
                t = ui.getDTGEntryEpochMillis(dtgEntry);
            }
            if (!ui.getLocation(locEntry)) {
                setLocation( loc);
            }

            let t0 = Math.round((t % 3600000) / 1000);
            let src = addPointIgnitionSource(loc, t0);
            setIgnitionCode(src);
        }
    });
}

function checkIgnitions() {
    let src = code.getCode(ignitionSrc);
    let loc = ui.getLocation(locEntry);
    if (loc) {
        try {
            let igns = parseIgnitions(src, loc);
            return true;
        } catch (e) { alert(e) }
    } else { alert("no center location set") }
    return false;
}

// this one does not report parse errors but just returns ignitions
function getIgnitions() {
    let src = code.getCode(ignitionSrc);
    let loc = ui.getLocation(locEntry);
    if (loc) {
        try {
            return parseIgnitions(src, loc);
        } catch (e) { return null; }
    } else { return null; }
}

function clearIgnitions() {
    code.setCode(ignitionSrc, "");
    removeIgnitionAssets();
}

function removeIgnitionAssets() {
    curIgnitionAssets.forEach((a) => dataSource.entities.remove(a));
    curIgnitionAssets = [];
    odinCesium.requestRender();
}

function clearFixed() {
    code.setCode(fixedSrc, "");
}

function createFixedSrc() {
    let ws = ui.getSelectedChoiceValue("elmfire.fixedWs");
    let wd = ui.getSelectedChoiceValue("elmfire.fixedWd");
    let t2 = ui.getSelectedChoiceValue("elmfire.t2");
    let tcc = ui.getSelectedChoiceValue("elmfire.tcc");

    let dfm1 = ui.getSelectedChoiceValue("elmfire.fixedM1");
    let dfm10 = ui.getSelectedChoiceValue("elmfire.fixedM10");
    let dfm100 = ui.getSelectedChoiceValue("elmfire.fixedM100");
    let lfms = config.lfmMap.get(ui.getSelectedChoiceValue("elmfire.lfMoisture"));

    let src =
`! fixed Elmfire weather and fuel moisture input
ws   = ${ws.toFixed(0).padStart(4, ' ')}  ! wind speed [mph]
wd   = ${wd.toFixed(0).padStart(4, ' ')}  ! wind direction [°]

m1   = ${dfm1.toFixed(0).padStart(4, ' ')}  ! 1h dead fuel moisture [%]
m10  = ${dfm10.toFixed(0).padStart(4, ' ')}  ! 10h dead fuel moisture [%]
m100 = ${dfm100.toFixed(0).padStart(4, ' ')}  ! 100h dead fuel moisture [%]

lh   = ${lfms[0].toFixed(0).padStart(4, ' ')}  ! live herbaceous fuel moisture [%]
lw   = ${lfms[1].toFixed(0).padStart(4, ' ')}  ! live woody fuel moisture [%]

t2   = ${t2.toFixed(0).padStart(4, ' ')}  ! temperature at 2m [F]
tcc  = ${tcc.toFixed(0).padStart(4, ' ')}  ! total cloud covarage [%]
`;

    code.setCode(fixedSrc, src);
}

function saveSpotting() {
    // TBD
}

function clearSpotting() {
    code.setCode(spottingSrc, "");
}

function pickBarrierLine() {
    // TBD
}

function clearBarriers() {
    code.setCode(barrierSrc, "");
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

function pickNow() {
    addIgnitionPoint();
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
        odinCesium.addDataSource(ds);
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

    let ignitions = code.getCode(ignitionSrc);
    if (ignitions.length == 0) {
        alert("no ignitions");
        return;
    } else if (!checkIgnitions()) {
        return;
    }

    let computeWind = ui.isCheckBoxSelected("elmfire.computeWind");

    let elmfire = { id, name, nHours, size, date, pos, computeWind, ignitions };

    let fixed = code.getCode(fixedSrc);
    if (fixed.length > 0) {
        // TODO - should do some sanitizing here
        elmfire.fixed = fixed;
    }

    let spotting = code.getCode(spottingSrc);
    if (spotting.length > 0) {
        elmfire.spotting = spotting;
    }

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

    if (prevEntry) prevEntry.updateDisplay(); // removes its fill (and in showSelectedEntryMode its outlines)
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
    if (entry) updateInput(); // don't clear input when de-selecting (user might be entering a new run)
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

        setIgnitionCode( ef.ignitions);
        code.setCode(fixedSrc, ef.fixed);
        code.setCode(spottingSrc, ef.spotting);
        code.setCode(barrierSrc, ef.barriers);

    } else {
        ui.clearField(nameEntry);
        setLocation( null);
        ui.clearDTGEntry(dtgEntry);
        ui.setCheckBox("elmfire.computeWind", false);

        clearIgnitions();
        code.clearCode(fixedSrc);
        code.clearCode(spottingSrc);
        code.clearCode(barrierSrc);
    }
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
    ui.setListItems(spottingListView, spottingList);
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

//--- NML parsing

const RE_VAR = / *([a-zA-Z0-9_]+)(?:\((\d+)\))?[ \t]*=[ \t]*([0-9. \t,+-eE]+)(?: *!.*)?/;
const RE_IGNORE = /^ *(?:!.*)?$/;
const RE_T_IGN = /T_IGN\(\d+\) *=/;
const RE_NUM_IGN = /^ *NUM_IGNITIONS *= *(\d)+(?: *!.*)?$/;

class PointIgnition {
    constructor(geoPoint, t) {
        this.geoPoint = geoPoint;
        this.t = t;
    }
}

class LineIgnition {
    constructor(geoStart, geoEnd, t) {
        this.geoStart = geoStart;
        this.geoEnd = geoEnd;
        this.t = t;
    }
}

class Ignitions {
    constructor() {
        this.pointIgnitions = [];
        this.lineIgnitions = [];
    }
}

/// parse valid content for SIMULATOR section
function parseIgnitions(src, geoCenter) {
    let ignitions = new Ignitions();
    let xIgn = [], yIgn = [], tIgn = [];
    let numIgn = 0;
    let xStart = [], yStart = [], xEnd = [], yEnd = [], tLine = [];

    let s = src.trim();
    if (s.length == 0) return ignitions;

    for (const line of s.split('\n')) {
        if (!line || line.length == 0 || RE_IGNORE.test(line)) continue;

        let groups = line.match(RE_NUM_IGN);
        if (groups) {
            numIgn =groups[1];
            continue;
        }

        groups = line.match(RE_VAR);
        if (groups) {
            let varName = groups[1];
            let idx = groups[2];
            let vs = groups[3].split(',');
            if (vs.length == 1 && idx >= 1) {
                idx--; // Namelist indices are 1-based
                let value = Number.parseFloat(vs[0].trim());
                switch (varName) {
                    case "X_IGN": xIgn[idx] = value; break;
                    case "Y_IGN": yIgn[idx] = value; break;
                    case "T_IGN": tIgn[idx] = value; break;
                    case "NUM_IGNITIONS": numIgn = value; break;
                    case "X_START": xStart[idx] = value; break;
                    case "Y_START": yStart[idx] = value; break;
                    case "X_END": xEnd[idx] = value; break;
                    case "Y_END": yEnd[idx] = value; break;
                    case "T_LINE_IGN": tLine[idx] = value; break;

                    default: throw "unknown input variable: " + varName;
                }
            } else throw "invalid value (expected one number): " + groups[3];

            continue;
        }

        throw "malformed input line: " + line;
    }

    //--- sanity checks
    xIgn = util.removeUndefinedItems(xIgn);
    yIgn = util.removeUndefinedItems(yIgn);
    tIgn = util.removeUndefinedItems(tIgn);
    if ((xIgn.length != yIgn.length) || (xIgn.length != tIgn.length)) throw "inconsistent point ignition lengths";

    xStart = util.removeUndefinedItems(xStart);
    yStart = util.removeUndefinedItems(yStart);
    xEnd = util.removeUndefinedItems(xEnd);
    yEnd = util.removeUndefinedItems(yEnd);
    tLine = util.removeUndefinedItems(tLine);
    if ((xStart.length != yStart.length) || (xStart.length != tLine.length) || (xStart.length != xEnd.length) || (xStart.length != yEnd.length)) {
        throw "inconsistent line ignition lengths";
    }

    let utmCenter = util.latLon2Utm(geoCenter.lat, geoCenter.lon);

    if (xIgn.length > 0) {
        ignitions.pointIgnitions = getPointIgnitions( xIgn, yIgn, tIgn, utmCenter.utmZone, utmCenter.band);
    }
    if (xStart.length > 0) {
        ignitions.lineIgnitions = getLineIgnitions(xStart, yStart, xEnd, yEnd, tLine, utmZone, utmBand);
    }

    return ignitions;
}

function getPointIgnitions(xIgn, yIgn, tIgn, utmZone, utmBand) {
    let igns = [];
    for (let i = 0; i < xIgn.length; i++) {
        let geoPt = util.utmToLatLon( xIgn[i], yIgn[i], utmZone, utmBand);
        igns.push( new PointIgnition(geoPt, tIgn[i]));
    }
    return igns;
}

function getLineIgnitions(xStart, yStart, xEnd, yEnd, tLine, utmZone, utmBand) {
    let igns = [];
    for (let i = 0; i < xStart.length; i++) {
        let geoStart = util.utmToLatLon(xStart[i], yStart[i], utmZone, utmBand);
        let geoEnd = util.utmToLatLon(xEnd[i], yEnd[i], utmZone, utmBand);
        igns.push( new LineIgnition( geoStart, geoEnd, tLine[i]));
    }
    return igns;
}

function addPointIgnitionSource(loc, t) {
    let src = code.getCode(ignitionSrc);
    let n = (src.match(RE_T_IGN) || []).length + 1;

    if (n > 1) {
        src = src.replace(RE_NUM_IGN, `NUM_IGNITIONS = ${n}`);

    } else {
        if (src.length == 0) {
            src =
`! Elmfire ignition specification
NUM_IGNITIONS = 1
`;
        } else {
            src += `
NUM_IGNITIONS = 1
`;
        }
    }
    let utm = util.latLon2Utm(loc.lat, loc.lon);
    src += `
T_IGN(${n}) = ${t.toString().padStart(8, ' ')}  ! secs from base hour (${Math.round(t/60)} min)
X_IGN(${n}) = ${utm.easting.toFixed(0).padStart(8, ' ')}  ! UTM easting (zone ${utm.utmZone}) ${loc.lon.toFixed(4)}°
Y_IGN(${n}) = ${utm.northing.toFixed(0).padStart(8, ' ')}  ! UTM northing (band ${utm.band}) ${loc.lat.toFixed(4)}°
`;

    return src;
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

function createIgnitionEntities(igns) {
    let entities = [];

    let pis = igns.pointIgnitions;
    for (let i = 0; i < pis.length; i++) {
        let pi = pis[i];
        let pos = pi.geoPoint;

        let entity = new Cesium.Entity({
            position: Cesium.Cartesian3.fromDegrees(pos.lon, pos.lat),
            // TODO - should we add a label here?
            point: {
                pixelSize: config.ignPointSize,
                color: config.ignPointColor,
                outlineColor: config.ignPointOutlineColor,
                outlineWidth: config.ignPointOutlineWidth,
                heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
                //disableDepthTestDistance:Number.MAX_SAFE_INTEGER,
                distanceDisplayCondition: config.ignPointDC,
            }
        });

        let loc = ui.getLocation(locEntry);
        if (loc && (Math.abs(loc.lat - pos.lat) > 0.001 || Math.abs(loc.lon - pos.lon) > 0.001)) {
            entity.label = {
                text: (i + 1).toString(),
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
                disableDepthTestDistance:Number.MAX_SAFE_INTEGER,
            }
        }

        entities.push(entity);
    }

    return entities;
}
