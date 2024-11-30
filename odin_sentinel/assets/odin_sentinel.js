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
import * as wnd from "../odin_server/ui_windows.js";
import * as ws from "../odin_server/ws.js";
import * as odinCesium from "../odin_cesium/odin_cesium.js";

const MOD_PATH = "odin_sentinel::sentinel_service::SentinelService";

ws.addWsHandler( MOD_PATH, handleWsMessages);

var sentinelInactiveDuration = undefined;
var sentinelDataSource = new Cesium.CustomDataSource("sentinel");
var sentinelView = undefined;
var sentinelEntries = new Map();
var sentinelInfos = new Map();
var sentinelList = new SkipList( // id-sorted display list for trackEntryView
    3, // max depth
    (a, b) => a.id < b.id, // sort function
    (a, b) => a.id == b.id // identity function
);

var selectedSentinelEntry = undefined;
var selectedImage = undefined;

var sentinelImageView = undefined;
var sentinelAlarmView = undefined;
var sentinelGasView = undefined;
var sentinelThermoView = undefined;
var sentinelAnemoView = undefined;
var sentinelVocView = undefined;
var sentinelAccelView = undefined;
var sentinelGpsView = undefined;
var sentinelOrientationView = undefined;
var sentinelCloudCoverView = undefined;
var sentinelPowerView = undefined;

var sentinelNameLabel = undefined;

var maxHistory = config.maxHistory;

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

        this.setAlarmList();
        this.setPos();
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

    // note this might be deferred if we don't have a barometric alt
    setPos() {
        let gpsRecs = this.sentinel.gps;
        if (gpsRecs && gpsRecs.length > 0) {
            let gps = gpsRecs[0].gps;
            // we don't get altitude from gps (not accurate enough)
            let gasRecs = this.sentinel.gas;
            if (gasRecs && gasRecs.length > 0) {
                let gas = gasRecs[0].gas; // we should match this to gps time
                this.pos = Cesium.Cartesian3.fromDegrees( gps.longitude, gps.latitude, gas.altitude);
            } else {
                this.pos = Cesium.Cartesian3.fromDegrees( gps.longitude, gps.latitude, 0); // only temp
                odinCesium.withDetailedSampledTerrain( [Cesium.Cartographic.fromDegrees( gps.longitude, gpd.latitude)], (positions)=>{
                    this.pos = positions[0].toCartesian();
                });
            }
        } else {
            this.pos = Cesium.Cartesian3.fromDegrees(0, 0, 0);  // TODO - we should move this out of sight
        }
    }

    setAlarmList () {
        let alarms = this.sentinel.smoke.filter( (rec)=> rec.smoke.smokeProb > 0);
        this.sentinel.fire.forEach( (rec)=> {
            if (rec.fire.fireProb > 0) {
                util.sortIn( alarms, rec, (a,b)=> a.timeRecorded > b.timeRecorded);
            }
        });
        
        this.alarmList = alarms;
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


//--- module initialization

odinCesium.addDataSource(sentinelDataSource);

createIcon();
createWindow();
sentinelView = initSentinelView();

sentinelImageView = initSentinelImagesView();
sentinelAccelView = initSentinelAccelView();
sentinelAnemoView = initSentinelAnemoView();
sentinelThermoView = initSentinelThermoView();
sentinelAlarmView = initSentinelAlarmView();
sentinelGasView = initSentinelGasView();
sentinelVocView = initSentinelVocView();
sentinelGpsView = initSentinelGpsView();
sentinelOrientationView = initSentinelOrientationView();
sentinelCloudCoverView = initSentinelCloudCoverView();
sentinelPowerView = initSentinelPowerView();
sentinelNameLabel = ui.getText("sentinel.name");

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

        ui.Text("sentinel.name"),
        ui.Panel("data", true)(
            ui.TabbedContainer()(
                ui.Tab("alarm", false)( ui.List("sentinel.alarm.list", maxDataRows)),
                ui.Tab("img", true)( ui.List("sentinel.image.list", maxDataRows, selectImage)),
                ui.Tab("gas", false)( ui.List("sentinel.gas.list", maxDataRows)),
                ui.Tab("temp", false)( ui.List("sentinel.thermo.list", maxDataRows)),
                ui.Tab("wind", false)( ui.List("sentinel.anemo.list", maxDataRows)),
                ui.Tab("cloud", false)( ui.List("sentinel.cloudcover.list", maxDataRows)),
                ui.Tab("voc", false)( ui.List("sentinel.voc.list", maxDataRows)),
                ui.Tab("accel", false)( ui.List("sentinel.accel.list", maxDataRows)),
                ui.Tab("gps", false)( ui.List("sentinel.gps.list", maxDataRows)),
                ui.Tab("att", false)( ui.List("sentinel.orientation.list", maxDataRows)),
                ui.Tab("power", false)( ui.List("sentinel.power.list", maxDataRows))
            )
        )
    );
}

