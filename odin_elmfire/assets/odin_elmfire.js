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

const MOD_PATH = "odin_elmfire::service::ElmfireService";
const DEBOUNCE_SECS = 3;

/* #region ElmfireEntry ************************************************************************************/

class FireEntry {

    constructor (ef) {
        this.id = ef.id;
        this.ef = ef;

        //--- display assets
        //this.symEntity = this.createSymEntity(); // always on
        this.toaDs = undefined; // on demand time-of-arrival image
        this.flinEntity = undefined; // on demand fireline intensity image
        this.vsEntity = undefined; // on demand spread rate image
        this.cfEntity = undefined; // on demand crown fire image
        this.flEntity = undefined; // on demand flame length image
    }

    createSymEntity() {
        let entity = createSymEntity(this.id, this.pos);
        entity._uiElmFire = this; // backlink for selection

        return entity
    }

    state() {
        return (this.ef.output) ? "✓" : "⧗";
    }

    hasRasterEntities() {
        return (this.flinEntity || this.vsEntity || this.cfEntity || this.flEntity);
    }

    cleanUp() {
        let ec = dataSource.entities;

        //ec.remove(this.symEntity);
        if (this.flinEntity) ec.remove(this.flinEntity);
        if (this.vsEntity) ec.remove(this.vsEntity);
        if (this.flEntity) ec.remove(this.flEntity);
        if (this.cfEntity) ec.remove(this.cfEntity);

        if (this.toaDs) odinCesium.removeDataSource(this.toaDs);
    }

    // note - this should be kept in sync with odin_elmfire::ElmFire::get_id
    static getId(name, date, position) {
        return `${name}__${util.toYYYYMMDDhhmmZString(date,true,true)}__${main.GeoPoint.encode(position)}`;
    }
}

class Ignition {
    constructor(date, lon, lat) {
        this.date = date;
        this.lon = lon;
        this.lat = lat;
    }
}

/* #endregion ElmfireEntry */

var toaRender = { ...config.toaRender };
var flinRender = { ...config.flinRender };
var vsRender = { ...config.vsRender };
var flRender = { ...config.flRender };
var cfRender = { ...config.cfRender };

var fireView = undefined;
var areaView = undefined;
var nameEntry = undefined;
var locEntry = undefined;
var dtgEntry = undefined;
var ignitionSrc = undefined;
var fixedSrc = undefined;
var spottingSrc = undefined;
var barrierSrc = undefined;

var statsView = undefined;

var fireEntries = new Map();  // id -> FireEntry
var selectedEntry = undefined;

var curCenterAsset = undefined;
var curIgnitionAssets = [];

var ignitions = [];

var spottingList = [];

createIcon();
createWindow();

initFireView();
initAreaView();

setIncidentChoices();
setFixedWxChoices();
setBarrierChoices();

setInputDebouncers();

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

