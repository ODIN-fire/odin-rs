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
import * as main from "../odin_server/main.js";
import * as ui from "../odin_server/ui.js";
import * as ws from "../odin_server/ws.js";
import * as util from "../odin_server/ui_util.js";
import { ExpandableTreeNode } from "../odin_server/ui_data.js";

const MOD_PATH = "odin_share::share_service::ShareService";
const NAME_PLACEHOLDER = '◻︎';
const DEFAULT_TYPE = "Json";

var sharedItems = new Map();

var shareHandlers = []; // the list of share message handlers set by other modules
var shareEditors = new Map(); // item_type -> {label,editor} map populated by client modules

ws.addWsHandler( MOD_PATH, handleWsMessages); // we process websocket messages for MOD_PATH

// UI elements we frequently use
var keyEntry = undefined;
var commentEntry = undefined;
var dataEntry = undefined;
var editorChoice = undefined;
var deleteBtn = undefined;
var saveBtn = undefined;
var localCb = undefined;

var hasDataEntryChanged = false; // state to keep track if dataEntry has changed

compileConfigGlobs();
createIcon();
createWindow();

var dirView = initDirView();
var completionList = initSuffixList();

setObjButtonsDisabled(true);

//--- the window.main.share interface - this is how other modules can get/set shared values and register for change notifications

var share = {
    addShareHandler: function (newHandler) {
        shareHandlers.push( newHandler);
    },

    addShareEditor: function  (dataType, label, editorFunc) {
        let editorEntry = {label: label, editor: editorFunc};
    
        let editors = shareEditors.get(dataType);
        if (editors) {
            editors.push( editorEntry);
        } else {
            shareEditors.set( dataType, [editorEntry]);
        }
    },

    getShared: function (key) {
        return sharedItems.get( key);
    },

    getAllMatching: function (regex) {
        let matching = [];
        for (e of sharedItems.entries) {
            if (e[0].match(regex)) matching.push(e);
        }
        matching.sort( (a,b) => a.localeCompare(b)); 
        return matching;
    },

    findAll: function (pred) {
        let matching = [];
        for (e of sharedItems.entries) {
            if (pred(e)) matching.push(e);
        }
        matching.sort( (a,b) => a.localeCompare(b)); 
        return matching;
    },

    setShared: function (key, type, data, isLocal=false, comment=null) {
        let value = { type, comment, data };
        let sharedItem = { key, value };

        if (isLocal) { // notify shareHandlers right away
            handleSetShared( {setShared: sharedItem}, true);
        } else {
            ws.sendWsMessage( MOD_PATH, "setShared", sharedItem);
            // sharedItems will be updated when server responds in handleWsMessages()
        }
    },

    removeShared: function (sharedItem) {
        if (sharedItem) {
            let key = sharedItem.key;
            if (sharedItem.isLocal) {
                handleRemoveShared( {removeShared: key});
            } else {
                ws.sendWsMessage( MOD_PATH, "removeShared", {key});
            }
        }
    }
};

main.exportObjToMain( 'share', share);

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
                (completionList = ui.List("share.suffix.list", 15, selectSuffix))
            )
        ),
        ui.Panel("edit item", true)(
            (keyEntry = ui.TextInput( "key","share.obj.key", "30rem", {isFixed: true, placeHolder: "enter item key", changeAction: keyChanged})),
            (commentEntry = ui.TextInput( "comment", "share.obj.cmt", "30rem", {isFixed: true, placeHolder: "enter (optional) item comment"})),
            (dataEntry = ui.TextArea("share.obj.text", "30rem", "8lh", {isFixed: true, changeAction: dataChanged})),
            ui.RowContainer()(
                (localCb = ui.CheckBox("local", null, "share.obj.cb-local")),
                ui.HorizontalSpacer(4),
                (editorChoice = ui.Choice("editor")),
                ui.Button("run", runSelectedEditor),
                ui.HorizontalSpacer(4),
                (deleteBtn = ui.Button("delete", removeItem)),
                (saveBtn = ui.Button("save", saveItem))
            )
        )
    );
}