function showSentinels (cond) { // triggered by panel
    sentinelDataSource.show = cond;
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
            { name: "stat", tip: "inactive alert", width: "2rem", attrs:["alignRight"], map: e => e.inactive ? "⚠︎" : "" },
            { name: "last report", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.sentinel.timeRecorded) }
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

function initSentinelAlarmView() {
    return initListView( "sentinel.alarm.list", [
        { name: "sen", tip: "sensor number", width: "3rem", attrs: [], map: e => e.sensorNo },
        { name: "type", tip: "alarm type (fire,smoke)", width: "2rem", attrs:[], map: e => e.fire ? "\u1f525" : "\u2601" },
        { name: "prob", tip: "probability [0..1]", width: "6rem", attrs: ["fixed", "alignRight"], map: e => (e.fire ? e.fire.fireProb : e.smoke.smokeProb).toFixed(2) },
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
        { name: "alt", tip: "altitude [m]", width: "4rem", attrs: ["fixed", "alignRight"], map: e => util.f_0.format(e.gps.altitude) },
        ui.listItemSpacerColumn(),
        { name: "hdop", tip: "horizontal dilution of precision", width: "2rem", attrs: ["fixed", "alignRight"], map: e => util.f_1.format(e.gps.hdop) },
        //{ name: "q", tip: "quality", width: "2rem", attrs: ["fixed", "alignRight"], map: e => e.gps.quality },
        //{ name: "n", tip: "number of satellites", width: "2rem", attrs: ["fixed", "alignRight"], map: e => e.gps.numberOfSatellites },
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

function initSentinelPowerView() {
    return initListView( "sentinel.power.list", [
        { name: "batV", tip: "battery Voltage [V]", width: "4rem", attrs: ["fixed", "alignRight"], map: e => util.f_2.format(e.power.batteryVoltage) },
        { name: "batA", tip: "battery current [mA]", width: "4rem", attrs: ["fixed", "alignRight"], map: e => util.f_2.format(e.power.batteryCurrent/1000) },
        { name: "solV", tip: "solar Voltage [V]", width: "4rem", attrs: ["fixed", "alignRight"], map: e => util.f_2.format(e.power.solarVoltage) },
        { name: "solA", tip: "solar current [mA]", width: "4rem", attrs: ["fixed", "alignRight"], map: e => util.f_2.format(e.power.solarCurrent/1000) },
        { name: "loadV", tip: "load Voltage [V]", width: "4rem", attrs: ["fixed", "alignRight"], map: e => util.f_2.format(e.power.loadVoltage) },
        { name: "loadA", tip: "load current [mA]", width: "4rem", attrs: ["fixed", "alignRight"], map: e => util.f_2.format(e.power.loadCurrent/1000) },
        ui.listItemSpacerColumn(),
        { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.timeRecorded) }
    ]); 
}

function initSentinelImagesView() {
    return initListView( "sentinel.image.list", [
        { name: "show", tip: "show image", width: "3rem", attrs: [], map: e => ui.createCheckBox(e.window, toggleShowImage, null) },
        { name: "fov", tip: "show fov", width: "3rem", attrs: [], map: e => ui.createCheckBox(e.fovAsset, toggleShowFov, null) },

        { name: "sen", tip: "sensor number", width: "2rem", attrs: [], map: e => e.sensorNo },
        { name: "type", tip: "ir: infrared, vis: visible", width: "2rem", attrs: [], map: e => e.image.isInfrared ? "ir" : "vis" },
        { name: "hdg", tip: "heading [°]", width: "3rem", attrs: ["fixed", "alignRight"], map: e => imageHeading(e.image) }, 
        ui.listItemSpacerColumn(),
        { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.timeRecorded) }
    ]);
}

function imageHeading (image) {
    if (image.hpr) {
        let hdg = util.toDegrees(image.hpr.heading);
        if (hdg < 0) hdg += 360.0;
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
                    let uri = "./sentinel-image/" + e.image.localFilename;
                    let w = wnd.ImageWindow( 
                        imageTitle(e), null,
                        () => { ui.setCheckBox(cb, false) },
                        uri, "", 
                        config.imageWidth, config.imageHeight,
                        event.clientX + 10, event.clientY + 10
                    );
                    e.window = w;
                }, 0);
            }
        }
    }
}

function imageTitle (e) {
    if (e.image) {
        return `${selectedSentinelEntry.displayId} : ${e.sensorNo} │ ${util.toLocalMDHMSString(e.timeRecorded)} │ ${imageHeading(e.image)}°`;
    } else {
        return "?"
    }
}