function updateElmfires(elmFires,sel=null) {
    for (let ef of elmFires) {
        let oldEntry = fireEntries.get(ef.id);
        if (oldEntry) {
            oldEntry.cleanUp();
        }

        let e = new FireEntry(ef);
        fireEntries.set(e.id, e);

        if (sel && ef === sel) {
            selectedEntry = e; // will be selected when we call updateFireView()
        }
    }

    updateFireView();
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
    let btnWidth = "3rem";

    let view = ui.Window("elmfire", "elmfire", "./asset/odin_elmfire/fires-icon.svg")(
        ui.LayerPanel("elmfire", toggleShowFires),
        ui.Panel("simulations", true)(
            (fireView = ui.List("elmfire.fires", 5, selectEntry, null, null, zoomToSelection)),
            ui.RowContainer("end")(
                (statsView = ui.KvTable("elmfire.stats", 8, "21.2rem", "21.2rem")),
                (areaView = ui.List("elmfire.area", 8))
            ),
            ui.VarText(null, "elmfire.error", 0, "100%")
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
                ui.Choice("size [km]", "elmfire.size", null, "3rem"),
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
                        ui.Button("clear", clearFixed),
                        ui.HorizontalSpacer(1.0),
                        ui.Choice("dir", "elmfire.fixedDir", null, "4rem"),
                        ui.Choice("spd", "elmfire.fixedSpd", null, "3rem"),
                        ui.Choice("dfm", "elmfire.dfMoisture", null, "4rem"),
                        ui.Choice("lfm", "elmfire.lfMoisture", null, "4rem"),
                        ui.Button("create", createFixedSrc)
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
        ui.Panel("display parameters", false)(
            ui.initSlider("TOA stroke width", "fires.toa.stroke_width", toaStrokeWidthChanged, 1,3,1, util.f_0, 1),
            ui.ColorField("TOA stroke color", "fires.toa.stroke_color", true, toaStrokeColorChanged),
            ui.initSlider("TOA dim factor", "fires.toa.dim_factor", toaDimFactorChanged, 0,1,0.1, util.f_1, 1)
        )
    );

    ignitionSrc.placeholder = "Elmfire ignition input (NML format)";
    fixedSrc.placeholder = "Elmfire fixed wind and fuel moisture input (NML format)";
    spottingSrc.placeholder = "Elmfire spotting input (NML format)";
    barrierSrc.placeholder = "Elmfire barrier input (NML format)";

    return view;
}

//--- listview initializations

function initFireView() {
    ui.setListItemDisplayColumns(fireView, ["fit", "header"], [
        { name: "", tip: "processing state", width: "1.5rem", attrs: [], map: e => e.state() },
        { name: "name", tip: "fire name", width: "10rem", attrs: [], map: e => util.maxString(e.ef.name, 20) },
        { name: "date", tip: "local date of update", width: "10rem", attrs:["fixed", "alignRight"], map: e=> util.toLocalDateHMTimeString(e.ef.date) },
        ui.listItemSpacerColumn(2),
        { name: "toa", tip: "show time of arrival", width: "2rem", attrs:[], map: e => fileCb(e, "toa.geojson", e.toaDs, toggleShowToa)},
        { name: "flin", tip: "show fireline intensity", width: "2rem", attrs:[], map: e => fileCb(e, "flin.png", e.flinEntity, toggleShowFlin)},
        { name: "vs", tip: "show spread rate", width: "2rem", attrs:[], map: e => fileCb(e, "vs.png", e.vsEntity, toggleShowVs)},
        { name: "cf", tip: "show crown fire", width: "2rem", attrs:[], map: e => fileCb(e, "cf.png", e.cfEntity, toggleShowCf)},
        { name: "fl", tip: "show flame length", width: "2rem", attrs:[], map: e => fileCb(e, "fl.png", e.flEntity, toggleShowFl) },
        { name: "wnd", tip: "show wind", width: "2rem", attrs:[], map: e => "-"}
    ]);
}

function fileCb( e, fname, isShowing, toggleShow) {
    return (e.ef.output && e.ef.output.files.includes(fname)) ? ui.createCheckBox(isShowing, toggleShow) : "-";
}

function initAreaView() {
    ui.setListItemDisplayColumns(areaView, ["fit", "header"], [
        { name: "time", tip: "", width: "4rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalHMTimeString(e.date) },
        { name: "dt", tip: "hours from start", width: "3rem", attrs: ["fixed", "alignRight"], map: e=> areaItemDt(e) },
        { name: "area", tip: "area [ac]", width:  "7rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.area,0)}
    ]);
}

function areaItemDt(e) {
    if (selectedEntry && selectedEntry.ef) {
        return util.formatFloat((e.date - selectedEntry.ef.date) / 3600000.0, 1);
    } else {
        return ""
    }
}

function setIncidentChoices() {
    // this is in [km]
    ui.setChoiceItems("elmfire.nHours", [3, 4, 5, 6, 7, 8], 1);
    ui.setChoiceItems("elmfire.size", [24, 30, 42, 60], 0);
}

function setFixedWxChoices() {
    // initialize from config so that we can override based on configured location
    ui.setChoiceItems("elmfire.fixedDir", config.fixedDir, config.defaultDir);
    ui.setChoiceItems("elmfire.fixedSpd", config.fixedSpd, config.defaultSpd);
    ui.setChoiceItems("elmfire.dfMoisture", [...config.dfmMap.keys()], config.defaultDfm);
    ui.setChoiceItems("elmfire.lfMoisture", [...config.lfmMap.keys()], config.defaultLfm);
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
    ui.setFieldValue(nameEntry, name);
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
    let nHours = ui.getSelectedChoiceValue("elmfire.nHours");
    let dir = ui.getSelectedChoiceValue("elmfire.fixedDir");
    let spd = ui.getSelectedChoiceValue("elmfire.fixedSpd");
    let dfms = config.dfmMap.get(ui.getSelectedChoiceValue("elmfire.dfMoisture"));
    let lfms = config.lfmMap.get(ui.getSelectedChoiceValue("elmfire.lfMoisture"));

    let src = `! fixed Elmfire weather and fuel moisture input
ws   = ${spd.toFixed(0).padStart(4, ' ')}  ! wind speed [mph]
wd   = ${dir.toFixed(0).padStart(4, ' ')}  ! wind direction [°]

m1   = ${dfms[0].toFixed(0).padStart(4, ' ')}  ! 1h dead fuel moisture [%]
m10  = ${dfms[1].toFixed(0).padStart(4, ' ')}  ! 10h dead fuel moisture [%]
m100 = ${dfms[2].toFixed(0).padStart(4, ' ')}  ! 100h dead fuel moisture [%]

lh   = ${lfms[0].toFixed(0).padStart(4, ' ')}  ! live herbaceous fuel moisture [%]
lw   = ${lfms[1].toFixed(0).padStart(4, ' ')}  ! live woody fuel moisture [%]
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

function toggleShowToa(event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let e = ui.getListItemOfElement(cb);
        if (ui.isCheckBoxSelected(cb)) {
            if (!e.toaDs) {
                loadToa(e); // async
            }
        } else {
            if (e.toaDs) {
                odinCesium.removeDataSource( e.toaDs);
                e.toaDs = null;
                odinCesium.requestRender();
            }
        }
    }
}

async function loadToa(e) {
    let ef = e.ef;
    let url = `elmfire-data/${ef.id}/outputs/toa.geojson`;

    fetch(url).then( (response) => {
        if (response.ok) {
            let data = response.json();
            if (data) {
                Cesium.GeoJsonDataSource.load(data, toaRender).then( (ds) => {
                    setToaRenderOpts(e, ds);

                    e.toaDs = ds;
                    ds.show = true;
                    odinCesium.addDataSource(ds);

                    console.log("loaded ", url);
                });
            } else console.log("no data for TOA request: ", url);
        } else console.log("request failed: ", url);
    }, (reason) => console.log("failed to retrieve: ", url, ", reason: ", reason));
}

function setToaRenderOpts(fireEntry, ds) {
    for (const e of ds.entities.values) {
        if (e.polygon) {
            if (!e.polyline) {
                e.polyline = {
                    positions: e.polygon.hierarchy._value.positions,
                    material: toaRender.stroke,
                    width: toaRender.strokeWidth,
                    clampToGround: true
                };
                e.addProperty('polyline');
                e.polygon.outline = false;

                if (fireEntry.hasRasterEntities()) { // or any other image we have to see through
                    e.polygon.fill = false;
                }
                //e.polygon.fill = true;
                //e.polygon.material = toaRender.fill;
            }
        }
    }
}

function toggleShowFlin(e) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let e = ui.getListItemOfElement(cb);
        if (ui.isCheckBoxSelected(cb)) {
            if (!e.flinEntity) {
                e.flinEntity = createRasterEntity( `elmfire-data/${e.ef.id}/outputs/flin.png`, e.ef.output.bbox, flinRender);
                dataSource.entities.add(e.flinEntity);
                dataSource.show = true;
                odinCesium.requestRender();
            }
        } else {
            if (e.flinEntity) {
                dataSource.entities.remove(e.flinEntity);
                e.flinEntity = null;
                odinCesium.requestRender();
            }
        }
    }
}

function toggleShowVs(e) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let e = ui.getListItemOfElement(cb);
        if (ui.isCheckBoxSelected(cb)) {
            if (!e.vsEntity) {
                e.vsEntity = createRasterEntity( `elmfire-data/${e.ef.id}/outputs/vs.png`, e.ef.output.bbox, vsRender);
                dataSource.entities.add(e.vsEntity);
                dataSource.show = true;
                odinCesium.requestRender();
            }
        } else {
            if (e.vsEntity) {
                dataSource.entities.remove(e.vsEntity);
                e.vsEntity = null;
                odinCesium.requestRender();
            }
        }
    }
}

function toggleShowCf(e) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let e = ui.getListItemOfElement(cb);
        if (ui.isCheckBoxSelected(cb)) {
            if (!e.cfEntity) {
                e.cfEntity = createRasterEntity( `elmfire-data/${e.ef.id}/outputs/cf.png`, e.ef.output.bbox, cfRender);
                dataSource.entities.add(e.cfEntity);
                dataSource.show = true;
                odinCesium.requestRender();
            }
        } else {
            if (e.cfEntity) {
                dataSource.entities.remove(e.cfEntity);
                e.cfEntity = null;
                odinCesium.requestRender();
            }
        }
    }
}

function toggleShowFl(e) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let e = ui.getListItemOfElement(cb);
        if (ui.isCheckBoxSelected(cb)) {
            if (!e.cfEntity) {
                e.flEntity = createRasterEntity( `elmfire-data/${e.ef.id}/outputs/fl.png`, e.ef.output.bbox, flRender);
                dataSource.entities.add(e.flEntity);
                dataSource.show = true;
                odinCesium.requestRender();
            }
        } else {
            if (e.flEntity) {
                dataSource.entities.remove(e.flEntity);
                e.flEntity = null;
                odinCesium.requestRender();
            }
        }
    }
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

function toggleShowFm(e) { }
function toggleShowWind(e) { }

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

    let selEntry = selectedEntry;
    ui.setListItems(fireView, entries);
    if (selEntry) ui.setSelectedListItem(fireView, selEntry);
}

function zoomToSelection(event) {
    let sel = ui.getSelectedListItem(event);
    if (sel) {
        odinCesium.zoomTo(Cesium.Cartesian3.fromDegrees(sel.lon, sel.lat, config.zoomHeight));
    }
}

function selectEntry(e) {
    selectedEntry = ui.getSelectedListItem(fireView);
    updateStatsView();
    updateAreaView();
    updateErrorView();
    updateInput();
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
        ui.setListItems(areaView, e.ef.output.area);
    } else {
        ui.clearList(areaView);
    }
}

function updateErrorView() {
    let e = selectedEntry;
    if (e) {
        showError( e.ef.error);
    }
}

function updateInput() {
    let e = selectedEntry;
    if (e) {
        let ef = e.ef;
        ui.setField(nameEntry, ef.name);
        ui.setLocation(locEntry, ef.pos);
        ui.setDTGEntryEpochMillis(dtgEntry, ef.date);
        ui.setCheckBox("elmfire.computeWind", ef.computeWind);

        code.setCode(ignitionSrc, ef.ignitions);
        code.setCode(fixedSrc, ef.fixed);
        code.setCode(spottingSrc, ef.spotting);
        code.setCode(barrierSrc, ef.barriers);

    } else {
        ui.clearField(nameEntry);
        ui.clearLocation(locEntry);
        ui.clearDTGEntry(dtgEntry);
        ui.setCheckBox("elmfire.computeWind", false);

        code.clearCode(ignitionSrc);
        code.clearCode(fixedSrc);
        code.clearCode(spottingSrc);
        code.clearCode(barrierSrc);
    }
}

function getKvList(ef) {
    let stats = ef.output.stats[0];
    let compStats = ef.output.compStats;
    return [
        ["date", util.toLocalDateHMTimeString(stats.date)],
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

function toggleShowFires(event) {
    let showIt = ui.isCheckBoxSelected(event.target);
    dataSource.show = showIt;
    showToa(showIt);
    odinCesium.requestRender();
}

function showFires(cond) {
    dataSource.show = cond;
    showToa(cond);
    odinCesium.requestRender();
}

function showToa(cond) {
    elmfires.forEach((e, id, map) => {
        e.forEach((fire) => {
            if (fire.toaDs) {
                fire.toaDs.show = cond;
            }
        })
    });
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


function selectFixed(e) {
    let name = ui.getSelectedListItem(fixedListView);
    if (name) {
        loadSpec("fixed", name, fixedSrc);
        ui.setField("elmfire.fixedName", name);
    }
}

function storeFixed() {
    storeSpec(fixedSrc, "elmfire.fixedgName", "storeFixed");
}

function delFixed() {
    delSpec("elmfire.fixedName", "delFixed");
}

function updateFixedView() {
    ui.setListItems(fixedListView, fixedList);
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

function toaStrokeWidthChanged(event) {
    toaRender.strokeWidth = ui.getSliderValue(event.target);
    updateToaRendering();
}

function toaStrokeColorChanged(event) {
    let clrSpec = event.target.value;
    if (clrSpec) {
        toaRender.strokeColor = Cesium.Color.fromCssColorString(clrSpec);
        updateToaRendering();
    }
}

function toaFillOpacityChanged(event) {
    toaRender.fillOpacity = ui.getSliderValue(event.target);
    updateToaRendering();
}

function toaFillColorChanged(event) {
    let clrSpec = event.target.value;
    if (clrSpec) {
        toaRender.fillColor = Cesium.Color.fromCssColorString(clrSpec);
        updateToaRendering();
    }
}

function toaDimFactorChanged(event) {
    toaRender.dimFactor = ui.getSliderValue(event.target);
    updateToaRendering();
}

function updateToaRendering() {
    if (selectedEntry) {
        let dimFactor = 1.0;

        let ds = selectedEntry.toaDs;
        if (ds && ds.show) {
            let renderOpts = getToaRenderOptions(dimFactor);
            ds.entities.values.forEach( e => {
                if (e.polygon) {
                    e.polygon.material = renderOpts.fill;
                    //e.polygon.outline = true;
                    //e.polygon.outlineWidth = renderOpts.strokeWidth;
                    //e.polygon.outlineColor = renderOpts.stroke;
                }
                if (e.polyline) {
                    e.polyline.width = renderOpts.strokeWidth;
                    e.polyline.material = renderOpts.stroke;
                }
            });

            dimFactor *= toaRender.dimFactor;
        }

        odinCesium.requestRender();
    }
}

function getToaRenderOptions(dimFactor=1.0) {
    return {
        fill: toaRender.fillColor.withAlpha( toaRender.fillOpacity * dimFactor),
        stroke: toaRender.strokeColor.withAlpha(dimFactor),
        strokeWidth: toaRender.strokeWidth
    };
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
            src = `! Elmfire ignition specification
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
