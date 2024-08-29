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

import { config } from "./odin_sentinel_config.js";
import * as util from "../odin_server/ui_util.js";
import { SkipList, CircularBuffer } from "../odin_server/ui_data.js";
import * as ui from "../odin_server/ui.js";
import * as ws from "../odin_server/ws.js";
import * as odinCesium from "../odin_cesium/odin_cesium.js";

const MODULE_PATH = util.asset_path(import.meta.url);

ws.addWsHandler( MODULE_PATH, handleWsMessages);

var sentinelDataSource = new Cesium.CustomDataSource("sentinel");
var sentinelView = undefined;
var sentinelEntries = new Map();
var sentinelList = new SkipList( // id-sorted display list for trackEntryView
    3, // max depth
    (a, b) => a.id < b.id, // sort function
    (a, b) => a.id == b.id // identity function
);

var selectedSentinelEntry = undefined;
var selectedImage = undefined;

var sentinelImageView = undefined;
var sentinelFireView = undefined;
var sentinelSmokeView = undefined;
var sentinelGasView = undefined;
var sentinelThermoView = undefined;
var sentinelAnemoView = undefined;
var sentinelVocView = undefined;
var sentinelAccelView = undefined;
var sentinelGpsView = undefined;
var sentinelOrientationView = undefined;
var sentinelCloudCoverView = undefined;

var maxHistory = config.maxHistory;

let diagnosticCommands = new Map([
    ["trigger alert", triggerAlertCmd], 
    ["turn lights on", turnLightsOnCmd], 
    ["turn lights off", turnLightsOffCmd],

]);

class SentinelAssets {
    constructor(symbol, details) {
        this.symbol = symbol; // billboard
        this.details = details; // gas coverage, camera-coverage, wind
    }

    updatePosition(lat,lon) {
        let pos = Cesium.Cartesian3.fromDegrees(lon, lat);
        if (this.symbol) this.symbol.position = pos;
        if (this.details) this.details.position = pos;
    }

    showAssets (cond) {
        // TODO - is this right? we should just add/remove the sentinelDataSource
        if (this.symbol) this.symbol.show = cond;
        if (this.details) this.details.show = cond;
    }
}

class SentinelEntry {
    constructor(sentinel) {
        this.id = sentinel.deviceId;
        this.displayId = util.maxString(sentinel.deviceId, 4);

        this.sentinel = sentinel;
        this.assets = null;

        this.pos = this.position();
        this.enuToEcef = Cesium.Transforms.eastNorthUpToFixedFrame(this.pos);
    }

    hasFire() {
        let fire = this.sentinel.fire;
        return (fire && fire.length > 0 && fire[0].fire.fireProb > 0.5);
    }

    hasSmoke() {
        let smoke = this.sentinel.smoke;
        (smoke && smoke.length > 0 && smoke[0].smoke.smokeProb > 0.5);
    }

    alertStatus() {
        if (this.hasFire()){
            if (this.hasSmoke()) return ui.createImage("./asset/odin_sentinel/fire-smoke.png");
            else return  ui.createImage("./asset/odin_sentinel/fire.png");
        } else if (this.hasSmoke()) {
            return ui.createImage("./asset/odin_sentinel/smoke.png");
        } else {
            return "";
        }
    }

    fireStatus() {
        let fire = this.sentinel.fire;
        //return (fire && fire.length > 0) ? util.f_1.format(fire[0].fire.fireProb) : "-";
        return (fire && fire.length > 0) ? fire[0].fire.fireProb.toFixed(2) : "-";
    }

    smokeStatus() {
        let smoke = this.sentinel.smoke;
        return (smoke && smoke.length > 0) ? smoke[0].smoke.smokeProb.toFixed(2) : "-";
    }

    imageStatus() {
        let images = this.sentinel.image;
        return (images && images.length > 0) ? images.length : "-";
    }

    showAssets(cond) {
        if (this.assets) this.assets.showAssets(cond);
    }

    // this could look for the best DOP value
    position() {
        let gps = this.sentinel.gps;
        if (gps && gps.length > 0) {
            let r = gps[0].gps;
            return Cesium.Cartesian3.fromDegrees(r.longitude, r.latitude, r.altitude);
        } else {
            return Cesium.Cartesian3.fromDegrees(0, 0, 0);  // TODO - we should move this out of sight
        }
    }

