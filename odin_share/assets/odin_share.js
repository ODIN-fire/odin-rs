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
import * as odinCesium from "../odin_cesium/odin_cesium.js";
import * as util from "../odin_server/ui_util.js";
import { ExpandableTreeNode } from "../odin_server/ui_data.js";

const MOD_PATH = "odin_share::share_service::ShareService";
const NAME_PLACEHOLDER = '◻︎';
const DEFAULT_TYPE = "Json";
const LOCAL_SHARE = "LOCAL_SHARE";

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
var complChoice = undefined;
var typeChoice = undefined;

var typeCandidates = main.ALL_TYPES;

var messages = [];
var hasDataEntryChanged = false; // state to keep track if dataEntry has changed

var shareDataSource = new Cesium.CustomDataSource("share");
odinCesium.addDataSource(shareDataSource);

compileConfigGlobs();
createIcon();
createWindow();

var dirView = initDirView();
var ownRoleList = initOwnRoleList();
var extRoleList = initExtRoleList();
var msgList = initMsgList();

// we don't have other direct layer manager dependencies (e.g. odin_cesium) so we go through the main interface here
odinCesium.initLayerPanel("share", config, showShareLayer);
console.log("share layer initialized.");

//--- set the window.main.share interface object

class OdinShare extends main.Share {

    constructor() {
        super();
    }

    setSharedItem (key, type, data, isLocal=false, comment=null) {
        let value = new main.SharedValue( type, comment, data);
        let sharedItem = new main.SharedItem( key, isLocal, value);

        if (isLocal) { // notify shareHandlers right away
            handleSetShared( {key,value}, true);
        } else {
            ws.sendWsMessage( MOD_PATH, "setShared", sharedItem);
            // sharedItems will be updated when server responds in handleWsMessages()
        }
    }

    removeSharedItem (key) {
        let sharedItem = this._get(key);
        if (sharedItem) {
            if (sharedItem.isLocal) {
                handleRemoveShared( {key});
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
    for (var e of config.keyCompletions) {
        if (e.pattern) {
            e.glob = util.glob2regexp( e.pattern);
        }
    }

    for (var e of config.keyTypes) {
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
            ),
            ui.ColumnContainer() (
                (msgList = ui.List("share.msg.list", 8)),
                (msgEntry = ui.TextInput("send", "share.msg.entry", "25rem", {placeHolder: "enter message text", changeAction: sendMsg})),
                ui.RowContainer()(
                    ui.Button("send", sendMsg), 
                    ui.Button("clear all", clearMsgList)
                )
            )
        ),
        ui.Panel("item directory", true)(
                ui.RowContainer("start")(
                    ui.TreeList("share.dir.list", 15, "32rem", selectShareEntry),
                    ui.ColumnContainer("end")(
                        ui.Button("clear local", clearLocalItems, "6rem"),
                        ui.Button("store local", saveLocalItems, "6rem"),
                        ui.Button("load local", loadLocalItems, "6rem"),
                    )
                )
        ),
        ui.Panel("item editor", false)(
            ui.RowContainer()(
                ui.ColumnContainer()(
                    (keyEntry = ui.TextInput( "key","share.obj.key", "24rem", {isFixed: true, placeHolder: "enter item key", changeAction: keyChanged})),
                    (commentEntry = ui.TextInput( "comment", "share.obj.cmt", "24rem", {isFixed: true, placeHolder: "enter (optional) item comment"}))
                ),
                ui.ColumnContainer()(
                    (complChoice = ui.Choice( "compl", "share.obj.compl", completeKey)),
                    (typeChoice = ui.Choice( "type", "share.obj.type", selectType))
                )
            ),
            (dataEntry = ui.TextArea("share.obj.text", "35.4rem", "8lh", {isFixed: true, changeAction: dataChanged})),
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

function initDirView() {
    let view = ui.getList("share.dir.list");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "show", tip: "render selected item", width: "2.5rem", attrs:[], map: e=> itemRenderCb(e) },
            { name: "loc", tip: "item is local", width: "2rem", attrs: [], map: e => itemScope(e) },
            { name: "owner", tip: "owner of item", width: "6rem", attrs: [], map: e => itemOwner(e) },
            { name: "type", tip: "item type", width: "6rem", attrs: ["small"], map: e=> itemType(e) }
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
            { name: "message", tip: "message text", width: "24rem", attrs: [], map: e=>e.msg },
        ]);
    }
    return view;
}

