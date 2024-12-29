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

// TODO - still needs store/restore support for local shared items 

import { config } from "./odin_share_config.js";
import * as main from "../odin_server/main.js";
import * as ui from "../odin_server/ui.js";
import * as ws from "../odin_server/ws.js";
import * as util from "../odin_server/ui_util.js";
import { ExpandableTreeNode } from "../odin_server/ui_data.js";

const MOD_PATH = "odin_share::share_service::ShareService";
const NAME_PLACEHOLDER = '◻︎';
const DEFAULT_TYPE = "Json";

ws.addWsHandler( MOD_PATH, handleWsMessages); // we process websocket messages for MOD_PATH

// UI elements we frequently use
var keyEntry = undefined;
var commentEntry = undefined;
var dataEntry = undefined;
var editorChoice = undefined;
var deleteBtn = undefined;
var saveBtn = undefined;
var localCb = undefined;
var addRoleEntry = undefined;
var msgEntry = undefined;

var messages = [];
var hasDataEntryChanged = false; // state to keep track if dataEntry has changed

compileConfigGlobs();
createIcon();
createWindow();

var dirView = initDirView();
var completionList = initSuffixList();
var ownRoleList = initOwnRoleList();
var extRoleList = initExtRoleList();
var msgList = initMsgList();

setObjButtonsDisabled(true);

//--- set the window.main.share interface object

class OdinShare extends main.Share {

    constructor() {
        super();
    }

    setSharedItem (key, type, data, isLocal=false, comment=null) {
        let value = { type, comment, data };
        let sharedItem = { key, value };

        if (isLocal) { // notify shareHandlers right away
            handleSetShared( {setShared: sharedItem}, true);
        } else {
            ws.sendWsMessage( MOD_PATH, "setShared", sharedItem);
            // sharedItems will be updated when server responds in handleWsMessages()
        }
    }

    removeSharedItem (sharedItem) {
        if (sharedItem) {
            let key = sharedItem.key;
            if (sharedItem.isLocal) {
                handleRemoveShared( {removeShared: key});
            } else {
                ws.sendWsMessage( MOD_PATH, "removeShared", {key});
            }
        }
    }

    requestRole (newRole) {
        ws.sendWsMessage( MOD_PATH, "requestRole", newRole); // comes back as 'roleAccepted' or 'roleRejected'
    }

    releaseRole (role) {
        if (this._ownRoles.has(role)) {
            ws.sendWsMessage( MOD_PATH, "releaseRoles", [role]);
        }
    }

    publishRole (role, isPublishing) {
        if (super.publishRole( role, isPublishing)) {
            let msg = isPublishing ? "startPublishRole" : "stopPublishRole";
            ws.sendWsMessage( MOD_PATH, msg, role);
            return true;
        }
        return false;
    }

    publishCmd (cmd) {
        for (let r of this._ownRoles.values()) {
            if (r.isPublishing) {
                ws.sendWsMessage( MOD_PATH, "publishCmd", {role: r.role, cmd: JSON.stringify(cmd)});
            }
        }
    }

    publishMsg (role,msg) {
        let publishMsg = {role, msg, date: Date.now()}; // FIXME - this should use a global simClock
        if (this._ownRoles.get(role)) {
            ws.sendWsMessage( MOD_PATH, "publishMsg", publishMsg); // this sends it to subscribers
            handlePublishMsg( publishMsg); // we don't get this back as role owner
        }
    }

    subscribeToExtRole (role, isSubscribe) {
        if (isSubscribe){
            ws.sendWsMessage( MOD_PATH, "subscribeRole", role);
        } else {
            ws.sendWsMessage( MOD_PATH, "unsubscribeRole", role);
        }
        super.subscribeToExtRole(role, isSubscribe);
    }
}