    lastCartographic (height=0.0) {
        let gps = this.sentinel.gps;
        if (gps && gps.length > 0) {
            let r = gps[0].gps;
            return new Cesium.Cartographic(util.toRadians(r.longitude),util.toRadians(r.latitude),height);
        } else {
            return null;
        }
    }

    temperature() {
        let thermo = this.sentinel.thermometer;
        if (thermo && thermo.length > 0) {
            return thermo[0].thermometer.temperature;
        }
    }

    humidity() {
        let gas = this.sentinel.gas;
        if (gas && gas.length > 0) {
            return gas[0].gas.humidity;
        }
    }

    windSpeed() {
        let anemo = this.sentinel.anemometer;
        if (anemo && anemo.length > 0) {
            return anemo[0].anemometer.speed;
        }
    }

    windDirection() {
        let anemo = this.sentinel.anemometer;
        if (anemo && anemo.length > 0) {
            return anemo[0].anemometer.angle;
        }
    }
}

// object for computation of image viewing angle display
const imgVector = {
    entity: undefined,
    vec: [new Cesium.Cartesian3(), new Cesium.Cartesian3()],
    clr: config.color,
    sensor: undefined,

    // temp vars we allocate only once
    p1Body: Cesium.Cartesian3.fromElements( 0, 500, 0), // forward in body frame 
    p1Enu: new Cesium.Cartesian3(),
    p1Ecef: new Cesium.Cartesian3(),

    setViewVector (sentinelEntry, imageRecord) {
        let image = imageRecord.image;
        this.sensor = imageRecord.sensorNo.toString();

        let p1Enu = Cesium.Matrix3.multiplyByVector(image.bodyToEnu, this.p1Body, this.p1Enu);
        let p1Ecef = Cesium.Matrix4.multiplyByPoint(sentinelEntry.enuToEcef, p1Enu, this.p1Ecef);

        this.vec[0] = sentinelEntry.pos;
        this.vec[1] = p1Ecef;
        this.clr = sentinelEntry.hasFire() ? config.alertColor : config.color;

        if (this.entity) {
            let e = this.entity;
            e.position = this.p1Ecef;
            e.label.text = this.sensor;
            e.label.fillColor = this.clr;
            e.polyline.positions = this.vec;
            e.polyline.material = this.clr;
        }
    },

    createEntity () {
        // could use "CallbackProperty( () => this.xx, false)"" for position, label.text, polyline.positions
        // (not we need a ColorMaterialProperty with a "p.color = new Cesium.CallbackProperty" for polyline.material) = but not label.fillColor
        // Callbacks are more instantaneous but at cost of whole frame rendering

        let ddc = new Cesium.DistanceDisplayCondition(0, config.zoomHeight); // (we show a vector length of 500m)

        return new Cesium.Entity({
            position: this.p1Ecef,
            label: {
                text: this.sensor,
                font: config.labelFont,
                scale: 0.8,
                horizontalOrigin: Cesium.HorizontalOrigin.LEFT,
                verticalOrigin: Cesium.VerticalOrigin.TOP,
                pixelOffset: config.labelOffset,
                fillColor: this.clr,
                distanceDisplayCondition: ddc
            },
            polyline: {
                positions: this.vec,
                material: this.clr,
                distanceDisplayCondition: ddc
            }
        });
    },

    show (cond) {
        if (cond) {
            if (!this.entity) {
                let entity = this.createEntity();
                if (entity) {
                    this.entity = entity;
                    odinCesium.addEntity(entity);
                }
            }
        }
        if (this.entity) {
            this.entity.show = cond;
            odinCesium.requestRender();
        }
    },

    showViewVector (sentinelEntry,imageRecord) {
        if (sentinelEntry && imageRecord && imageRecord.image.orientation) {
            this.setViewVector(sentinelEntry, imageRecord);
            this.show(true);
        } else {
            this.show(false);
        }
    }
};

//--- module initialization

odinCesium.addDataSource(sentinelDataSource);

createIcon();
createWindow();
sentinelView = initSentinelView();

sentinelImageView = initSentinelImagesView();
sentinelAccelView = initSentinelAccelView();
sentinelAnemoView = initSentinelAnemoView();
sentinelThermoView = initSentinelThermoView();
sentinelFireView = initSentinelFireView();
sentinelSmokeView = initSentinelSmokeView();
sentinelGasView = initSentinelGasView();
sentinelVocView = initSentinelVocView();
sentinelGpsView = initSentinelGpsView();
sentinelOrientationView = initSentinelOrientationView();
sentinelCloudCoverView = initSentinelCloudCoverView();