function toggleShareLayer(event) {
    // nothing to toggle yet
}

function showShareLayer (cond) {
    shareDataSource.show = cond;
    odinCesium.requestRender();
}

function itemRenderCb (e) {
    return e && e.value ? ui.createCheckBox( isItemShowing(e), toggleShowItem) : "";
}

function isItemShowing (e) {
    return shareDataSource.entities.getById( e.key);
}

function toggleShowItem(event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let e = ui.getListItemOfElement(cb);

        if (ui.isCheckBoxSelected(cb)) {
            let entity = getItemEntity(e);
            if (entity) shareDataSource.entities.add( entity);
        } else {
            shareDataSource.entities.removeById( e.key);
        }
        odinCesium.requestRender();
    }
}

function getItemEntity (e) {
    if (e.value) {
        switch (e.value.type) {
            case "GeoPoint": case "GeoPoint3": return createPointEntity(e);
            case "GeoLine": return createLineEntity(e);
            case "GeoLineString": return createLineStringEntity(e);
            case "GeoRect": return createRectEntity(e);
            case "GeoPolygon": return createPolygonEntity(e);
            default: return null;
        }
    }  
}

// is this is a dir or a sealed prefix it is global. Otherwise check the 'global' property of a value
function itemScope (e) {
    return (e.value && e.isLocal) ? '⨁' : '';
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

//--- shared item selections

// tree list selection in dirView
function selectShareEntry(event) {
    let e = ui.getSelectedListItem(dirView);
    if (e) {
        let key = e.key;
        if (e.value) {
            ui.setField( keyEntry, key);
            ui.clearChoiceItems( complChoice); // nothing to complete
            ui.setChoiceItems( typeChoice, typeCandidates, 0);
            ui.setField( commentEntry, e.value.comment);
            setDataEntry( JSON.stringify(e.value.data, 0, 2));
            updateTypeCandidates(e);
            return;
        }
    } 

    // did we select a branch node
    let node = ui.getSelectedTreeNode( dirView);
    if (node) {
        let path = node.collectNamesUp('/');
        ui.setField( keyEntry, path);
        updateKeyCompletions( path);
        updateTypeCandidates();
        ui.clearTextAreaContent(dataEntry);
    }
}

function updateKeyCompletions(key) {
    if (key) {
        let choices = [];
        for (var e of config.keyCompletions) {
            if (e.glob && key.match(e.glob)) {
                e.completion.forEach( c=> choices.push(c));
            } 
        }
        if (choices.length > 0) {
            ui.setChoiceItems( complChoice, choices, 0);
            return;
        }
    }

    ui.clearChoiceItems( complChoice);
}

function updateTypeCandidates(selItem = null) {
    typeCandidates = selItem ? [selItem.value.type] : getTypeCandidates(ui.getFieldValue(keyEntry));

    if (typeCandidates.length > 0) {
        ui.setChoiceItems( typeChoice, typeCandidates, 0);
    } else {
        ui.clearChoiceItems( typeChoice);
    }
    updateEditorChoices();
}

function getTypeCandidates (key) {
    let candidates = [];
    if (key) {
        for (var e of config.keyTypes) {
            if (e.glob && key.match(e.glob)) candidates.push(e.type);
        }
    }
    return candidates;
}

// complChoice selection
function completeKey (event) {
    let completion = ui.getSelectedChoiceValue(event);
    if (completion) {
        let key = ui.getFieldValue(keyEntry) + completion;
        ui.setField( keyEntry, key);

        let i = key.indexOf(NAME_PLACEHOLDER);
        ui.focusField(keyEntry);
        if (i>=0) { // key not complete yet
            ui.selectFieldRange( keyEntry, i, i+1);
        } else {
            ui.triggerFieldChange(keyEntry);
        }
        updateTypeCandidates();
    } 
}

function selectType (event){
    let type = ui.getSelectedChoiceValue( typeChoice);
    if (type) setTemplate( type);
    updateEditorChoices();
}

function setTemplate (type) {
    let templ = config.typeTemplates.get(type);
    let json = JSON.stringify( templ, 0, 2);
    setDataEntry( json);
}

function setDataEntry (src) {
    ui.setTextAreaContent( dataEntry, src);
    hasDataEntryChanged = false;
}

// textarea change (triggered when loosing focus)
function dataChanged(event) {
    hasDataEntryChanged = true;
}

// triggered when keyEntry changed
function keyChanged (event) {
    let key = ui.getFieldValue(keyEntry);

    if (!ui.selectNodePath( dirView, key)) { // key was not in our current list
        updateKeyCompletions(key);
        updateTypeCandidates();
        ui.setField( commentEntry, null);

        if (typeCandidates.length == 0) {
            typeCandidates = main.ALL_TYPES;
            ui.setChoiceItems( typeChoice, typeCandidates, 0);
            setTemplate( typeCandidates[0]);

        } else if (typeCandidates.length == 1) {
            setTemplate( typeCandidates[0]);
        }
    }
}

function updateEditorChoices () {
    let selType = ui.getSelectedChoiceValue( typeChoice);
    if (selType) { // we only set editors once we know the type
        let editors = main.getShareEditorForItemType( selType);
        if (editors && editors.length > 0)  {
            ui.setChoiceItems( editorChoice, editors, 0);
            return;
        }
    }

    ui.clearChoiceItems( editorChoice);
}



// triggered when pressing "delete" button
function removeItem(event) {
    let key = ui.getNonEmptyFieldValue(keyEntry);
    let sharedItem = share.getSharedItem(key);
    if (sharedItem) {
        share.removeSharedItem(sharedItem.key);
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
    let dataType = ui.getSelectedChoiceValue( typeChoice);

    if (key && dataType && dataSrc.length > 0) {
        // todo - should also allow changes of comment or isLocal
        if (hasDataEntryChanged) { 
            try {
                let data = JSON.parse(dataSrc);
                let templ = config.typeTemplates.get(dataType);
                if (templ) {
                    if (!util.haveEqualKeys( data, templ)) {
                        window.alert("entered data does not correspond with template");
                        return;
                    }
                }

                share.setSharedItem( key, dataType, data, isLocal, comment);

            } catch (error) {
                window.alert("invalid JSON: " + error);
            }
        } else {
            window.alert("data was not changed");
        }
    } else {
        window.alert("missing shared item key, type or data");
    }
}

function saveLocalItems() {
    let sharedItems = share.findAllSharedItems( e=> e.isLocal);
    if (sharedItems.length > 0) {
        let json = JSON.stringify(sharedItems);
        localStorage.setItem( LOCAL_SHARE, json);
    }
}

function loadLocalItems() {
    let json = localStorage.getItem(LOCAL_SHARE);
    if (json) {
        let sharedItems = JSON.parse(json);
        if (sharedItems && sharedItems.length > 0){
            for (let e of sharedItems) {
                share.setSharedItem(e.key, e.value.type, e.value.data, e.isLocal, e.value.comment);
            }
        }
    }
}

function clearLocalItems() {
    let sharedItems = share.findAllSharedItems( e=> e.isLocal);
    for (let e of sharedItems) {
        share.removeSharedItem(e.key);
    }
}

// this is how we get data and/or sync operations from the server
function handleWsMessages(msgType, msg) {
    switch (msgType) {
        case "initSharedItems": handleInitSharedItems(msg); break;
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
    let key = msg.key;
    let value = msg.value;
    let sharedItem = new main.SharedItem(key,isLocal,value);
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

function handleInitSharedItems(o) { 
    let items = config.keyCategories.slice(); // add the category entries

    // we get this as a map (JS object with keys as properties)
    for (var e of Object.entries(o)) { // add the values from the server
        let item = new main.SharedItem( e[0], false, e[1]);
        items.push(item);

        share._set( item.key, item); // store the item in our share object
    }

    items.sort( (a,b)=> a.key.localeCompare(b.key));

    let tree = ExpandableTreeNode.from( items, e=>e.key );
    ui.setTree( dirView, tree);

    main.notifyShareHandlers( main.SHARE_INITIALIZED);
}


function runSelectedEditor(event) {
    function valueCallback (data) {
        let src = JSON.stringify(data, 0, 2);

        setDataEntry( src);
        hasDataEntryChanged = true;
    }

    let e = ui.getSelectedChoiceValue(editorChoice);
    if (e) {
        let key = ui.getNonEmptyFieldValue(keyEntry);
        if (isValidItemKey(key)) {
            ui.selectChoiceItem( typeChoice, e.type); // if we run the editor we select a type
            e.editor( valueCallback);
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

function createPointEntity(e) {
    let d = e.value.data;
    return new Cesium.Entity({
        id: e.key,
        name: e.key,
        position: Cesium.Cartesian3.fromDegrees( d.lon, d.lat, 0),
        label: entityLabel(e.key),
        point: {
            pixelSize: config.pointSize,
            color: config.color,
            outlineColor: config.outlineColor,
            outlineWidth: config.outlineWidth,
            //distanceDisplayCondition: config.pointDC, 
        }
    });
}

function createLineEntity(e) {
    let d = e.value.data;
    let points = [
        Cesium.Cartesian3.fromDegrees( d.start.lon, d.start.lat),
        Cesium.Cartesian3.fromDegrees( d.end.lon, d.end.lat),
    ];
    let midPoint =  Cesium.Cartesian3.fromDegrees( (d.start.lon + d.end.lon)/2, (d.start.lat + d.end.lat)/2);

    return new Cesium.Entity({
        id: e.key,
        name: e.key,
        position: midPoint,
        label: entityLabel(e.key),
        polyline: {
            positions: points,
            clampToGround: true,
            material: color,
            width: config.lineWidth
        }
    });
}

function createLineStringEntity(e){
    let d = e.value.data;
    let points = d.points.map( p=> Cesium.Cartesian3.fromDegrees( p.lon, p.lat));
    let midPoint = points[points.length/2];

    return new Cesium.Entity({
        id: e.key,
        name: e.key,
        position: midPoint,
        label: entityLabel(e.key),
        polyline: {
            positions: points,
            clampToGround: true,
            material: color,
            width: config.lineWidth
        }
    });
}

function createRectEntity(e) {
    let d = e.value.data;
    let vertices = [
        Cesium.Cartesian3.fromDegrees( d.west, d.south),
        Cesium.Cartesian3.fromDegrees( d.west, d.north),
        Cesium.Cartesian3.fromDegrees( d.east, d.north),
        Cesium.Cartesian3.fromDegrees( d.east, d.south)
    ];
    let center =  Cesium.Cartesian3.fromDegrees( (d.east + d.west)/2, (d.north + d.south)/2);

    return new Cesium.Entity({
        id: e.key,
        name: e.key,
        position: center,
        label: entityLabel(e.key),
        polygon: {
            hierarchy: vertices,
            heightReference: Cesium.CLAMP_TO_GROUND,
            fill: true,
            material: config.fillColor,
            outlineColor: config.outlineColor,
            outlineWidth: config.outlineWidth,
        }
    });
}

function createPolygonEntity(e){
    let d = e.value.data;
    let exterior = d.exterior.map( p=> Cesium.Cartesian3.fromDegrees( p.lon, p.lat));
    let cp = util.centerLonLat(d.exterior);
    let center = Cesium.Cartesian3.fromDegrees( cp.lon, cp.lat);

    return new Cesium.Entity({
        id: e.key,
        name: e.key,
        position: center,
        label: entityLabel(e.key),
        polygon: {
            hierarchy: exterior,
            heightReference: Cesium.CLAMP_TO_GROUND,
            fill: true,
            material: config.fillColor,
            outlineColor: config.outlineColor,
            outlineWidth: config.outlineWidth,
        }
    });
}

function entityLabel (key) {
    return {
        text: key,
        scale: 0.8,
        horizontalOrigin: Cesium.HorizontalOrigin.LEFT,
        verticalOrigin: Cesium.VerticalOrigin.TOP,
        font: config.labelFont,
        fillColor: config.color,
        showBackground: true,
        backgroundColor: config.labelBackground,
        pixelOffset: config.labelOffset,
        distanceDisplayCondition: config.labelDC,
    };
}