function toggleShowFov(event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let e = ui.getListItemOfElement(cb); // imageRec
        if (e) {
            if (ui.isCheckBoxSelected(cb)) {
                if (!e.fovAsset) {
                    e.fovAsset = createFovAsset( e);
                    if (e.fovAsset) { sentinelDataSource.entities.add(e.fovAsset); }
                }
            } else {
                if (e.fovAsset) {
                    sentinelDataSource.entities.remove(e.fovAsset);
                    e.fovAsset = undefined;
                }
            }
            odinCesium.requestRender();
        }
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
        case "device_infos": handleDeviceInfoMessage(msg); break;
        case "inactive_duration": handleInactiveDurationMessage(msg); break;
        case "sentinels": handleSentinelsMessage(msg); break;
        case "update": handleSentinelUpdateMessage(msg); break;
        case "alert": handleSentinelAlertMessage(msg); break;
        case "cmdResponse": logResponse(msg); break;
    }
}

function handleDeviceInfoMessage(deviceInfos) {
    sentinelInfos = deviceInfos;
}

// this is for client side inactive checks
function handleInactiveDurationMessage(millis) {
    sentinelInactiveDuration = millis;
}

function handleSentinelsMessage(sentinels) {
    sentinelEntries.clear();
    sentinels.forEach(sentinel => addSentinelEntry(sentinel));
    odinCesium.requestRender();
    
    if (sentinelInactiveDuration) {
        checkInactiveStatus();
        setTimeout( ()=> checkInactiveStatus(), 60000); // run this every minute
    }
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
        sentinel.image.forEach( rec=> {
            setImagePosition( sentinel, rec);
            setImageHpr( sentinel, rec.image);
        });
    }

    if (sentinel.gps) e.assets = createAssets(e);
    checkFireAsset(e);
}

function handleSentinelAlertMessage(alert) {
    let id = alert.deviceId;
    let e = sentinelEntries.get(id);
    if (e) {
        e.inactive = true;
        ui.updateListItem(sentinelView, e);
    }
}