let share = new OdinShare();
main.setShareObj(share);

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
    return ui.Window("Share", "share", "./asset/odin_share/share.svg")(
        ui.LayerPanel("share", toggleShareLayer),
        ui.Panel("roles/owners", false) (
            ui.RowContainer("start")(
                ui.ColumnContainer()(
                    (ownRoleList = ui.List("share.own-role.list", 3)),
                    (addRoleEntry = ui.TextInput("new", "share.own-role.entry", "8rem", {placeHolder: "enter new role", changeAction: addOwnRole})),
                    ui.RowContainer()(
                        ui.Button("add", addOwnRole), 
                        ui.Button("del", deleteRole),
                        ui.Button("clear", clearRoleSelection)
                    )
                ),
                ui.HorizontalSpacer(2),
                ui.ColumnContainer()(
                    (extRoleList = ui.List("share.ext-role.list", 8)),
                    ui.RowContainer()(
                        ui.Button("sub all", subscribeAll), 
                        ui.Button("clear sub", unsubscribeAll)
                    )
                )
            )
        ),
        ui.Panel("messages", false)(
            (msgList = ui.List("share.msg.list", 8)),
            (msgEntry = ui.TextInput("send", "share.msg.entry", "27rem", {placeHolder: "enter message text", changeAction: sendMsg})),
            ui.RowContainer()(
                ui.Button("send", sendMsg), 
                ui.Button("clear all", clearMsgList)
            )
        ),
        ui.Panel("item directory", true)(
            ui.RowContainer("start")(
                ui.TreeList("share.dir.list", 15, 25, selectShareEntry),
                (completionList = ui.List("share.suffix.list", 15, selectSuffix))
            )
        ),
        ui.Panel("item editor", false)(
            (keyEntry = ui.TextInput( "key","share.obj.key", "30rem", {isFixed: true, placeHolder: "enter item key", changeAction: keyChanged})),
            (commentEntry = ui.TextInput( "comment", "share.obj.cmt", "30rem", {isFixed: true, placeHolder: "enter (optional) item comment"})),
            (dataEntry = ui.TextArea("share.obj.text", "30rem", "8lh", {isFixed: true, changeAction: dataChanged})),
            ui.RowContainer()(
                (localCb = ui.CheckBox("local", null, "share.obj.cb-local")),
                ui.HorizontalSpacer(4),
                (editorChoice = ui.Choice("editor")),
                ui.Button("run", runSelectedEditor),
                ui.HorizontalSpacer(4),
                (deleteBtn = ui.Button("del", removeItem)),
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
            { name: "priv", tip: "item is private", width: "2rem", attrs: [], map: e => itemVisibility(e) },
            { name: "owner", tip: "owner of item", width: "6rem", attrs: [], map: e => itemOwner(e) },
            { name: "type", tip: "item type", width: "8rem", attrs: ["small"], map: e=> itemType(e) }
        ]);
    }
    return view;
}

function initSuffixList() {
    let view = ui.getList("share.suffix.list");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "completion", tip: "allowed completion for selected dir item", width: "8rem", attrs: ["alignLeft", "small"], map: e=>e },
        ]);
    }
    return view;
}


function initOwnRoleList() {
    let view = ui.getList("share.own-role.list");
    if (view) {
        function togglePublish(event) {
            let cb = ui.getCheckBox(event.target);
            if (cb) {
                let e = ui.getListItemOfElement(cb);
                if (e)  share.publishRole( e.role,  ui.isCheckBoxSelected(cb));              
            }
        }

        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "own role", tip: "", width: "8rem", attrs: ["alignLeft", "small"], map: e=>e.role },
            { name: "sub", tip: "number of external subscribers", width: "2.5rem", attrs: ["alignRight", "fixed"], map: e=> e.nSubscribers },
            ui.listItemSpacerColumn(),
            { name: "pub", tip: "is this role publishing", width: "2.5rem", attrs: [], map: e => ui.createCheckBox( e.isPublishing, togglePublish) }
        ]);
    }
    return view;
}

