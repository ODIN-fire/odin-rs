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
var wsUrl = getWsUrl();

var isShutdown = false;
var connectDate = undefined; // time when last (re-)connection was established

// message stats (to assess health status)
var lastMsgDate = undefined; // last timestamp (epoch millis) when we received a message
var lastLatency = 0; // in millis (only measured for pings)
var lastMsgCount = 0; // number of total messages received (including pings so should be increasing)

// wsHandlers is a map object from module-names to handler functions.
// each handler function takes the msg name and the payload object as arguments:
//      `function (msgName, msgObject) {...}`
// handler functions have to be registered by JS modules during initialization with the `addWsHandler(k,v)` function
var wsHandlers = new Map();

window.addEventListener('unload', shutdown);

function getWsUrl() {
    // firefox does reject the url if given as a relative path ("./ws"), probably because it doesn't handle the
    // protocol replacement. We have to construct it explicitly from the document URL

    let url = new URL(window.location.href);
    let protocol = url.protocol == "https:" ? "wss:" : "ws:";
    let host = url.host;
    let path = url.pathname;

    return `${protocol}//${host}${path}/ws`
}

export function addWsHandler(modName,newHandler) {
    wsHandlers.set( modName, newHandler);
}

//--- these can all be used to assess connectivity status
export function isConnected () { return (ws != undefined); }

// to be called from ws handlers to check if data initialization messages are due to a reconntect, i.e. whould 
// purge old state before processing the incoming message. To do this the caller has to keep track of 
// connectDates
export function getConnectDate() { return connectDate; }
export function getLastMsgDate() { return lastMsgDate; }
export function getLastMsgCount() { return lastMsgCount; }

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
    if (ws) {
        let msg = {};
        msg.mod = modPath;
        msg[msgType] = msgData;

        let json = JSON.stringify(msg);
        ws.send(json);
    } else {
        console.log("sendWsMessage() error: no WebSocket");
    }
}

// high level re-init registration for service modules that need to re-initialize if the document was hidden and becomes visible again
// use the optional visChangeFunc function argument if the caller has to modify its state before being reinitialized (e.g. to preserve
// selections)
export function reInitializeOnVisibilityChange (modPath, visChangeFunc=null) {
    console.log("registering visibility change initialization for ", modPath);
    document.addEventListener('visibilitychange', (e)=> {
        let isVisible = !document.hidden;
        if (visChangeFunc) {
            visChangeFunc(isVisible); // allow caller to set some state based on document visibility status
        }
        if (isVisible) {
            if (ws) { // if there is no websocket we will get re-initialized anyways
                console.log("sending websocket reinitialization request for ", modPath);
                sendWsReinitializeMessage(modPath);
            }
        }
    });
}

// send a system message to request re-initialization through the existing websocket connection
// use this if the requesting service has to obtain data updates after a visibilitychange event even though the websocket is still open
// note this is only sent to the corresponding SpaService.
// Use this if the caller registers for visibilitychange events itself (e.g. to track hidden status)
export function sendWsReinitializeMessage (modPath) {
    if (ws) {
        let msg = {mod: modPath, __system__: "reinitialize"};
        let json = JSON.stringify(msg);
        ws.send(json);
    } else {
        console.log("sendWsReinitializeMessage() error: no WebSocket");
    }
}

export function shutdown() {
    console.log("closing websocket...");
    isShutdown = true;
    if (ws) ws.close();
}

function connect () {
    if (wsUrl) {
        if ("WebSocket" in window) {
            if (!ws) {
                console.log("initializing websocket: " + wsUrl);            
                ws = new WebSocket(wsUrl);

                ws.onopen = function() {
                    connectDate = Date.now();
                };

                ws.onmessage = function(evt) {
                    lastMsgDate = Date.now();
                    lastMsgCount++;

                    let data = evt.data.toString();
                    if (data.startsWith("__ping__")) { // this is a heartbeat (not JSON)
                        let timestamp = Number.parseInt( data.substring(9));
                        if (!Number.isNaN(timestamp)) {
                            lastLatency = lastMsgDate - timestamp;
                        }

                    } else { // everything else is supposed to be a structured JSON message with a JS_MODULE receiver spec
                        try {
                            let msg = JSON.parse(data);
                            handleServerMessage(msg);
                        } catch(error) {
                            console.error(error);
                        }
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
                    ws = undefined;
                };
            } else {
                console.log("WebSocket still open, ignoring connection request");
            }
        } else {
            console.log("WebSocket NOT supported by your Browser!");
        }
    } else {
        console.log("no WebSocket url set");
    }
}

function checkReconnect () {
    if (!ws && !document.hidden) {
        console.log("reconnecting WebSocket..");
        connect();
    }
}


// execute after all js modules have initialized to make sure handlers have been set
// this is crucial for modules that get initialization data through the ws - as soon as we are connected this is sent by the server
export function postInitialize() {
    connect();

    // reconnect if the websocket was closed due to a suspend or inactivity
    document.addEventListener('visibilitychange', checkReconnect);
}