function handleSentinelUpdateMessage(update) {
    let id = update.deviceId;
    let e = sentinelEntries.get(id);

    if (e) {
        e.inactive = false;
        let sentinel = e.sentinel;

        if (update.fire) {
            updateAlarmList( e, update);
            updateSentinelReadings(e, 'fire', update, sentinelAlarmView);
            checkFireAsset(e);
        }
        else if (update.smoke) {
            updateAlarmList( e, update);
            updateSentinelReadings(e, 'smoke', update, sentinelAlarmView);
        }
        else if (update.image) {
            setImagePosition( sentinel, update);
            setImageHpr(sentinel, update.image);
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
        else if (update.power) {
            updateSentinelReadings(e, 'power', update, sentinelPowerView);
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
    let hpr = Cesium.HeadingPitchRoll.fromQuaternion(q);
    hpr.heading = -hpr.heading; // TODO quat not enu ? 
    if (hpr.heading < 0) hpr.heading += Math.PI*2;
    orientation.hpr = hpr;
}

function setImagePosition (sentinel, imageRecord) {
    let time = imageRecord.timeRecorded;
    let gpsRec = sentinel.gps.find( (rec)=> time >= rec.timeRecorded);
    let gasRec = sentinel.gas.find( (rec)=> time >= rec.timeRecorded);
    if (gpsRec && gasRec) {
        imageRecord.image.pos = Cesium.Cartesian3.fromDegrees( gpsRec.gps.longitude, gpsRec.gps.latitude, gasRec.gas.altitude);
    }
}

function setImageHpr (sentinel, image) {
    if (image.orientationRecord) {
        let oRec = getRecordWithId( sentinel.orientation, image.orientationRecord.id);
        if (oRec) {
            let o = oRec.orientation;
            let q = new Cesium.Quaternion( o.qx, o.qy, o.qz, o.w);
            let hpr = Cesium.HeadingPitchRoll.fromQuaternion(q);
            hpr.heading = -hpr.heading; // TODO quat not enu ? 
            if (hpr.heading < 0) hpr.heading += Math.PI*2;
            image.hpr = hpr;

            let qRot = Cesium.Quaternion.inverse(q, new Cesium.Quaternion());    
            image.bodyToEnu = Cesium.Matrix3.fromQuaternion( qRot);
        }
    }
}

function checkInactiveStatus() {
    if (sentinelInactiveDuration) {
        let now = Date.now();
        sentinelEntries.values().forEach( e=> {
            if (now - e.sentinel.timeRecorded >= sentinelInactiveDuration) {
                if (!e.inactive) {
                    e.inactive = true;
                    ui.updateListItem(sentinelView, e);
                }
            } else {
                if (e.inactive) {
                    e.inactive = false;
                    ui.updateListItem(sentinelView, e);
                }
            }
        })
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

function updateAlarmList (sentinelEntry, update) {
    let prob = update.fire ? update.fire.fireProb : update.smoke.smokeProb;
    if (prob) { 
        let alarms = sentinelEntry.alarmList;
        alarms.unshift(update);
        if (alarms.length >= maxHistory) alarms.pop();
    }
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

    if (sentinelEntry == selectedSentinelEntry) {
        if (newReading.fire || newReading.smoke) {
            ui.setListItems(view, sentinelEntry.alarmList);
        } else {
            ui.setListItems(view, readings);
        }
    }
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
        createDetailAsset(sentinelEntry),
        // fov assets are created on demand
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
            //heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
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
            //heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
            disableDepthTestDistance: Number.POSITIVE_INFINITY,
        }
    });

    sentinelDataSource.entities.add(entity);
    return entity;
}

function createFovAsset (imageRec) {
    let image = imageRec.image;
    let pos = image.pos;
    let hpr = image.hpr;

    if (pos && hpr) {
        let hprFov = new Cesium.HeadingPitchRoll( hpr.heading - Math.PI/2, hpr.pitch, hpr.roll);
        let fov = getFov( selectedSentinelEntry, imageRec.sensorNo);

        return new Cesium.Entity({
            position: pos,
            orientation: Cesium.Transforms.headingPitchRollQuaternion( pos, hprFov),
            ellipsoid: {
                radii: new Cesium.Cartesian3(fov.dist, fov.dist, fov.dist),
                innerRadii: new Cesium.Cartesian3(10.0, 10.0, 10.0),
                minimumClock: Cesium.Math.toRadians(fov.left),
                maximumClock: Cesium.Math.toRadians(fov.right),
                minimumCone: Cesium.Math.toRadians(90.0),
                maximumCone: Cesium.Math.toRadians(90.0),
                material: config.fovColor,
                outline: true,
                outlineColor: config.fovOutlineColor,
                outlineWidth: 1,
                distanceDisplayCondition: config.fovDC,
            }
        });
    } else {
        return undefined;
    }
}

function getFov (sentinelEntry, sensorNo) {
    let fovLeft = config.fovLeft;
    let fovRight = config.fovRight;
    let fovDist = config.fovDist;

    let si = sentinelInfos[ sentinelEntry.id];
    if (si) {
        let ssi = si.sensorInfos.find( (e) => e.sensorNo == sensorNo );
        if (ssi) {
            fovLeft = ssi.fovLeft;
            fovRight = ssi.fovRight;
            fovDist = ssi.fovDist;
        }
    }
    return { left: fovLeft, right: fovRight, dist: fovDist };
}

function sentinelInfoText (se) {
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
    if (event.detail.src && event.detail.src.detail == 2) return; // this is the 2nd of a double click

    let e = event.detail.curSelection;
    if (e && e != selectedSentinelEntry) {
        selectedSentinelEntry = e;
        setSelectedSentinelName();
        setDataViews(e);
        selectedImage = null;
    }
}

function setSelectedSentinelName() {
    if (selectedSentinelEntry) {
        let sentinelInfo = sentinelInfos[selectedSentinelEntry.id];
        if (sentinelInfo) { 
            ui.setTextContent( sentinelNameLabel, selectedSentinelEntry.id + " : " + sentinelInfo.name); 
            return;
        } 
    }

    ui.clearTextContent(sentinelNameLabel);
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

function setDataViews(sentinelEntry) {
    let sentinel = sentinelEntry.sentinel;

    ui.setListItems(sentinelAlarmView, sentinelEntry.alarmList);
    ui.setListItems(sentinelImageView, sentinel.image);
    ui.setListItems(sentinelAccelView, sentinel.accelerometer);
    ui.setListItems(sentinelAnemoView, sentinel.anemometer);
    ui.setListItems(sentinelThermoView, sentinel.thermometer);
    ui.setListItems(sentinelGasView, sentinel.gas);
    ui.setListItems(sentinelVocView, sentinel.voc);
    ui.setListItems(sentinelGpsView, sentinel.gps);
    ui.setListItems(sentinelOrientationView, sentinel.orientation);
    ui.setListItems(sentinelPowerView, sentinel.power);
}

function clearDataViews() {
    ui.clearList(sentinelImageView);
    ui.clearList(sentinelAccelView);
    ui.clearList(sentinelAnemoView);
    ui.clearList(sentinelThermoView);
    ui.clearList(sentinelAlarmView);
    ui.clearList(sentinelGasView);
    ui.clearList(sentinelVocView);
    ui.clearList(sentinelGpsView);
    ui.clearList(sentinelOrientationView);
    ui.clearList(sentinelPowerView);
}

function selectImage(event) {
    let e = event.detail.curSelection;
    selectedImage = e;

    if (e) {
        if (e.window) {
            ui.raiseWindowToTop(e.window);
        }
    }
}