initSentinelCmdList();

odinCesium.setEntitySelectionHandler(sentinelSelection);
odinCesium.initLayerPanel("sentinel", config, showSentinels);
console.log("ui_cesium_sentinel initialized");


function createIcon() {
    return ui.Icon("./asset/odin_sentinel/sentinel.svg", (e)=> ui.toggleWindow(e,'sentinel'));
}

function createWindow() {
    let maxDataRows = 8;

    return ui.Window("Sentinels", "sentinel", "./asset/odin_sentinel/sentinel.svg")(
        ui.LayerPanel("sentinel", toggleShowSentinels),
        ui.List("sentinel.list", 10, selectSentinel,null,null,zoomToSentinel),
        ui.Panel("data", true)(
            ui.TabbedContainer()(
                ui.Tab("fire", false)( ui.List("sentinel.fire.list", maxDataRows)),
                ui.Tab("smoke", false)( ui.List("sentinel.smoke.list", maxDataRows)),
                ui.Tab("imgs", true)( ui.List("sentinel.image.list", maxDataRows, selectImage)),
                ui.Tab("gas", false)( ui.List("sentinel.gas.list", maxDataRows)),
                ui.Tab("temp", false)( ui.List("sentinel.thermo.list", maxDataRows)),
                ui.Tab("wind", false)( ui.List("sentinel.anemo.list", maxDataRows)),
                ui.Tab("cloud", false)( ui.List("sentinel.cloudcover.list", maxDataRows)),
                ui.Tab("voc", false)( ui.List("sentinel.voc.list", maxDataRows)),
                ui.Tab("accel", false)( ui.List("sentinel.accel.list", maxDataRows)),
                ui.Tab("gps",false)( ui.List("sentinel.gps.list", maxDataRows)),
                ui.Tab("att", false)( ui.List("sentinel.orientation.list", maxDataRows))
            )
        ),
        ui.Panel("diagnostics", false)(
            ui.List("sentinel.diag.cmdList", 6, selectSentinelCmd),
            ui.ColumnContainer()(
                ui.TextArea( "sentinel.diag.cmd", 44, 4, 0, true)
            ),
            ui.RowContainer()(
                ui.Button("send", sendSentinelCmd),
                ui.Button("clear history", clearSentinelHistory)
            ),
            ui.ColumnContainer()(
                ui.TextArea( "sentinel.diag.log", 44, 4, 0, true, true)
            )
        )
    );
}

function showSentinels (cond) { // triggered by panel
    if (imgVector.entity) imgVector.entity.show = cond;
    sentinelEntries.forEach( e=> e.showAssets(cond));
    odinCesium.requestRender();
}


function toggleShowSentinels(event) { // show action triggered by layer view (not panel)
    // TBD
}

function initSentinelView() {
    let view = ui.getList("sentinel.list");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "", width: "2rem", attrs: [], map: e => e.alertStatus() },
            { name: "id", width: "5rem", attrs: ["alignLeft"], map: e => e.displayId },
            { name: "fire", tip: "fire probability [0..1]", width: "4rem", attrs: ["fixed", "alignRight"], map: e => e.fireStatus() },
            { name: "smoke", tip: "smoke probability [0..1]", width: "4rem", attrs: ["fixed", "alignRight"], map: e => e.smokeStatus() },
            { name: "img", tip: "number of available images", width: "4rem", attrs: ["fixed", "alignRight"], map: e => e.imageStatus() },
            ui.listItemSpacerColumn(),
            { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.sentinel.timeRecorded) }
        ]);
    }
    return view;
}

