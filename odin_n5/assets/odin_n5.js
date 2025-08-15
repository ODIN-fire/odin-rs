/**
 * Copyright © 2025, United States Government, as represented by the Administrator of 
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

//--- 1. import JS module configuration
import { config } from "./odin_n5_config.js";              // associated static config for this module

//--- 2. import other JS modules
import * as main from "../odin_server/main.js";               // global functions (e.g. for data sharing)
import * as util from "../odin_server/ui_util.js";            // common, cross-module support functions
import * as data from "../odin_server/ui_data.js";
import * as ui from "../odin_server/ui.js";                   // ODIN specific user interface library 
import * as ws from "../odin_server/ws.js";                   // websocket processing
import * as odinCesium from "../odin_cesium/odin_cesium.js";  // virtual globe rendering interface from odin_cesium


//--- 3. constants
const MOD_PATH = "odin_n5::n5_service::N5Service";   // the name of the associated odin-rs SpaService 


//--- 4. registering JS message handlers
ws.addWsHandler( MOD_PATH, handleWsMessages);                 // incoming websocket messages for MOD_PATH
//main.addShareHandler( handleShareMessage);                  // if module uses shared data items
//main.addSyncHandler( handleSyncMessage);                    // if module supports synchronization commands

//--- 5. data type definitions, module variable initialization

class N5Assets {
    constructor(symbol, wind, info) {
        this.symbol = symbol; // billboard
        this.wind = wind; // barb (billboard)
        this.info = info; // info text (label)
    }

    showAssets (cond) {
        // TODO - is this right? we should just add/remove the sentinelDataSource
        if (this.symbol) this.symbol.show = cond;
        if (this.wind) this.wind.show = cond;
        if (this.info) this.info.show = cond;
    }
}

class N5Entry {
    constructor(device) {
        this.id = device.id;
        this.device = device;

        this.pos = Cesium.Cartesian3.fromDegrees( device.position.lon, device.position.lat);
        this.assets = this.createAssets();
    }

    hasRecentAlerts() {
        let alerts = this.device.alerts;
        return (alerts && alerts.length > 0 && isRecentAlert(alerts[0]));
    }

    isInactive() {
        let device = this.device;

        if (!device.online) return true;
        if (!device.active) return true;

        let data = this.latestData();
        if (data && (Date.now() - data.date) > config.inactiveDuration) return true;
        
        return false;
    }

    latestData () {
        let data = this.device.data;
        return (data.length == 0) ? null : data.last();
    }

    latestDateString() {
        let data = this.latestData();
        return data ?  util.toLocalMDHMSString(data.date) : "-";
    }

    createAssets () {
        let symEntity = this.createSymbolEntity();
        let windEntity = this.creatwWindEntity();
        let infoEntity = this.createInfoEntity(); 
        return new N5Assets(symEntity,windEntity,infoEntity);
    }

    createSymbolEntity () {
        let device = this.device;
        let hasRecentAlerts = this.hasRecentAlerts();

        let src = (hasRecentAlerts) ? "./asset/odin_n5/n5-alert-sym.png" : "./asset/odin_n5/n5-sym.png";
        let clr = (hasRecentAlerts) ? config.alertColor : config.color;

        let entity = new Cesium.Entity({
            id: device.id,
            position: this.pos,
            billboard: {
                image: src,
                distanceDisplayCondition: config.billboardDC,
                color: clr,
                heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
            },
            label: {
                text: device.id.toString(),
                scale: 0.8,
                horizontalOrigin: Cesium.HorizontalOrigin.LEFT,
                verticalOrigin: Cesium.VerticalOrigin.TOP,
                font: config.labelFont,
                fillColor: clr,
                showBackground: true,
                backgroundColor: config.labelBackground,
                pixelOffset: config.labelOffset,
                distanceDisplayCondition: config.labelDC,
                heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
                disableDepthTestDistance: Number.POSITIVE_INFINITY,
            },
            point: {
                pixelSize: config.pointSize,
                color: clr,
                outlineColor: config.pointOutlineColor,
                outlineWidth: config.pointOutlineWidth,
                distanceDisplayCondition: config.pointDC, 
            }
        });
        entity._uiDeviceEntry = this; // backlink for selection

        n5DataSource.entities.add(entity);
        return entity;
    }

    creatwWindEntity () {
        let device = this.device;
        let data = this.latestData();

        if (data) {
            let src = this.getWindSymbolSrc( data.wind_spd);
            if (src) {
                let clr = this.hasRecentAlerts() ? config.alertColor : config.color;

                let entity = new Cesium.Entity({
                    id: device.id + "-wind",
                    position: this.pos,
                    billboard: {
                        image: src,
                        distanceDisplayCondition: config.billboardDC,
                        color: clr,
                        heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
                        alignedAxis: Cesium.Cartesian3.UNIT_Z,
                        rotation: util.toRadians( 360 - data.wind_dir)
                    }
                });

                n5DataSource.entities.add(entity);
                return entity;
            }
        }
        return null;
    }

    getWindSymbolSrc (spd) {
        let s = 0;
        if (spd < 2.5) return null;
        else if (spd < 7.5)  s = 5;
        else if (spd < 12.5) s = 10;
        else if (spd < 17.5) s = 15;
        else if (spd < 22.5) s = 20;
        else if (spd < 27.5) s = 25;
        else if (spd < 32.5) s = 30;
        else if (spd < 37.5) s = 35;
        else s = 50;

        return `./asset/odin_n5/wind-${s}.png`;
    }

    createInfoEntity () {
        let device = this.device;
        let clr = this.hasRecentAlerts() ? config.alertColor : config.color;
        let infoText = this.deviceInfoText();

        if (infoText) {
            let entity = new Cesium.Entity({
                id: device.id + "-info",
                position: this.pos,
                label: {
                    text: infoText,
                    font: config.infoFont,
                    scale: 0.8,
                    horizontalOrigin: Cesium.HorizontalOrigin.LEFT,
                    verticalOrigin: Cesium.VerticalOrigin.TOP,
                    fillColor: config.color,
                    showBackground: true,
                    backgroundColor: config.labelBackground, // alpha does not work against model
                    outlineColor: clr,
                    outlineWidth: 1,
                    pixelOffset: config.infoOffset,
                    distanceDisplayCondition: config.infoDC,
                    heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
                    disableDepthTestDistance: Number.POSITIVE_INFINITY,
                }
            });
        
            n5DataSource.entities.add(entity);
            return entity;

        } else {
            return null;
        }
    }

    deviceInfoText () {
        let data = this.latestData();
        return data ? `${data.temperature} F\n${data.humidity} %\n${data.wind_dir} °\n${data.wind_spd} mph` : null;
    }
}

var n5DataSource = new Cesium.CustomDataSource("n5");
var deviceEntries = new Map();  // id -> device

var deviceView = undefined;
var selectedDeviceEntry = null;

var dataView = undefined;
var alertView = undefined;

//--- 6. UI initialization
odinCesium.addDataSource(n5DataSource);

createIcon();
createWindow();                                           
initDeviceView();
initDataView();                                           
initAlertsView();

odinCesium.setEntitySelectionHandler(n5DeviceSelection);
odinCesium.initLayerPanel("n5", config, showN5);
console.log("odin_n5 initialized");

//--- 7. function definitions
function createIcon() {                                     
    return ui.Icon("./asset/odin_n5/n5.svg", (e)=> ui.toggleWindow(e,'n5'));
}

function createWindow() {                                 
    return ui.Window("N5 Shield Sensors", "n5", "./asset/odin_n5/n5.svg")(
        ui.LayerPanel("n5", toggleShowN5),    
        
        ui.Panel("devices", true) (
            (deviceView = ui.List("n5.devices", 10, selectDeviceEntry,null,null,zoomToDevice)),
        ),
        ui.Panel("data", true)(
            (dataView = ui.List( "n5.data", 10)),
        ),
        ui.Panel("alerts", true)(
            (alertView = ui.List( "n5.alerts", 5))
        )
    );
}

function initDeviceView() {                               
    let view = ui.getList("n5.devices");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [    
            { name: "", width: "2.2rem", attrs: [], map: e => e.hasRecentAlerts() ?  "⚠️" : ""},
            { name: "id", width: "3rem", attrs: ["alignLeft"], map: e => e.id },
            { name: "type", width: "8rem", attrs:[], map: e => e.device.device_type.toLowerCase() },
            ui.listItemSpacerColumn(),
            { name: "stat", tip: "inactive/offline/overdue status", width: "2rem", attrs:["alignRight"], map: e => e.isInactive() ? "🔺" : "✓" },
            { name: "data", tip: "number of data points", width: "3rem", attrs:["fixed", "alignRight"], map: e => e.device.data.length },
            ui.listItemSpacerColumn(),
            { name: "last report", width: "9rem", attrs: ["fixed", "alignRight"], map: e => e.latestDateString() }
        ])
    }
}

function initDataView() {                               
    let view = ui.getList("n5.data");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [ 
            { name: "temp", tip: "temperature [F]", width: "3rem", attrs: ["fixed", "alignRight"], map: e => e.temperature },
            { name: "hum", tip: "humidity [%]", width: "2.5rem", attrs: ["fixed", "alignRight"], map: e => e.humidity },

            ui.listItemSpacerColumn(0.5),
            { name: "ir", tip: "heat index", width: "3rem", attrs: ["fixed", "alignRight"], map: e => util.f_1.format( e.ic_score) },
            { name: "smk", tip: "smoke index", width: "3rem", attrs: ["fixed", "alignRight"], map: e => util.f_1.format( e.smoke_index) },
            { name: "aqi", tip: "air quality index", width: "3rem", attrs: ["fixed", "alignRight"], map: e => util.f_1.format( e.air_quality) },

            ui.listItemSpacerColumn(0.5),
            { name: "dir", tip: "wind direction [°]", width: "2.5rem", attrs: ["fixed", "alignRight"], map: e => util.f_0.format( e.wind_dir) },
            { name: "spd", tip: "wind speed [mph]", width: "2rem", attrs: ["fixed", "alignRight"], map: e => util.f_0.format(e.wind_spd) },

            ui.listItemSpacerColumn(0.5),
            { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.date) }
        ])
    }
}

function initAlertsView() {                
    let view = ui.getList("n5.alerts");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [  
            { name: "alert", width: "15rem", attrs:[], map: e => alertType(e) },

            ui.listItemSpacerColumn(),
            { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.date) }
        ])
    }
}

function alertType (alert) {
    switch (alert.alert_type) {
        case 1: return "fire alert";
        case 2: return "fire warning";
        case 3: return "air quality";
        case 50: return "IR camera";
        case 51: return "gas discrepancy";
        case 52: return "particle discrepancy";
        case 100: return "system test 1";
        case 101: return "system test 2";
        case 102: return "system test 3";
        default: return "?";
    }
}

// list -> entity
function selectDeviceEntry(event) {        
    let de = event.detail.curSelection;
    if (de) {
        selectedDeviceEntry = de; 
        
        ui.setListItems( dataView, de.device.data.toReversed());
        ui.setListItems( alertView, de.device.alerts.toReversed());
    }
}

// entity -> list
function n5DeviceSelection() {
    let sel = odinCesium.getSelectedEntity();
    if (sel && sel._uiDeviceEntry) {
        if (!Object.is( sel._uiDeviceEntry,selectedDeviceEntry)) {
            ui.setSelectedListItem(deviceView, sel._uiDeviceEntry);
        }
    }
}

function showN5 (cond) { // triggered by panel
    n5DataSource.show = cond;
    odinCesium.requestRender();
}

function toggleShowN5(event) { // show action triggered by layer view (not panel)
    n5DataSource.show = !n5DataSource.show;
    odinCesium.requestRender();
}

function zoomToDevice (event) {
    let lv = ui.getList(event);
    if (lv) {
        let de = ui.getSelectedListItem(lv);
        if (de) {
            let position = de.device.position;
            let pos = Cesium.Cartographic.fromDegrees( position.lon, position.lat, config.zoomHeight);

            odinCesium.zoomTo( Cesium.Cartographic.toCartesian(pos));
            odinCesium.setSelectedEntity(de.assets.symbol);
        }
    }
}

function handleWsMessages(msgType, msg) {
    switch (msgType) {
        case "snapshot": handleSnapshotMsg(msg); break;
        case "update": handleUpdateMsg(msg); break;
    }
}

function handleSnapshotMsg (msg) {
    msg.devices.forEach( (device) => {
        device.data = data.CircularBuffer.fromArray( device.data, config.maxHistory);
        device.alerts = data.CircularBuffer.fromArray( device.alerts, config.maxHistory);

        let de = new N5Entry( device);
        de.date = msg.date;
        deviceEntries.set( de.id, de);
    });
    let deviceList = [...deviceEntries.values()].sort( compareDevices);
    ui.setListItems( deviceView, deviceList); 
}

function compareDevices (a,b) {
    if (a.id < b.id) return -1;
    else if (a.id > b.id) return 1;
    else return 0;
}

function handleUpdateMsg (msg) {
    msg.changes.forEach( (update) => {
        let id = update.id;
        let data = update.data;
        let alerts = update.alerts;

        let de = deviceEntries.get(id);
        if (de) {
            de.date = msg.date;
            de.device.data.push( data);
            if (alerts && alerts.length > 0) {
                alerts.forEach( (a)=> de.device.alerts.push( a));
            }

            ui.updateListItem( deviceView, de);
            if (Object.is( selectedDeviceEntry, de)) {
                ui.setListItems( dataView, de.device.data.toReversed());
                if (alerts && alerts.length > 0) ui.setListItems( alertView, de.device.alerts.toReversed());
            }
        }
    });

    ui.updateListItems( deviceView);
}

//function handleShareMessage (msg) {                          // shared data updates (local and between users)
//    if (msg.setShared) {
//        let sharedItem = msg.setShared;
//    }
//}

//function handleSyncMessage (msg) {                           // user interface sync (between users)
//    if (msg.updateCamera) {  }
//}

function isRecentAlert (alert) {
    return (Date.now() - alert.date) > config.maxAlertAge;
}