function setObjButtonsDisabled (isDisabled) {
    ui.setButtonDisabled(deleteBtn, isDisabled);
    ui.setButtonDisabled(saveBtn, isDisabled);
}

function initDirView() {
    let view = ui.getList("share.dir.list");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "pub", tip: "item is public", width: "2rem", attrs: ["small"], map: e => itemVisibility(e) },
            { name: "lck", tip: "locked", width: "2rem", attrs: [], map: e => itemMutability(e) },
            { name: "type", tip: "item type", width: "10rem", attrs: ["small"], map: e=> itemType(e) }
        ]);
    }
    return view;
}

function initSuffixList() {
    let view = ui.getList("share.suffix.list");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "completion", tip: "allowed completion for selected dir item", width: "10rem", attrs: ["alignLeft", "small"], map: e=>e },
        ]);
    }
    return view;
}

// is this is a dir or a sealed prefix it is global. Otherwise check the 'global' property of a value
function itemVisibility (e) {
    return (e.value && !e.isLocal) ? "✔︎" : "";
}

function itemMutability (e) {
    return (e.sealed) ? '🔒' : '';
}

// answer if this item is a sealed prefix (for global values), an extensible dir or a value
function itemType (e) {
    if (e.value) {
        return e.value.type;
    } else {
        return e.type;
    }
}

// triggered when keyEntry changed
function keyChanged (event) {
    let key = ui.getFieldValue(keyEntry);
    let si = sharedItems.get(key);

    if (si) {
        let json = JSON.stringify( si.value, 0, 2);
        setDataEntry( json);
        setObjButtonsDisabled(false);

    } else {
        checkTemplate(key);
    }
}

function lookupTypeInfo (key) {
    if (key) {
        for (var e of config.typeInfos) {
            if (e.glob && key.match(e.glob)) {
                return e;
            }
        }
    }
}

function checkTemplate (key) {
    let ti = lookupTypeInfo(key);
    if (ti && ti.template) {
        let json = JSON.stringify( ti.template, 0, 2);
        setDataEntry( json);
        setObjButtonsDisabled(false);
        setEditorChoice( ti.type);
    }
}

function setEditorChoice (itemType) {
    let editors = shareEditors.get(itemType);
    ui.setChoiceItems( editorChoice, editors, 0);
}

function toggleShareLayer(event) {
    // nothing to toggle yet
}

// tree list selection in dirView
function selectShareEntry(event) {
    let e = ui.getSelectedListItem(dirView);
    let key = null;

    if (e) {
        key = e.key;
        if (e.value) {
            ui.setField( commentEntry, e.value.comment);
            setDataEntry( JSON.stringify(e.value.data, 0, 2));
            setObjButtonsDisabled(false);

        } else { // dir entry
            ui.setField( commentEntry, null);
            setDataEntry(null);  
            setObjButtonsDisabled(true);
        }
    } else { // no item selected but check for branch nodes
        let node = ui.getSelectedTreeNode(dirView);
        key =  node ? node.collectNamesUp('/') : null;

        ui.setField( commentEntry, null);
        setDataEntry(null);
        setObjButtonsDisabled(true);
    }

    ui.setField( keyEntry, key);
    setSuffixList(key);

    let ti = lookupTypeInfo(key);
    if (ti) setEditorChoice(ti.type);
}

function setSuffixList (key) {
    if (key) {
        for (var e of config.completions) {
            if (e.glob && key.match(e.glob)) {
                ui.setListItems( completionList, e.completion);
                return;
            } 
        }
    }
    ui.setListItems( completionList, null);
}

// list selection in completionList
function selectSuffix(event) {
    let e = ui.getSelectedListItem(completionList);
    if (e) {
        let key = ui.getFieldValue(keyEntry) + e;
        let i = key.indexOf(NAME_PLACEHOLDER);
        
        ui.setField(keyEntry, key);
        ui.focusField(keyEntry);
        if (i>=0) { // key not complete yet
            ui.selectFieldRange( keyEntry, i, i+1);

        } else {
            checkTemplate(key);
            setObjButtonsDisabled(false);
        }
    }
}