function initListView (id, colSpecs) {
    let view = ui.getList(id);
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], colSpecs);
    }
    return view;
}
function initSentinelFireView() {
    return initListView( "sentinel.fire.list", [
        { name: "sen", tip: "sensor number", width: "2rem", attrs: [], map: e => e.sensorNo },
        { name: "prob", tip: "fire probability [0..1]", width: "6rem", attrs: ["fixed", "alignRight"], map: e => e.fire.fireProb.toFixed(2) },
        ui.listItemSpacerColumn(),
        { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.timeRecorded) }
    ]);
}
function initSentinelSmokeView() {
    return initListView( "sentinel.smoke.list", [
        { name: "sen", tip: "sensor number", width: "2rem", attrs: [], map: e => e.sensorNo },
        { name: "prob", tip: "smoke probability [0..1]", width: "6rem", attrs: ["fixed", "alignRight"], map: e => e.smoke.smokeProb.toFixed(2) },
        ui.listItemSpacerColumn(),
        { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.timeRecorded) }
    ]);
}
function initSentinelGasView() {
    return initListView( "sentinel.gas.list", [
        { name: "sen", tip: "sensor number", width: "2rem", attrs: [], map: e => e.sensorNo },
        { name: "gas", tip: "gas resistance [Ω]", width: "3rem", attrs: ["fixed", "alignRight"], map: e => e.gas.gas },
        { name: "hum", tip: "humidity [%]", width: "4rem", attrs: ["fixed", "alignRight"], map: e => util.f_1.format(e.gas.humidity) },
        { name: "pres", tip: "pressure [hPa]", width: "4.5rem", attrs: ["fixed", "alignRight"], map: e => util.f_1.format(e.gas.pressure) },
        { name: "alt", tip: "altitude [ft]", width: "5rem", attrs: ["fixed", "alignRight"], map: e => util.f_0.format(e.gas.altitude) },
        ui.listItemSpacerColumn(),
        { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.timeRecorded) }
    ]);
}
function initSentinelThermoView() {
    return initListView( "sentinel.thermo.list", [
        { name: "sen", tip: "sensor number", width: "2rem", attrs: [], map: e => e.sensorNo },
        { name: "temp", tip: "temperature [°C]", width: "6rem", attrs: ["fixed", "alignRight"], map: e => util.f_1.format(e.thermometer.temperature) },
        ui.listItemSpacerColumn(),
        { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.timeRecorded) }
    ]);   
}
function initSentinelAnemoView() {
    return initListView( "sentinel.anemo.list", [
        { name: "sen", tip: "sensor number", width: "2rem", attrs: [], map: e => e.sensorNo },
        { name: "dir", tip: "wind direction [°]", width: "4rem", attrs: ["fixed", "alignRight"], map: e => util.f_0.format(e.anemometer.angle) },
        { name: "spd", tip: "wind speed [m/s]", width: "6rem", attrs: ["fixed", "alignRight"], map: e => util.f_2.format(e.anemometer.speed) },
        ui.listItemSpacerColumn(),
        { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.timeRecorded) }
    ]);  
}
function initSentinelVocView() {
    return initListView( "sentinel.voc.list", [
        { name: "sen", tip: "sensor number", width: "2rem", attrs: [], map: e => e.sensorNo },
        { name: "tvoc", tip: "total volatile organic compounds [ppb]", width: "3rem", attrs: ["fixed", "alignRight"], map: e => util.f_0.format(e.voc.TVOC) },
        { name: "eCO2", tip: "estimated CO₂ concentration [ppm]", width: "4rem", attrs: ["fixed", "alignRight"], map: e => util.f_0.format(e.voc.eCO2) },
        ui.listItemSpacerColumn(),
        { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.timeRecorded) }
    ]);   
}
function initSentinelAccelView() {
    return initListView( "sentinel.accel.list", [
        { name: "sen", tip: "sensor number", width: "2rem", attrs: [], map: e => e.sensorNo },
        { name: "ax", tip: "x-acceleration [m/s²]", width: "5rem", attrs: ["fixed", "alignRight"], map: e => util.f_3.format(e.accelerometer.ax) },
        { name: "ay", tip: "y-acceleration [m/s²]",width: "5rem", attrs: ["fixed", "alignRight"], map: e => util.f_3.format(e.accelerometer.ay) },
        { name: "az", tip: "z-acceleration [m/s²]",width: "5rem", attrs: ["fixed", "alignRight"], map: e => util.f_3.format(e.accelerometer.az) },
        ui.listItemSpacerColumn(),
        { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.timeRecorded) }
    ]); 
}
function initSentinelGpsView() {
    return initListView( "sentinel.gps.list", [
        { name: "sen", tip: "sensor number", width: "2rem", attrs: [], map: e => e.sensorNo },
        { name: "lat", tip: "latitude [°]", width: "5rem", attrs: ["fixed", "alignRight"], map: e => util.f_5.format(e.gps.latitude) },
        { name: "lon", tip: "longitude [°]", width: "7rem", attrs: ["fixed", "alignRight"], map: e => util.f_5.format(e.gps.longitude) },
        { name: "alt", tip: "altitude [m]", width: "3rem", attrs: ["fixed", "alignRight"], map: e => util.f_0.format(e.gps.altitude) },
        ui.listItemSpacerColumn(),
        { name: "hdop", tip: "horizontal dilution of precision", width: "2rem", attrs: ["fixed", "alignRight"], map: e => util.f_1.format(e.gps.hdop) },
        { name: "q", tip: "quality", width: "2rem", attrs: ["fixed", "alignRight"], map: e => e.gps.quality },
        { name: "n", tip: "number of satellites", width: "2rem", attrs: ["fixed", "alignRight"], map: e => e.gps.numberOfSatellites },
        ui.listItemSpacerColumn(),
        { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.timeRecorded) }
    ]);  
}
function initSentinelOrientationView() {
    return initListView( "sentinel.orientation.list", [
        { name: "sen", tip: "sensor number", width: "3rem", attrs: [], map: e => e.sensorNo },
        { name: "hdg", tip: "view direction [°]", width: "3rem", attrs: ["fixed", "alignRight"], map: e => util.degString(e.orientation.hpr.heading) },
        { name: "pitch", tip: "view tilt [°]", width: "3rem", attrs: ["fixed", "alignRight"], map: e => util.degString(e.orientation.hpr.pitch) },
        { name: "roll", tip: "view rotation [°]", width: "3rem", attrs: ["fixed", "alignRight"], map: e => util.degString(e.orientation.hpr.roll) }, 
        ui.listItemSpacerColumn(),
        { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.timeRecorded) }
    ]);  
}

