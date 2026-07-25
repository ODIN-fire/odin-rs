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
import * as ws from "../odin_server/ws.js";
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

/* #endregion ElmfireEntry */

var toaRender = { ...config.toaRender };
var flinRender = { ...config.flinRender };
var vsRender = { ...config.vsRender };
var flRender = { ...config.flRender };
var cfRender = { ...config.cfRender };

var fireView = undefined;
var areaView = undefined;
var ignitionView = undefined;
var fixedInputView = undefined;
var spottingView = undefined;
var spottingSrc = undefined;

var nameEntry = undefined;
var nHoursEntry = undefined;
var sizeEntry = undefined;
var statsView = undefined;

var fireEntries = new Map();  // id -> FireEntry
var selectedEntry = undefined;

var fixedInput = [];
var ignitions = [];
var spottings = [];


createIcon();
createWindow();

initFireView();
initAreaView();
initIgnitionView();
initFixedInputView();

ui.setChoiceItems(nHoursEntry, [4, 5, 6, 7, 8], 2);
ui.setChoiceItems(sizeEntry, [30, 42, 60], 0);

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

function handleSpottingMessage(spottingList) {
    spottings = spottingList;
    updateSpottingView();
}

//--- view initialization

function createIcon() {
    return ui.Icon("./asset/odin_elmfire/fires-icon.svg", (e)=> ui.toggleWindow(e,'elmfire'), "elmfire data");
}