function initExtRoleList() {
    let view = ui.getList("share.ext-role.list");
    if (view) {
        function toggleSubscription(event) {
            let cb = ui.getCheckBox(event.target);
            if (cb) {
                let e = ui.getListItemOfElement(cb);
                if (e) share.subscribeToExtRole( e.role, ui.isCheckBoxSelected(cb));
            }
        }

        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "ext role", tip: "", width: "8rem", attrs: ["alignLeft", "small"], map: e=>e.role },
            { name: "pub", tip: "is role currently published", width: "2.5rem", attrs: [], map: e=> e.isPublishing ? '✓' : '' },
            { name: "sub", tip: "are we subscribed to role", width: "2.5rem", attrs: [], map: e => ui.createCheckBox( e.isSubscribed, toggleSubscription) }
        ]);
    }
    return view;
}

function initMsgList() {
    let view = ui.getList("share.msg.list");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "time", tip: "time of send", width: "5rem", attrs:["small", "fixed"], map: e=> util.toLocalTimeString(e.date) },
            { name: "role", tip: "role of msg sender", width: "4rem", attrs: ["alignLeft", "small"], map: e=>e.role },
            { name: "", tip:"", width: "1rem", attrs: [], map: e=> share._getOwnRole(e.role) ? '>' : '' },
            { name: "message", tip: "message text", width: "26rem", attrs: [], map: e=>e.msg },
        ]);
    }
    return view;
}

// is this is a dir or a sealed prefix it is global. Otherwise check the 'global' property of a value
function itemVisibility (e) {
    return (e.value && e.isLocal) ? '☒' : '';
}

function itemOwner (e) {
    return (e.value && e.value.owner) ? e.value.owner : '';
}

// answer if this item is a sealed prefix (for global values), an extensible dir or a value
function itemType (e) {
    if (e.value) {
        return e.value.type;
    } else {
        return e.type;
    }
}

function addOwnRole (event) {
    let newRole = ui.getFieldValue(addRoleEntry);
    if (newRole) {
        share.requestRole(newRole); // this will come back to us as a 'newOwner' message
    } else {
        window.alert("no role name provided");
    }
}

function deleteRole(event) {
    let role = share.selectedOwnRole();
    if (role) {
        share.releaseRole(role);
    }
}

function clearRoleSelection(event){
    ui.clearSelectedListItem( ownRoleList);
}

function subscribeAll (event) {
    for (let e of share._extRoles.values()) {
        share.subscribeToExtRole(e.role, true);
        ui.updateListItem(extRoleList, e);
    }
}

function unsubscribeAll (event) {
    for (let e of share._extRoles.values()) {
        share.subscribeToExtRole(e.role, false);
        ui.updateListItem(extRoleList, e);
    }
}

// triggered when keyEntry changed
function keyChanged (event) {
    let key = ui.getFieldValue(keyEntry);
    let si = share.getSharedItem(key);

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
    let editors = main.getShareEditorForItemType(itemType);
    ui.setChoiceItems( editorChoice, editors, 0);
}

function toggleShareLayer(event) {
    // nothing to toggle yet
}

function sendMsg(event) {
    let selRoleEntry = ui.getSelectedListItem( ownRoleList);
    if (selRoleEntry && selRoleEntry.isPublishing) {
        let msgText = ui.getFieldValue(msgEntry);
        if (msgText && msgText.length > 0) {
            share.publishMsg(selRoleEntry.role, msgText);
            ui.setField( msgEntry, null);
        }

    } else {
        window.alert("please select publishing role for message");
    }
}