function initSentinelCloudCoverView() {
    return initListView( "sentinel.cloudcover.list", [
        { name: "sen", tip: "sensor number", width: "2rem", attrs: [], map: e => e.sensorNo },
        { name: "cc", tip: "cloud coverage [%]", width: "3rem", attrs: ["fixed", "alignRight"], map: e => util.f_0.format(0.0) },
        ui.listItemSpacerColumn(),
        { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.timeRecorded) }
    ]);  
}
function initSentinelImagesView() {
    return initListView( "sentinel.image.list", [
        { name: "", width: "2rem", attrs: [], map: e => ui.createCheckBox(e.window, toggleShowImage, null) },
        { name: "sen", tip: "sensor number", width: "2rem", attrs: [], map: e => e.sensorNo },
        { name: "type", tip: "ir: infrared, vis: visible", width: "2rem", attrs: [], map: e => e.image.isInfrared ? "ir" : "vis" },
        { name: "hdg", tip: "heading [°]", width: "3rem", attrs: ["fixed", "alignRight"], map: e => imageHeading(e.image) }, 
        ui.listItemSpacerColumn(),
        { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.timeRecorded) }
    ]);
}

function imageHeading (image) {
    if (image.hpr) {
        let hdg = util.toDegrees(-image.hpr.heading);
        if (hdg < 0) hdg = 360 + hdg;
        return util.f_0.format(hdg);
    } else {
        return "-";
    }
}

function toggleShowImage(event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let e = ui.getListItemOfElement(cb);
        if (e) {
            if (e.window) {
                ui.removeWindow(e.window);
                e.window = null;
            } else {
                setTimeout(() => { // otherwise the mouseUp will put the focus back on sentinelsView
                    let w = ui.createWindow( imageTitle(e), false, () => {
                        e.window = undefined;
                        ui.updateListItem(sentinelImageView, e);
                    });
                    let uri = "./sentinel-image/" + e.image.localFilename;
                    let img = ui.createImage( uri, "waiting for image..", config.imageWidth);
                    ui.addWindowContent(w, img);
                    //ui.setWindowResizable(w, true);
                    ui.addWindow(w);
                    ui.setWindowLocation(w, event.clientX + 10, event.clientY + 10);
                    ui.showWindow(w);

                    e.window = w;
                }, 0);
            }
        }
    }
}

function imageTitle (e) {
    if (e.image) {
        return `sensor: ${e.sensorNo}  │  date: ${util.toLocalMDHMString(e.timeRecorded)}  │  heading: ${imageHeading(e.image)}°`;
    } else {
        return "?"
    }
}

function initSentinelCmdList() {
    let view = ui.getList("sentinel.diag.cmdList");
    if (view) {
        ui.setListItemDisplayColumns( view, ["fit"], [
            { name: "template", tip: "name of command to instantiate", width: "26rem", attrs:[], map: e => e }
        ]);

        ui.setListItems(view, Array.from(diagnosticCommands.keys()));
    }
}