function setDataEntry (src) {
    ui.setTextAreaContent( dataEntry, src);
    hasDataEntryChanged = false;
}

// textarea change (triggered when loosing focus)
function dataChanged(event) {
    hasDataEntryChanged = true;
}

// triggered when pressing "delete" button
function removeItem(event) {
    let key = ui.getNonEmptyFieldValue(keyEntry);
    let sharedItem = sharedItems.get(key);
    if (sharedItem) {
        share.removeShared(sharedItem);
    } else {
        window.alert("missing key or item not shared");
    }
}

// triggered when pressing "save" button
function saveItem(event) {
    let key = ui.getNonEmptyFieldValue(keyEntry);
    let comment = ui.getNonEmptyFieldValue(commentEntry);
    let dataSrc = ui.getTextAreaContent( dataEntry);
    let isLocal =  ui.isCheckBoxSelected( localCb);

    if (key && dataSrc.length > 0) {
        // todo - should also allow changes of comment or isLocal
        if (hasDataEntryChanged) { 
            try {
                let data = JSON.parse(dataSrc);
                let dataType = DEFAULT_TYPE;

                let ti = lookupTypeInfo(key);
                if (ti) {
                    dataType = ti.type;
                    if (ti.template) {
                        if (!util.haveEqualKeys( data, ti.template)) {
                            window.alert("entered data does not correspond with template");
                            return;
                        }
                    }
                }

                share.setShared( key, dataType, data, isLocal, comment);
                setObjButtonsDisabled(true);

            } catch (error) {
                window.alert("invalid JSON: " + error);
            }
        } else {
            window.alert("data was not changed");
        }
    } else {
        window.alert("missing shared item key or data");
    }
}

// this is how we get data and/or sync operations from the server
function handleWsMessages(msgType, msg) {
    switch (msgType) {
        case "initSharedItems": initSharedItems(msg); break;
        case "setShared": handleSetShared(msg, false); break;
        case "removeShared": handleRemoveShared(msg); break;
        default: console.log("ignoring unknown share message of type: ", msgType);
    }
}

function handleSetShared (msg, isLocal) {
    let sharedItem = { key: msg.key, isLocal, value: msg.value };
    sharedItems.set( sharedItem.key, sharedItem);

    ui.sortInTreeItem( dirView, sharedItem, sharedItem.key);
    notifyShareHandlers( {setShared: sharedItem} );
}

function handleRemoveShared (msg) {
    let sharedItem = sharedItems.get(msg.key);

    if (sharedItem) {
        let key = sharedItem.key;
        sharedItems.delete( key);

        ui.removeTreeItemPath( dirView, key);
        notifyShareHandlers( {removeShared: key});
    }
}

function initSharedItems(o) {
    let items = config.categories.slice();

    for (var e of Object.entries(o)) {
        let item = { key: e[0], isLocal: false, value: e[1] };
        items.push(item);

        sharedItems.set( item.key, item);
    }

    let tree = ExpandableTreeNode.from( items, e=>e.key );
    ui.setTree( dirView, tree);
}

/** function used by odin_share client modules to add type specific editors */
export function addShareEditor (dataType, label, editorFunc) {
    let editorEntry = {label: label, editor: editorFunc};

    let editors = shareEditors.get(dataType);
    if (editors) {
        editors.push( editorEntry);
    } else {
        shareEditors.set( dataType, [editorEntry]);
    }
}

function runSelectedEditor(event) {
    let e = ui.getSelectedChoiceValue(editorChoice);
    if (e) {
        let key = ui.getNonEmptyFieldValue(keyEntry);
        if (isValidItemKey(key)) {
            let data = e.editor();
            let src = JSON.stringify(data, 0, 2);

            setDataEntry( src);
            setObjButtonsDisabled(false);
            hasDataEntryChanged = true;

        } else {
            window.alert("no valid item key to edit");
        }
    } else {
        window.alert("no editor selected");
    }
}

function isValidItemKey (key) {
    return (key && key.length > 0 && key.indexOf(NAME_PLACEHOLDER) < 0);
}

function notifyShareHandlers (msg) {
    for (h of shareHandlers) {
        h(msg);
    }
}