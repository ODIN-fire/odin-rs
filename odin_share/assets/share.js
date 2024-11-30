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

// this module is a common service for others that want to share data and sync views

import * as ws from "../odin_server/ws.js";
import * as util from "../odin_server/ui_util.js";

const MOD_PATH = "odin_server::ShareService";

//--- the shared data types (not exported)
var point2d = Map.new();
var point3d = Map.new();
var bbox = Map.new();
//... and more to follow

var syncHandlers = [];

ws.addWsHandler( MOD_PATH, handleWsMessages);

//--- end init

// this is how we get data and/or sync operations from the server
function handleWsMessages(msgType, msg) {
    switch (msgType) {
        case "point2dList": msg.forEach( p=> point2d.set( p.name, p)); break;
        case "point3dList": msg.forEach( p=> point3d.set( p.name, p)); break;
        case "bboxList": msg.forEach( p=> bbox.set( p.name, p)); break;

        case "newPoint2d": 
            point2d.set( msg.name, msg);
            syncHandlers.forEach( h => h(msg));
            break;

        case "newPoint3d": 
            point3d.set( msg.name, msg);
            syncHandlers.forEach( h => h(msg));
            break;

        case "newBbox":
            bbox.set( msg.name, msg);
            syncHandlers.forEach( h => h(msg));
            break;

        // poly-lines and polygons to follow
    }
}

export function addSyncHandler(newHandler) {
    syncHandlers.push( newHandler);
}

// this is called by other JS modules
export function sharePoint2d (p) {
    let msg = {};
    msg.newPoint2d = p;

    syncHandlers.forEach( h => h(msg)); // send a 'newPoint2d' message to other modules

    p.pending = true; // store that we sent this to the server but haven't heard back yet
    ws.sendWsMessage( MOD_PATH, msg); // send it to the server
}

export function hasPoint2d (name, group, category) {
    let p = point2d.get(name);
    if (p) {
        if (!matchesGroup( p.group, group)) return false;
        if (!matchesCategory( p.category, category)) return false;
        return true;
    } else {
        return false;
    }
}

export function getPoint2d (name, group, category) {
    return getItem(point2d, name, group, category);
}

function getItem (map, name, group, category) {
    let e = map.get(name);
    if (e && matchesGroup(e,group) && matchesCategory(e,category)){
        return e;
    } else {
        return null;
    }
}

function matchesGroup (e, group) {
    return group ? (e.group == group) : true;
}

function matchesCategory (e, category) {
    return category ? (e.category == category) : true;
}