function sentinelSelection() {
    let sel = odinCesium.getSelectedEntity();
    if (sel && sel._uiSentinelEntry) {
        if (sel._uiSentinelEntry !== selectedSentinelEntry) {
            ui.setSelectedListItem(sentinelView, sel._uiSentinelEntry);
        }
    }
}

function handleWsMessages(msgType, msg) {
    switch (msgType) {
        case "sentinels": handleSentinelsMessage(msg); break;
        case "update": handleSentinelUpdateMessage(msg); break;
        case "cmdResponse": logResponse(msg); break;
    }
}

function handleSentinelsMessage(sentinels) {
    sentinelEntries.clear();
    sentinels.forEach(sentinel => addSentinelEntry(sentinel));
}

function addSentinelEntry(sentinel) {
    let e = new SentinelEntry(sentinel);

    sentinelEntries.set(sentinel.deviceId, e);
    let idx = sentinelList.insert(e);
    ui.insertListItem(sentinelView, e, idx);

    if (sentinel.orientation) {
        sentinel.orientation.forEach( rec=> setOrientationHpr(rec.orientation));
    }
    if (sentinel.image) {
        sentinel.image.forEach( rec=> setImageOrientation(sentinel, rec.image));
    }

    if (sentinel.gps) e.assets = createAssets(e);
    checkFireAsset(e);
}

function handleSentinelUpdateMessage(update) {
    let id = update.deviceId;
    let e = sentinelEntries.get(id);

    if (e) {
        let sentinel = e.sentinel;
        if (update.fire) {
            updateSentinelReadings(e, 'fire', update, sentinelFireView);
            checkFireAsset(e);
        }
        else if (update.smoke) updateSentinelReadings(e, 'smoke', update, sentinelSmokeView);
        else if (update.image) {
            setImageOrientation(sentinel, update.image);
            updateSentinelReadings(e, 'image', update, sentinelImageView);
        }
        else if (update.anemometer) {
            updateSentinelReadings(e, 'anemometer', update, sentinelAnemoView);
            updateDetails(e);
        }
        else if (update.gas) {
            updateSentinelReadings(e, 'gas', update, sentinelGasView);
            updateDetails(e);
        }
        else if (update.voc) updateSentinelReadings(e, 'voc', update, sentinelVocView);
        else if (update.accelerometer) updateSentinelReadings(e, 'accelerometer', update, sentinelAccelView);
        else if (update.gps) {
            updateSentinelReadings(e, 'gps', update, sentinelGpsView);
            if (!e.assets) {
                e.assets = createAssets(e);
                odinCesium.requestRender();
            }
        }
        else if (update.thermometer) {
            updateSentinelReadings(e, 'thermometer', update, sentinelThermoView);
            updateDetails(e);
        }
        else if (update.orientation) {
            setOrientationHpr( update.orientation); // get heading/pitch/roll
            updateSentinelReadings(e, 'orientation', update, sentinelOrientationView);
            updateDetails(e);
        }
        else if (update.cloudcover) { updateSentinelReadings(e, 'cloudcover', update, sentinelCloudCoverView); }
    }
}

function setOrientationHpr(orientation) {
    let o = orientation;
    let q = new Cesium.Quaternion( o.qx, o.qy, o.qz, o.w);
    orientation.hpr = Cesium.HeadingPitchRoll.fromQuaternion(q);
}

function setImageOrientation (sentinel, image) {
    if (image.orientationRecord) {
        let oRec = getRecordWithId( sentinel.orientation, image.orientationRecord.id);
        if (oRec) {
            let o = oRec.orientation;
            let q = new Cesium.Quaternion( o.qx, o.qy, o.qz, o.w);
            image.hpr = Cesium.HeadingPitchRoll.fromQuaternion(q);

            let qRot = Cesium.Quaternion.inverse(q, new Cesium.Quaternion());    
            image.bodyToEnu = Cesium.Matrix3.fromQuaternion( qRot);
        }
    }
}

function getRecordWithId( records, id) {
    if (records && records.length > 0) {
        let n = records.length;
        for (let i=0; i<n; i++) {
            let rec = records[i];
            if (rec.id == id) {
                return rec;
            }
        }
    }
    return null;
}