function clearMsgList (event) {
    if (window.confirm("do you want to clear all messages?")) {
        messages = [];
        ui.clearList( msgList);
    }
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
    let sharedItem = share.getSharedItem(key);
    if (sharedItem) {
        share.removeSharedItem(sharedItem);
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

                share.setSharedItem( key, dataType, data, isLocal, comment);
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

        // own roles
        case "roleAccepted": handleRoleAccepted(msg); break;
        case "roleRejected": handleRoleRejected(msg); break;

        // ext roles
        case "initExtRoles": handleInitExtRoles(msg); break;
        case "extRoleAdded": handleExtRoleAdded(msg); break;

        case "rolesDropped": handleRolesDropped(msg); break;

        case "startPublish": handleExtRolePublished(msg, true); break;
        case "publishCmd": handlePublishCmd(msg); break;
        case "publishMsg": handlePublishMsg(msg); break;
        case "stopPublish": handleExtRolePublished(msg, false); break;

        case "updateRole": handleUpdateRole(msg); break;

        default: console.log("ignoring unknown share message of type: ", msgType);
    }
}

function handleSetShared (msg, isLocal) {
    let sharedItem = { key: msg.key, isLocal, value: msg.value };
    share._set( sharedItem.key, sharedItem);

    ui.sortInTreeItem( dirView, sharedItem, sharedItem.key);
    main.notifyShareHandlers( {setShared: sharedItem} );
}

function handleRemoveShared (msg) {
    let sharedItem = share.getSharedItem(msg.key);

    if (sharedItem) {
        let key = sharedItem.key;
        share._delete( key);

        ui.removeTreeItemPath( dirView, key);
        main.notifyShareHandlers( {removeShared: key});
    }
}

function handleRoleAccepted (roleEntry) {
    share._roleAccepted( roleEntry ); // we still get a separate 'rolePublished' when that happens
    ui.setListItems( ownRoleList, share._ownRolesList());
    ui.setField( addRoleEntry, null);
}

function handleRoleRejected (newRole) {
    window.alert("user role rejected: ", newRole);
}

function handleInitExtRoles (roleEntries) {
    share._initExtRoles(roleEntries);
    ui.setListItems( extRoleList, share._extRolesList());
}

function handleExtRoleAdded (roleEntry) {
    share._extRoleAdded( roleEntry);
    ui.setListItems( extRoleList, share._extRolesList());
}

function handleUpdateRole (roleEntry) {
    let res = share._updateRole(roleEntry);
    if (res.ownRolesChanged){
        let e = share._getOwnRole(roleEntry.role);
        ui.updateListItem( ownRoleList, e);
    } 
    if (res.extRolesChanged) {
        let e = share._getExtRole(roleEntry.role);
        ui.updateListItem( extRoleList, e);
    }
}

function handleRolesDropped (droppedRoles) {
    let res = share._dropRoles(droppedRoles);
    
    if (res.ownRolesChanged){ 
        ui.setListItems( ownRoleList, share._ownRolesList());
    }
    if (res.extRolesChanged) {
        ui.setListItems( extRoleList, share._extRolesList());
    }
}

function handleExtRolePublished(role, isPublishing) {
    let e = share._setExtRolePublished(role,isPublishing);
    if (e) {
        ui.updateListItem( extRoleList, e);
    }
}

// handle incoming publish cmd if we are subscribed to that role
function handlePublishCmd (publishedCmd) {
    let cmd = JSON.parse(publishedCmd.cmd);
    if (share._isSubscribedToExtRole( publishedCmd.role)) {
        main.notifySyncHandlers( cmd);
    }
}

function handlePublishMsg (publishedMsg){
    if (messages.length >= config.maxMessages) {
        messages.splice(0,1);
    }
    messages.push( publishedMsg);

    ui.setListItems( msgList, messages);
    ui.selectLastListItem( msgList);
}

function initSharedItems(o) {
    let items = config.categories.slice(); // add the category entries

    for (var e of Object.entries(o)) { // add the values from the server
        let item = { key: e[0], isLocal: false, value: e[1] };
        items.push(item);

        share._set( item.key, item);
    }

    let tree = ExpandableTreeNode.from( items, e=>e.key );
    ui.setTree( dirView, tree);
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
