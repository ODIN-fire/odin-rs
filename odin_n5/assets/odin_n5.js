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
import * as ui from "../odin_server/ui.js";                   // ODIN specific user interface library 
import * as ws from "../odin_server/ws.js";                   // websocket processing
import * as odinCesium from "../odin_cesium/odin_cesium.js";  // virtual globe rendering interface from odin_cesium


//--- 3. constants
const MOD_PATH = "odin_n5::goesr_n5::N5Service";   // the name of the associated odin-rs SpaService 


//--- 4. registering JS message handlers
ws.addWsHandler( MOD_PATH, handleWsMessages);                 // incoming websocket messages for MOD_PATH
//main.addShareHandler( handleShareMessage);                  // if module uses shared data items
//main.addSyncHandler( handleSyncMessage);                    // if module supports synchronization commands

//--- 5. data type definitions, module variable initialization




//--- 6. UI initialization
createIcon();
createWindow();                                               // UI window definition
initDataSetView();                                            // initialize UI window components and store references


console.log("odin_n5 initialized");

//--- 7. function definitions
function createIcon() {                                       // define UI window icon (used to automatically populate icon box)
    return ui.Icon("./asset/odin_n5/n5.svg", (e)=> ui.toggleWindow(e,'n5'));
}

function createWindow() {                                     // define UI window structure and layout
    return ui.Window("N5 Shield Sensors", "n5", "./asset/odin_n5/n5.svg")(
        ui.LayerPanel("n5", toggleShowN5),                    // panel with module information (should be first)
        ui.List("n5.devices", 10, selectDevice,null,null,zoomToDevice),
        ui.VarText(" ", "n5.description"),                    // name / description of selected device

        ui.Panel("data", true)(
            ui.TabbedContainer()(
                ui.Tab("alarm", false)( ui.List("n5.data.alarm", maxDataRows)),
                ui.Tab("readings", true)( ui.List("n5.data.readings", maxDataRows)),
                ui.Tab("heat", false)( ui.List("n5.data.heat", maxDataRows))
            )
        )
    );
}

function initDataSetView() {                                 // UI component init 
    let view = ui.getList("n5.dataSets");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [    // defines List columns and display
        ])
    }
}

function selectN5device(event) {                         // UI component callback
    let ds = event.detail.curSelection;
    if (ds) {
        selectedDataSet = ds;                                // update selected items
    }
}

function handleWsMessages(msgType, msg) {
    switch (msgType) {
        case "devices": handleN5Devices(msg); break;
    }
}

function handleN5Devices (hotspots) {
    dataSets.push( hotspots);                                // update data
    ui.setListItems( dataSetView, displayDataSets);          // update UI components displaying data
}

//function handleShareMessage (msg) {                          // shared data updates (local and between users)
//    if (msg.setShared) {
//        let sharedItem = msg.setShared;
//    }
//}

//function handleSyncMessage (msg) {                           // user interface sync (between users)
//    if (msg.updateCamera) {  }
//}