function createWindow() {
    let scrollbarWidth = ui.getRootVar("--scrollbar-track-width");
    let containerGap = ui.getRootVar("--container-gap");
    let btnWidth = "3rem";

    let view = ui.Window("elmfire", "elmfire", "./asset/odin_elmfire/fires-icon.svg")(
        ui.LayerPanel("elmfire", toggleShowFires),
        ui.Panel("simulations", true)(
            (fireView = ui.List("elmfire.fires", 5, selectEntry, null, null, zoomToSelection)),
            ui.RowContainer("end")(
                (statsView = ui.KvTable("elmfire.stats", 8, "20rem", "20rem")),
                (areaView = ui.List("elmfire.area", 8))
            )
        ),
        ui.Panel("request", false)(
            ui.RowContainer()(
                (nameEntry = ui.TextInput("name", "elmfire.name", "10rem", { placeHolder: "event name" })),
                ui.HorizontalSpacer(1),
                (nHoursEntry = ui.Choice("forecast [h]", "elmfire.nHours", null, "3rem")),
                ui.HorizontalSpacer(1),
                (sizeEntry = ui.Choice("size [km]", "elmfire.size", null, "3rem")),
                ui.HorizontalSpacer(2),
                ui.Button("send", sendRequest, "4rem")
            )
        ),
        ui.Panel("ignition", false)(
            ui.RowContainer()(
                ui.ColumnContainer("end", null, "ignitions")(
                    ui.RowContainer("end")(
                        (ignitionView = ui.List("elmfire.ignition.list", 3)),
                        ui.HorizontalSpacer(containerGap),
                        ui.Button("del", delIgnition, btnWidth),
                        ui.Button("⍉", clearIgnitions, btnWidth)
                    ),
                    ui.RowContainer()(
                        ui.Button("⨁", pickIgnition, btnWidth, "elmfire.ignition.pick"),
                        ui.TextInput(null, "elmfire.ignition.date", "12rem", { isFixed: true, placeHolder: "yyyy-mm-ddTHH:MM", alignRight: true }),
                        ui.TextInput(null, "elmfire.ignition.lon", "6rem", { isFixed: true, placeHolder: "deg", alignRight: true }),
                        ui.TextInput(null, "elmfire.ignition.lat", "5rem", { isFixed: true, placeHolder: "deg", alignRight: true }),
                        ui.HorizontalSpacer(scrollbarWidth),
                        ui.Button("add", addIgnition, btnWidth),
                        ui.Button("set", setIgnition, btnWidth)
                    )
                )
            )
        ),
        ui.Panel("wx data", false)(
            ui.RowContainer("start")(
                ui.CheckBox("live", null, "elmfire.useLive", true),
                ui.HorizontalSpacer(0.5),
                ui.ColumnContainer("end", null, "fixed input steps")(
                    ui.RowContainer("end")(
                        (fixedInputView = ui.List("elmfire.fixed", 7, selFixed)),
                        ui.HorizontalSpacer(containerGap),
                        ui.Button("del", delFixed, btnWidth),
                        ui.Button("⍉", clearFixed, btnWidth)
                    ),
                    ui.RowContainer()(
                        ui.TextInput(null, "elmfire.fixed.ws", "3.2rem", { isFixed: true, placeHolder: "mph", alignRight: true }),
                        ui.TextInput(null, "elmfire.fixed.wd", "3.2rem", { isFixed: true, placeHolder: "deg", alignRight: true }),
                        ui.TextInput(null, "elmfire.fixed.m1", "3.2rem", { isFixed: true, placeHolder: "%", alignRight: true }),
                        ui.TextInput(null, "elmfire.fixed.m10", "3.2rem", { isFixed: true, placeHolder: "%", alignRight: true }),
                        ui.TextInput(null, "elmfire.fixed.m100", "3.2rem", { isFixed: true, placeHolder: "%", alignRight: true }),
                        ui.TextInput(null, "elmfire.fixed.lh", "3.2rem", { isFixed: true, placeHolder: "%", alignRight: true }),
                        ui.TextInput(null, "elmfire.fixed.lw", "3.2rem", { isFixed: true, placeHolder: "%", alignRight: true }),
                        ui.HorizontalSpacer(scrollbarWidth),
                        ui.Button("add", addFixed, btnWidth),
                        ui.Button("set", setFixed, btnWidth)
                    )
                )
            )
        ),
        ui.Panel("spotting", false, null, ui.CheckBox("", null, "elmfire.useSpotting", false))(
            ui.RowContainer("start")(
                (spottingView = ui.List("elmfire.spotSets", 6, selectSpotting)),
                (spottingSrc = ui.TextArea("elmfire.spotSrc", "30rem", "8lh", { isFixed: true })),
            ),
            ui.RowContainer("center")(
                ui.TextInput("set name", "elmfire.spottingName", "8rem"),
                ui.HorizontalSpacer(3),
                ui.Button("save", storeSpotting, "4rem"),
                ui.Button("del", delSpotting, "4rem")
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
        { name: "#", tip: "", width:  "1.5rem", attrs: ["fixed", "alignRight"], map: e => ignitions.indexOf(e) + 1},
        { name: "date", tip: "local date of update", width: "11.8rem", attrs:["fixed", "alignRight"], map: e=> util.toISOString(e.date, false) },
        { name: "lon", tip: "longitude", width:  "6.2rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.lon,4)},
        { name: "lat", tip: "latitude", width:  "5.2rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.lat,4)},
    ]);
}

function initFixedInputView() {
    ui.setListItemDisplayColumns(fixedInputView, ["fit", "header"], [
        { name: "h", tip: "", width:  "1.5rem", attrs: ["fixed", "alignRight"], map: e => fixedInput.indexOf(e)},
        { name: "ws", tip: "wind speed [mph]", width:  "3.4rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.ws,1)},
        { name: "wd", tip: "wind direction [deg]", width:  "3.4rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.wd,0)},
        { name: "m1", tip: "1h dead fuel moisture [%]", width:  "3.4rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.m1,0)},
        { name: "m10", tip: "10h dead fuel moisture [%]", width:  "3.4rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.m10,0)},
        { name: "m100", tip: "100h dead fuel moisture [%]", width:  "3.4rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.m100,0)},
        { name: "lh", tip: "live herbaceous fuel moisture [%]", width:  "3.4rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.lh,0)},
        { name: "lw", tip: "live woody fuel moisture [%]", width:  "3.4rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.lw,0)}
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
                color: renderOpts.color
            }),
            classificationType: Cesium.ClassificationType.TERRAIN
        }
    });
}

function toggleShowFm(e) { }
function toggleShowWind(e) { }



//--- button click handlers

function addIgnition() {
    let date = util.fromISOString(ui.getFieldValue("elmfire.ignition.date"));
    let lon = ui.getFloatNumFieldValue("elmfire.ignition.lon");
    let lat = ui.getFloatNumFieldValue("elmfire.ignition.lat");

    if (Number.isNaN(date) || Number.isNaN(lon) || Number.isNaN(lat)) {
        alert("all ignition fields need to be specified as valid dates and numbers");
        return;
    }

    ignitions.push({ date, lon, lat });
    ui.setListItems(ignitionView, ignitions);
    ui.selectLastListItem(ignitionView);
}

function setIgnition() { }

function delIgnition() {
    ui.removeListItemFromList(ignitionView, ignitions, false);
}

function clearIgnitions() {
    ignitions = [];
    ui.clearList(ignitionView);
}

function pickIgnition() {
    ui.setButtonActive("elmfire.ignition.pick", true);
    odinCesium.enterGeoPoint( (cp) => {
        if (cp) {
            ui.setField("elmfire.ignition.lon", util.f_4.format(cp.lon));
            ui.setField("elmfire.ignition.lat", util.f_4.format(cp.lat));
            setIgnitionDate();
        }
        ui.setButtonActive("elmfire.ignition.pick", false);
    });
}

function setIgnitionDate() {
    let now = Date.now();
    ui.setField("elmfire.ignition.date", util.toISOString(now, false));
}

function addFixed() {
    let nHours = ui.getSelectedChoiceValue("elmfire.nHours");
    if (fixedInput.length == nHours) {
        alert("can't add more fixed input steps for nHours = ", nHours);
        return;
    }

    let ws   = ui.getFloatNumFieldValue("elmfire.fixed.ws");
    let wd   = ui.getFloatNumFieldValue("elmfire.fixed.wd");
    let m1   = ui.getFloatNumFieldValue("elmfire.fixed.m1");
    let m10  = ui.getFloatNumFieldValue("elmfire.fixed.m10");
    let m100 = ui.getFloatNumFieldValue("elmfire.fixed.m100");
    let lh   = ui.getFloatNumFieldValue("elmfire.fixed.lh");
    let lw   = ui.getFloatNumFieldValue("elmfire.fixed.lw");

    if (fixedInput.length == 0) {
        // those are almost universally true so we use them as default
        if (Number.isNaN(lh)) lh = 30;
        if (Number.isNaN(lw)) lw = 60;

        if (util.anyNaN( ws, wd, m1, m10, m100, lh, lw)) {
            alert("first fixed input step needs to be fully specified");
            return
        }
    } else {   // complete from previous step
        let last = fixedInput[fixedInput.length - 1];
        if (Number.isNaN(ws)) ws = last.ws;
        if (Number.isNaN(wd)) wd = last.wd;
        if (Number.isNaN(m1)) m1 = last.m1;
        if (Number.isNaN(m10)) m10 = last.m10;
        if (Number.isNaN(m100)) m100 = last.m100;
        if (Number.isNaN(lh)) lh = last.lh;
        if (Number.isNaN(lw)) lw = last.lw;
    }

    //--- those we only keep as rounded values
    wd = Math.round(wd);
    m1 = Math.round(m1);
    m10 = Math.round(m10);
    m100 = Math.round(m100);
    lh = Math.round(lh);
    lw = Math.round(lw);

    fixedInput.push({ ws, wd, m1, m10, m100, lh, lw });
    ui.setListItems(fixedInputView, fixedInput);
    ui.selectLastListItem(fixedInputView);

    ui.setCheckBox("elmfire.useLive", false);
}

function delFixed() {
    ui.removeListItemFromList( fixedInputView, fixedInput, false);
}

function clearFixed() {
    fixedInput = [];
    ui.clearList(fixedInputView);
}

function selFixed(e) {
    let selItem = e.detail.curSelection;
    if (selItem) {
        ui.setField("elmfire.fixed.ws",   selItem.ws);
        ui.setField("elmfire.fixed.wd",   selItem.wd);
        ui.setField("elmfire.fixed.m1",   selItem.m1);
        ui.setField("elmfire.fixed.m10",  selItem.m10);
        ui.setField("elmfire.fixed.m100", selItem.m100);
        ui.setField("elmfire.fixed.lh",   selItem.lh);
        ui.setField("elmfire.fixed.lw",   selItem.lw);
    } else {
        ui.setField("elmfire.fixed.ws", null);
        ui.setField("elmfire.fixed.wd", null);
        ui.setField("elmfire.fixed.m1", null);
        ui.setField("elmfire.fixed.m10", null);
        ui.setField("elmfire.fixed.m100", null);
        ui.setField("elmfire.fixed.lh", null);
        ui.setField("elmfire.fixed.lw", null);
    }
}

function setFixed() {
    let selItem = ui.getSelectedListItem(fixedInputView);
    if (selItem) {
        let ws = ui.getFieldValue("elmfire.fixed.ws");
        let wd = ui.getFieldValue("elmfire.fixed.wd");
        let m1 = ui.getFieldValue("elmfire.fixed.m1");
        let m10 = ui.getFieldValue("elmfire.fixed.m10");
        let m100 = ui.getFieldValue("elmfire.fixed.m100");
        let lh = ui.getFieldValue("elmfire.fixed.lh");
        let lw = ui.getFieldValue("elmfire.fixed.lw");

        if (ws != "")   selItem.ws = ws;
        if (wd != "")   selItem.wd = wd;
        if (m1 != "")   selItem.m1 = m1;
        if (m10 != "")  selItem.m10 = m10;
        if (m100 != "") selItem.m100 = m100;
        if (lh != "")   selItem.lh = lh;
        if (lw != "") selItem.lw = lw;

        ui.updateListItem(fixedInputView, selItem);
    }
}

function sendRequest() {
    let name = ui.getFieldValue("elmfire.name");
    let nHours = ui.getSelectedChoiceValue("elmfire.nHours");
    let size = ui.getSelectedChoiceValue("elmfire.size");
    let useLive = ui.isCheckBoxSelected("elmfire.useLive");
    let useSpotting = ui.isCheckBoxSelected("elmfire.useSpotting");

    if (name == "") {
        alert("no name input specified");
        return;
    }

    if (ignitions.length == 0) {
        alert("no ignitions specified");
        return;
    }

    let elmfire = { name, nHours, size, ignitions };

    if (!useLive) {
        if (fixedInput.length == 0) {
            alert("no fixed steps specified\nselect 'use live' or enter steps");
            return;
        } else {
            elmfire.fixedInput = fixedInput;
        }
    }

    if (useSpotting) {
        let spotting = ui.getTextAreaContent(spottingSrc);

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
    return [
        ["date", util.toLocalDateHMTimeString(stats.date)],
        ["t-stop [h]", ef.nHours],
        ["total area [ac]", stats.totalArea],
        ["crown area [ac]", stats.crownArea],
        ["volume [ac-ft]", stats.volume],
        ["HRR peak [MW}", stats.hrrPeak],
        ["PM2.5 release [ug]", stats.pm25],
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

function selectSpotting(e) {
    let name = ui.getSelectedListItem(spottingView);
    if (name) {
        loadSpotting(name);
        ui.setField("elmfire.spottingName", name);
    }
}

async function loadSpotting(name) {
    let url = `elmfire-spotting/${name}`;

    fetch(url).then((response) => {
        if (response.ok) {
            let txt = response.text();
            if (txt) {
                txt.then((src) => {
                    ui.setTextAreaContent(spottingSrc, src);
                });
            }
        } else console.log("request failed: ", url);
    }, (reason) => console.log("failed to retrieve: ", url, ", reason: ", reason));
}

function storeSpotting() {
    let source = ui.getTextAreaContent(spottingSrc);
    if (!source || source.length == 0) {
        alert("no spotting source");
        return;
    }

    let name = ui.getFieldValue("elmfire.spottingName");
    if (!name || name.length == 0) {
        alert("no spotting name");
        return;
    }

    ws.sendWsMessage( MOD_PATH, "storeSpotting", { name, source });
}

function delSpotting() {
    let name = ui.getFieldValue("elmfire.spottingName");
    if (!name || name.length == 0) {
        alert("no spotting name");
        return;
    }

    ws.sendWsMessage( MOD_PATH, "delSpotting", { name });
}

function updateSpottingView() {
    ui.setListItems(spottingView, spottings);
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