function updateSentinelReadings (sentinelEntry, memberName, newReading, view) {
    let sentinel = sentinelEntry.sentinel;
    let readings = sentinel[memberName];

    sentinel.timeRecorded = newReading.timeRecorded;

    if (readings) {
        if (readings.length >= maxHistory) {
            readings.copyWithin(1,0,readings.length-1);
            readings[0] = newReading;
        } else {
            readings.unshift(newReading);
        }
        readings.sort( (a,b) => b.timeRecorded - a.timeRecorded); // in case records come out of order
    } else {
        readings = [newReading];
        sentinel[memberName] = readings;
    }

    if (sentinelEntry == selectedSentinelEntry)  ui.setListItems(view, readings);
    ui.updateListItem(sentinelView, sentinelEntry);
}

function checkFireAsset(e) {
    if (e.hasFire() || e.hasSmoke()) {
        if (e.assets) {
            e.assets.symbol.billboard.color = config.alertColor;

            /*
            if (!e.assets.fire) {
                e.assets.fire = createFireAsset(e);
                if (e.assets.fire) e.assets.fire.show = true;
            } else {
                // update fire location/probability
            }
            */
            odinCesium.requestRender();
        }
    }
}

function createAssets(sentinelEntry) {
    return new SentinelAssets(
        createSymbolAsset(sentinelEntry),
        createDetailAsset(sentinelEntry)
    );
}

function createSymbolAsset(sentinelEntry) {
    let sentinel = sentinelEntry.sentinel;

    let entity = new Cesium.Entity({
        id: sentinel.deviceId,
        position: sentinelEntry.pos,
        billboard: {
            image: './asset/odin_sentinel/sentinel-sym.png',
            distanceDisplayCondition: config.billboardDC,
            color: config.color,
            //heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
        },
        label: {
            text: sentinelEntry.displayId,
            scale: 0.8,
            horizontalOrigin: Cesium.HorizontalOrigin.LEFT,
            verticalOrigin: Cesium.VerticalOrigin.TOP,
            font: config.labelFont,
            fillColor: config.color,
            showBackground: true,
            backgroundColor: config.labelBackground,
            pixelOffset: config.labelOffset,
            distanceDisplayCondition: config.labelDC,
            heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
            disableDepthTestDistance: Number.POSITIVE_INFINITY,
        },
        point: {
            pixelSize: config.pointSize,
            color: config.color,
            outlineColor: config.pointOutlineColor,
            outlineWidth: config.pointOutlineWidth,
            distanceDisplayCondition: config.pointDC, 
        }
    });
    entity._uiSentinelEntry = sentinelEntry; // backlink

    sentinelDataSource.entities.add(entity);
    return entity;
}


function createDetailAsset (sentinelEntry) {
    let entity = new Cesium.Entity({
        id: sentinelEntry.id + "-info",
        position: sentinelEntry.pos,
        label: {
            text: sentinelInfoText(sentinelEntry),
            font: config.infoFont,
            scale: 0.8,
            horizontalOrigin: Cesium.HorizontalOrigin.LEFT,
            verticalOrigin: Cesium.VerticalOrigin.TOP,
            fillColor: config.color,
            showBackground: true,
            backgroundColor: config.labelBackground, // alpha does not work against model
            outlineColor: config.color,
            outlineWidth: 1,
            pixelOffset: config.infoOffset,
            distanceDisplayCondition: config.infoDC,
            heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
            disableDepthTestDistance: Number.POSITIVE_INFINITY,
        }
    });

    sentinelDataSource.entities.add(entity);
    return entity;
}

function sentinelInfoText (se) {
    //console.log(se);
    let value = (v,f) => v ? f(v) : '-';

    let temp = value(se.temperature(), (v)=>Math.round(v));
    let humidity = value(se.humidity(), (v)=>Math.round(v));
    let windDir = value(se.windDirection(), (v)=>Math.round(v));
    let windSpd = value(se.windSpeed(), (v)=>util.f_1.format(v));

    return `${temp} °C\n${humidity} %\n${windDir} °\n${windSpd} m/s`
}

function updateDetails (se) {
    if (se.assets && se.assets.details){
        se.assets.details.label.text = sentinelInfoText(se);
        odinCesium.requestRender();
    }
}

