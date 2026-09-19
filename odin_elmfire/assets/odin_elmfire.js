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

/* #region ElmfireEntry ************************************************************************************/

class FireEntry {

    constructor (ef) {
        this.id = ef.id;
        this.ef = ef;

        //--- display assets
        this.symEntity = this.createSymEntity(); // always on
        this.toaDs = undefined; // on demand time-of-arrival image
        this.flinEntity = undefined; // on demand fireline intensity image
        this.vsEntity = undefined; // on demand spread rate image
        this.cfEntity = undefined; // on demand crown fire image
        this.flEntity = undefined; // on demand flame length image
    }

    createSymEntity() {
        let ef = this.ef;

        let entity = new Cesium.Entity({
            id: this.id,
            position: Cesium.Cartesian3.fromDegrees( ef.pos.lon, ef.pos.lat),
            billboard: {
                image: "./asset/odin_elmfire/fire.png",
                distanceDisplayCondition: config.billboardDC,
                color: config.billboardColor,
                heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
                disableDepthTestDistance: Number.MAX_SAFE_INTEGER,  // otherwise symbol might get clipped by terrain
                alignedAxis: Cesium.Cartesian3.UNIT_Z,
            },
            label: {
                text: ef.name,
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

        ec.remove(this.symEntity);
        if (this.flinEntity) ec.remove(this.flinEntity);
        if (this.vsEntity) ec.remove(this.vsEntity);
        if (this.flEntity) ec.remove(this.flEntity);
        if (this.cfEntity) ec.remove(this.cfEntity);

        if (this.toaDs) odinCesium.removeDataSource(this.toaDs);
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
var ignitionView = undefined;
var fixedListView = undefined;
var fixedSrc = undefined;
var spottingListView = undefined;
var spottingSrc = undefined;
var barrierSrc = undefined;

var nameEntry = undefined;
var nHoursEntry = undefined;
var sizeEntry = undefined; // for simulation area
var barrierWidthEntry = undefined;
var statsView = undefined;

var fireEntries = new Map();  // id -> FireEntry
var selectedEntry = undefined;

var ignitions = [];

var fixedList = [];
var spottingList = [];

createIcon();
createWindow();

initFireView();
initAreaView();
initIgnitionView();

initFixedListView();
initSpottingListView();

ui.setChoiceItems(nHoursEntry, [3, 4, 5, 6, 7, 8], 1);
ui.setChoiceItems(sizeEntry, [24, 30, 42, 60], 0);
ui.setChoiceItems(barrierWidthEntry, [10, 20, 30, 40, 50, 60], 2);

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
        case "fixed":
            handleFixedMessage(msg);
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
    for (let ef of elmFires) {
        let oldEntry = fireEntries.get(ef.id);
        if (oldEntry) {
            oldEntry.cleanUp();
        }

        let e = new FireEntry(ef);
        fireEntries.set(e.id, e);
        dataSource.entities.add(e.symEntity);
    }

    updateFireView();
}

function handleSpottingMessage(newSpottingList) {
    spottingList = newSpottingList;
    updateSpottingView();
}

function handleFixedMessage(newFixedList) {
    fixedList = newFixedList;
    updateFixedView();
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
                (statsView = ui.KvTable("elmfire.stats", 8, "21rem", "21rem")),
                (areaView = ui.List("elmfire.area", 8))
            )
        ),
        ui.Panel("new simulation", false)(
            ui.RowContainer()(
                (nameEntry = ui.TextInput("name", "elmfire.name", "11rem", { placeHolder: "event name" })),
                ui.HorizontalSpacer(2),
                ui.Button("run", sendRequest, "4rem")
            ),
            ui.RowContainer()(
                (nHoursEntry = ui.Choice("hours", "elmfire.nHours", null, "3rem")),
                ui.HorizontalSpacer(1),
                (sizeEntry = ui.Choice("size [mi]", "elmfire.size", null, "3rem")),
                ui.HorizontalSpacer(1),
                ui.CheckBox( "comp wind", null, "elmfire.computeWind")
            ),

            ui.RowContainer("end")(
                (ignitionView = ui.List("elmfire.ignitions", 4)),
                ui.Button("del", delIgnition, "4rem"),
                ui.Button("new..", newIgnition, "4rem")
            ),

            ui.VerticalSpacer(0.3),
            ui.TabbedContainer()(
                ui.Tab("fixed wx", true)(
                    ui.RowContainer("start")(
                        (fixedSrc = code.CodeInput("elmfire.fixedSrc", "nml-highlight", "fortran-namelist", "29rem", "11lh")),
                        //(fixedSrc = ui.TextArea("elmfire.fixedSrc", "38rem", "8lh", { isFixed: true })),
                        ui.ColumnContainer()(
                            ui.CheckBox("use fixed wx", null, "elmfire.useFixed"),
                            ui.TextInput(null, "elmfire.fixedName", "8rem"),
                            ui.RowContainer()(
                                ui.Button("del", delFixed, "4rem"),
                                ui.Button("save", storeFixed, "4rem")
                            ),
                            (fixedListView = ui.List("elmfire.fixedSets", 4, selectFixed))
                        )
                    )
                ),
                ui.Tab("spotting", false)(
                    ui.RowContainer("start")(
                        (spottingSrc = code.CodeInput("elmfire.spottingSrc", "nml-highlight", "fortran-namelist", "29rem", "11lh")),
                        ui.ColumnContainer()(
                            ui.CheckBox("use spotting", null, "elmfire.useSpotting"),
                            ui.TextInput(null, "elmfire.spottingName", "8rem"),
                            ui.RowContainer()(
                                ui.Button("del", delSpotting, "4rem"),
                                ui.Button("save", storeSpotting, "4rem")
                            ),
                            (spottingListView = ui.List("elmfire.spottingSets", 4, selectSpotting))
                        )
                    )
                ),
                ui.Tab("barrier", false)(
                    ui.RowContainer("start")(
                        (barrierSrc = code.CodeInput("elmfire.barrierSrc", "wkt-highlight", "WKT", "29rem", "11lh")),
                        ui.ColumnContainer()(
                            ui.CheckBox("use barrier", null, "elmfire.useBarrier"),
                            (barrierWidthEntry = ui.Choice("width", "elmfire.barrier_width", null, "3rem")),
                            ui.Button("edit..", editBarrier, "4rem")
                        )
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

function initIgnitionView() {
    ui.setListItemDisplayColumns(ignitionView, ["fit", "header"], [
        { name: "lon", tip: "longitude [deg]", width: "7rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.lon, 4), edit: editLon },
        { name: "lat", tip: "latitude [deg]", width: "6rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.lat, 4), edit: editLat },
        { name: "date", tip: "local date of ignition", width: "11rem", attrs:["fixed", "alignRight"], map: e=> util.toLocalDateHMTimeString(e.date), edit: editDate }
    ]);
}


function initFixedListView() {
    ui.setListItemDisplayColumns(fixedListView, ["fit"], [
        { name: "", tip: "", width: "7rem", attrs: [], map: e => util.maxString(e, 12) }
    ]);
}

function initSpottingListView() {
    ui.setListItemDisplayColumns(spottingListView, ["fit"], [
        { name: "", tip: "", width: "7rem", attrs: [], map: e => util.maxString(e, 12) }
    ]);
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

function newIgnition() {
    odinCesium.enterGeoPoint((cp) => {
        if (cp) {
            let ign = new Ignition(Date.now(), cp.lon, cp.lat);
            ignitions.push(ign);
            ui.setListItems(ignitionView, ignitions);
        }
    });
}

function delIgnition() {
}


function editDate(event) {
    ui.editListItemField(event.target, (item, newValue) => {
        let newDate = util.parseDtg(newValue);
        if (!isNaN(newDate)) {
            item.date = newDate;
        } else {
            ui.beep();
            alert("invalid date input - enter 'mm/dd/yy HH:MM:SS");
        }
    });
}

function editLon(event) {
    ui.editListItemField(event.target, (item, newValue) => {
        let newVal = util.parseLongitude(newValue);
        if (!isNaN(newVal)) {
            item.lon = newVal;
        } else {
            ui.beep();
            alert("invalid longitude input - enter float value");
        }
    });
}

function editLat(event) {
    ui.editListItemField(event.target, (item, newValue) => {
        let newVal = util.parseLatitude(newValue);
        if (!isNaN(newVal)) {
            item.lat = newVal;
        } else {
            ui.beep();
            alert("invalid latitude input - enter float value");
        }
    });
}

function editBarrier() {
    let src = barrierSrc.value;
    let gls = (src) ? main.GeoLineString.fromWkt(src) : null;

    edit.editGeoLineString( gls, (gls) => {
        if (gls) {
            let wkt = gls.toWkt();
            if (wkt) {
                barrierSrc.value = wkt;
            }
        }
    });
}

function sendRequest() {
    let name = ui.getFieldValue("elmfire.name");
    let nHours = ui.getSelectedChoiceValue("elmfire.nHours");
    let size = ui.getSelectedChoiceValue("elmfire.size");
    let useFixed = ui.isCheckBoxSelected("elmfire.useFixed");
    let useSpotting = ui.isCheckBoxSelected("elmfire.useSpotting");
    let computeWind = ui.isCheckBoxSelected("elmfire.computeWind");

    if (name == "") {
        alert("no name input specified");
        return;
    }

    if (ignitions.length == 0) {
        alert("no ignitions specified");
        return;
    }

    let elmfire = { name, nHours, size, ignitions };

    if (computeWind) {
        elmfire.computeWind = computeWind;
    }

    if (useFixed) {
        let fixed = code.getCode(fixedSrc);
        if (fixed.length == 0) {
            alert("no fixed source entered");
            return;
        } else {
            // TODO - should do some sanitizing here
            elmfire.fixed = fixed;
        }
    }

    if (useSpotting) {
        let spotting = code.getCode(spottingSrc);
        if (spotting.length == 0) {
            alert("no spotting source entered");
            return;
        } else {
            // TODO - should do some sanitizing here
            elmfire.spotting = spotting;
        }
    }

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

function selectSpotting(e) {
    let name = ui.getSelectedListItem(spottingListView);
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
