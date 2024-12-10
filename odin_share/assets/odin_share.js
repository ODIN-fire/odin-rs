/*
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

import { config } from "./odin_share_config.js";
import * as ui from "../odin_server/ui.js";
import * as ws from "../odin_server/ws.js";
import * as util from "../odin_server/ui_util.js";
import { ExpandableTreeNode } from "../odin_server/ui_data.js";

const MOD_PATH = "odin_share::share_service::ShareService";

var sharedCategories = new Map();
var sharedItems = new Map();

var shareHandlers = []; // the list of share message handlers set by other modules

ws.addWsHandler( MOD_PATH, handleWsMessages); // we process websocket messages for MOD_PATH

var keyEntry = undefined;
var commentEntry = undefined;
var dataEntry = undefined;

compileConfigGlobs();
createIcon();
createWindow();

var dirView = initDirView();
var suffixList = initSuffixList();

//--- end init

function compileConfigGlobs() {
    for (var e of config.completions) {
        if (e.pattern) {
            e.glob = util.glob2regexp( e.pattern);
        }
    }

    for (var e of config.typeInfos) {
        if (e.pattern) {
            e.glob = util.glob2regexp( e.pattern);
        }
    }
}

function createIcon() {
    return ui.Icon("./asset/odin_share/share.svg", (e)=> ui.toggleWindow(e,'share'));
}

function createWindow() {
    return ui.Window("Shared Data", "share", "./asset/odin_share/share.svg")(
        ui.LayerPanel("share", toggleShareLayer),
        ui.Panel("item directory", true)(
            ui.RowContainer("start")(
                ui.TreeList("share.dir.list", 15, 25, selectShareEntry),
                (suffixList = ui.List("share.suffix.list", 15, selectSuffix))
            )
        ),
        ui.Panel("edit item", true)(
            (keyEntry = ui.TextInput( "key","share.obj.key", "30rem", {isFixed: true, placeHolder: "enter item key", changeAction: keyChanged})),
            (commentEntry = ui.TextInput( "comment", "share.obj.cmt", "30rem", {isFixed: true, placeHolder: "enter (optional) item comment"})),
            (dataEntry = ui.TextArea("share.obj.text", "30rem", "8lh", {isFixed: true})),
            ui.RowContainer()(
                ui.CheckBox("global", null, "share.obj.cb"),
                ui.HorizontalSpacer(4),
                ui.Button("delete", removeItem),
                ui.Button("save", saveItem)
            )
        )
    );
}

function initDirView() {
    let view = ui.getList("share.dir.list");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "vis", tip: "item visibility", width: "4rem", attrs: ["alignRight", "small"], map: e => itemVisibility(e) },
            ui.listItemSpacerColumn(),
            { name: "type", tip: "item value type", width: "10rem", attrs: ["small"], map: e=> itemCategory(e) }
        ]);
    }
    return view;
}

function initSuffixList() {
    let view = ui.getList("share.suffix.list");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "suffix", tip: "allowed suffix for dir item", width: "10rem", attrs: ["alignLeft", "small"], map: e=>e },
        ]);
    }
    return view;
}

// answer if this item is a sealed prefix (for global values), an extensible dir or a value
function itemCategory (e) {
    if (!e.value) { 
        return e.sealed ? '🔒' : '';
    } else {
        return e.value.type;
    }
}

function keyChanged (event) {
    let key = ui.getFieldValue(keyEntry);
    
    for (var e of config.typeInfos) {
        if (e.glob && key.match(e.glob)) {
            if (e.template) {
                let json = JSON.stringify( e.template, 0, 2);
                ui.setTextAreaContent( dataEntry, json);
                return;
            }
        }
    }
}

// is this is a dir or a sealed prefix it is global. Otherwise check the 'global' property of a value
function itemVisibility (e) {
    return (e.value && e.global) ? "pub" : "";
}

function toggleShareLayer(event) {
    // nothing to toggle yet
}

function selectShareEntry(event) {
    let e = ui.getSelectedListItem(dirView);
    let key = null;

    if (e) {
        key = e.key;
        if (e.value) {
            ui.setField( commentEntry, e.value.comment);
            ui.setTextAreaContent( dataEntry, JSON.stringify(e.value.data, 0, 2));
        } else { // dir entry
            ui.setField( commentEntry, null);
            ui.setTextAreaContent( dataEntry, null);   
        }
    } else { // no item selected but check for branch nodes
        let node = ui.getSelectedTreeNode(dirView);
        key =  node ? node.collectNamesUp('/') : null;
        ui.setField( commentEntry, null);
        ui.setTextAreaContent( dataEntry, null);    
    }

    ui.setField( keyEntry, key);
    setSuffixList(key);
}

function setSuffixList (key) {
    if (key) {
        for (var e of config.completions) {
            if (e.glob && key.match(e.glob)) {
                ui.setListItems( suffixList, e.completion);
                return;
            } 
        }
    }
    ui.setListItems( suffixList, null);
}

function selectSuffix(event) {
    let e = ui.getSelectedListItem(suffixList);
    if (e) {
        let key = ui.getFieldValue(keyEntry) + e;
        let i = key.indexOf('●');
        
        ui.setField(keyEntry, key);
        ui.focusField(keyEntry);
        if (i>=0) {
            ui.selectFieldRange( keyEntry, i, i+1);
        }
    }
}

function removeItem(event) {
    // TBD
}

function saveItem(event) {
    // TBD
}

// this is how we get data and/or sync operations from the server
function handleWsMessages(msgType, msg) {
    switch (msgType) {
        case "initSharedItems": initSharedItems(msg); break;
        default: console.log("ignoring unknown share message of type: ", msgType);
    }
}

export function addShareHandler(newHandler) {
    shareHandlers.push( newHandler);
}

function initSharedItems(o) {
    let items = config.categories.slice();

    for (var e of Object.entries(o)) {
        let item = { key: e[0], global: true, value: e[1] };
        items.push(item);
    }

    let tree = ExpandableTreeNode.from( items, e=>e.key );
    ui.setTree( dirView, tree);
}