function selectSentinel(event) {
    let e = event.detail.curSelection;
    if (e) {
        selectedSentinelEntry = e;
        let sentinel = e.sentinel;
        setDataViews(sentinel);
    } else {
        selectedSentinelEntry = undefined;
        clearDataViews();
    }
    selectedImage = null;
    imgVector.show(false);
}

function zoomToSentinel(event) {
    let lv = ui.getList(event);
    if (lv) {
        let se = ui.getSelectedListItem(lv);
        if (se) {
            let pos = se.lastCartographic(config.zoomHeight);
            odinCesium.zoomTo( Cesium.Cartographic.toCartesian(pos));
            odinCesium.setSelectedEntity(se.assets.symbol);
        }
    }
}

function setDataViews(sentinel) {
    ui.setListItems(sentinelImageView, sentinel.image);
    ui.setListItems(sentinelAccelView, sentinel.accelerometer);
    ui.setListItems(sentinelAnemoView, sentinel.anemometer);
    ui.setListItems(sentinelThermoView, sentinel.thermometer);
    ui.setListItems(sentinelFireView, sentinel.fire);
    ui.setListItems(sentinelSmokeView, sentinel.smoke);
    ui.setListItems(sentinelGasView, sentinel.gas);
    ui.setListItems(sentinelVocView, sentinel.voc);
    ui.setListItems(sentinelGpsView, sentinel.gps);
    ui.setListItems(sentinelOrientationView, sentinel.orientation);
}

function clearDataViews() {
    ui.clearList(sentinelImageView);
    ui.clearList(sentinelAccelView);
    ui.clearList(sentinelAnemoView);
    ui.clearList(sentinelThermoView);
    ui.clearList(sentinelFireView);
    ui.clearList(sentinelSmokeView);
    ui.clearList(sentinelGasView);
    ui.clearList(sentinelVocView);
    ui.clearList(sentinelGpsView);
    ui.clearList(sentinelOrientationView);
}

function selectImage(event) {
    let e = event.detail.curSelection;
    selectedImage = e;

    if (e) {
        if (e.window) {
            ui.raiseWindowToTop(e.window);
        }
        imgVector.showViewVector(selectedSentinelEntry, e);

    } else {
        imgVector.show(false);
    }
}

//--- diagnostics

function triggerAlertCmd() {
    return `{"event": "trigger-alert",\n  "data":{ "deviceIds": ["$DEVICE"]}}`;
}

function turnLightsOnCmd() {
    return `{"event": "switch-lights",\n  "data":{ "type": "external-lights",\n  "state": "on",\n     "deviceIds": ["$DEVICE"]}}`;
}

function turnLightsOffCmd() {
    return `{"event": "switch-lights",\n  "data":{ "subject": "external-lights",\n  "state": "off",\n     "deviceIds": ["$DEVICE"]}}`;
}

function selectSentinelCmd(event) {
    let cmdDescr = ui.getSelectedListItem(event.target);

    let cmdFunc = diagnosticCommands.get(cmdDescr);
    if (cmdFunc) {
        let cmd = cmdFunc();
        if (cmd) {
            ui.setTextAreaContent("sentinel.diag.cmd", cmd);
        }
    } else {
        console.log("ignoring unknown command: ", cmdDescr);
    }
}

function resolveCmdVariables(cmd) {
    if (cmd.includes("$DEVICE")) {
        if (!selectedSentinelEntry) {
            alert("please select device before sending command");
            return;
        }
        cmd = cmd.replace("$DEVICE", selectedSentinelEntry.id);
    }
    //... and possibly more variables to follow
    return cmd;
}

function sendSentinelCmd() {
    let cmdInput = ui.getTextAreaContent("sentinel.diag.cmd");
    if (cmdInput){
        cmdInput = resolveCmdVariables(cmdInput);
        try {
            let cmd = JSON.parse(cmdInput);
            ws.sendWsMessage( MODULE_PATH, "cmd", cmd);

        } catch (error) {
            alert("malformed JSON command: ", error);
        }
    }
}

function clearSentinelHistory() {
    ui.setTextAreaContent("sentinel.diag.log", null);
}

function logResponse (response) {
    if (response) {
        let logView = ui.getTextArea("sentinel.diag.log");
        let text = ui.getTextAreaContent(logView);
        let now = Date.now();

        if (text) {
            text = `${util.toLocalTimeString(now)} : ${response}\n${text}`;
        } else {
            text = `${util.toLocalTimeString(now)} : ${response}`;
        }

        ui.setTextAreaContent(logView, text);
    }
}
