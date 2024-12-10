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

// this module opens a websocket and listens for JSON messages of the form
// { "mod": "<module-path>", "<msg>": <payload-object> }

var ws = undefined;
var wsUrl = "./ws";
var isShutdown = false;

// wsHandlers is a map object from module-names to handler functions.
// each handler function takes the msg name and the payload object as arguments:
//      `function (msgName, msgObject) {...}`
// handler functions have to be registered by JS modules during initialization with the `addWsHandler(k,v)` function
var wsHandlers = new Map();

window.addEventListener('unload', shutdown);

export function addWsHandler(modName,newHandler) {
    wsHandlers.set( modName, newHandler);
}

// messages have the format { "mod": "<module-path>", "<MsgType>": <payload-object> }
// note that MsgType is an uppercase typename as it is directly derived from the respective server type
function handleServerMessage(msg) {
    //console.log(JSON.stringify(msg));
    let modName = msg.mod;
    if (modName) {
        let handlerFunc = wsHandlers.get(modName);
        if (handlerFunc) {
            let msgName = Object.keys(msg)[1]; // 2nd property is the payload message
            handlerFunc( msgName, Object.values(msg)[1]);
        } else {
            console.log("no module handler for message: ", msg);
        }

    } else {
        console.log("malformed websocket message: ", msg);
    }
}

export function sendWsMessage (modPath, msgType, msgData) {
    let msg = {};
    msg.mod = modPath;
    msg[msgType] = msgData;

    let json = JSON.stringify(msg);

    ws.send(json);
}

export function shutdown() {
    console.log("closing websocket...");
    isShutdown = true;
    if (ws) ws.close();
}

// execute after all js modules have initialized to make sure handlers have been set
// this is crucial for modules that get initialization data through the ws - as soon as we are connected this is sent by the server
export function postInitialize() {
    if (wsUrl) {
        if ("WebSocket" in window) {
            console.log("initializing websocket: " + wsUrl);            
            ws = new WebSocket(wsUrl);

            ws.onopen = function() {
                // nothing yet
            };

            ws.onmessage = function(evt) {
                try {
                    let data = evt.data.toString();
                    let msg = JSON.parse(data);
                    handleServerMessage(msg);
                } catch (error) {
                    console.log(error);
                    console.log("msg-data: ", evt.data.toString());
                }
            };

            ws.onerror = function(evt) {
                if (!isShutdown) {
                    console.log('websocket error: ', evt);
                    // TODO - if we get a 'WebSocket is already in CLOSING or CLOSED state' outside of a shutdown we should try to reconnect
                }
            };

            ws.onclose = function() {
                console.log("connection is closed.");
            };

        } else {
            console.log("WebSocket NOT supported by your Browser!");
        }
    } else {
        console.log("no WebSocket url set");
    }
}