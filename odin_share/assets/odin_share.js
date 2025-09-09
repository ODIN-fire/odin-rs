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
import { schema } from "./odin_share_schema.js";

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
var pendingSavedKeys = new Set();

var renderOpts = { ...config.render };

compileConfigGlobs();

createDataIcon();
createRoleIcon();

const viewer = await odinCesium.viewerReadyPromise; // Safari bug workaround - call before using odinCesium

createDataWindow();
initViewParameters();
createRoleWindow();

var dirView = initDirView();
var ownRoleList = initOwnRoleList();
var extRoleList = initExtRoleList();
var msgList = initMsgList();

var shareDataSource = new Cesium.CustomDataSource("share");
odinCesium.addDataSource(shareDataSource);

var selItem = undefined; // the (interactively) selected item

// we don't have other direct layer manager dependencies (e.g. odin_cesium) so we go through the main interface here
odinCesium.initLayerPanel("share", config, showShareLayer);
console.log("share layer initialized.");

//--- set the window.main.share interface object

/* #region Share object *********************************************************************************/

class OdinShare extends main.Share {

    constructor() {
        super();
    }

    setSharedItem (key, type, data, isLocal=false, comment=null) {
        if (type == main.JSON) data = JSON.stringify(data); // we store JSON as a generic string
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

/* #endregion share object */

function compileConfigGlobs() {
    for (var e of schema.keyCompletions) {
        if (e.pattern) e.glob = util.glob2regexp( e.pattern);
    }

    for (var e of schema.keyTypes) {
        if (e.pattern) e.glob = util.glob2regexp( e.pattern);
    }

    for (var e of schema.keyTemplates) {
        if (e.pattern) e.glob = util.glob2regexp( e.pattern);
    }
}

/* #region role window ******************************************************************************/

function createRoleIcon() {
    return ui.Icon("./asset/odin_share/share.svg", (e)=> ui.toggleWindow(e,'share.role'), "shared msgs");
}

function createRoleWindow() {
    return ui.Window("Messages", "share.role", "./asset/odin_share/share.svg")(
        // no layerPanel as we don't have any entities
        ui.RowContainer("start")(
            ui.ColumnContainer()(
                (ownRoleList = ui.List("share.role.own.list", 3)),
                ui.RowContainer()(
                    (addRoleEntry = ui.TextInput(null, "share.role.own.entry", "8.5rem", {placeHolder: "enter new role", changeAction: addOwnRole})),
                    ui.Button("+", addOwnRole), 
                    ui.Button("−", deleteRole),
                    ui.Button("∅", clearRoleSelection)
                )
            ),
            ui.HorizontalSpacer(1),
            ui.ColumnContainer()(
                (extRoleList = ui.List("share.role.ext.list", 8)),
                ui.RowContainer()(
                    ui.Button("sub all", subscribeAll), 
                    ui.Button("clear sub", unsubscribeAll)
                )
            )
        ),
        ui.ColumnContainer() (
            (msgList = ui.List("share.role.msg.list", 8)),
            ui.RowContainer()(
                ui.Button("clear all", clearMsgList),
                ui.HorizontalSpacer(1),
                ui.Button("send", sendMsg), 
                (msgEntry = ui.TextInput(null, "share.role.msg.entry", "23rem", {placeHolder: "enter message text", changeAction: sendMsg}))
            )
        )
    );
}

function initOwnRoleList() {
    let view = ui.getList("share.role.own.list");
    if (view) {
        function togglePublish(event) {
            let cb = ui.getCheckBox(event.target);
            if (cb) {
                let e = ui.getListItemOfElement(cb);
                if (e)  share.publishRole( e.role,  ui.isCheckBoxSelected(cb));              
            }
        }

        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "own role", tip: "", width: "9rem", attrs: ["alignLeft", "small"], map: e=>e.role },
            { name: "sub", tip: "number of external subscribers", width: "2.5rem", attrs: ["alignRight", "fixed"], map: e=> e.nSubscribers },
            ui.listItemSpacerColumn(),
            { name: "pub", tip: "is this role publishing", width: "2.5rem", attrs: [], map: e => ui.createCheckBox( e.isPublishing, togglePublish) }
        ]);
    }
    return view;
}

function initExtRoleList() {
    let view = ui.getList("share.role.ext.list");
    if (view) {
        function toggleSubscription(event) {
            let cb = ui.getCheckBox(event.target);
            if (cb) {
                let e = ui.getListItemOfElement(cb);
                if (e) share.subscribeToExtRole( e.role, ui.isCheckBoxSelected(cb));
            }
        }

        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "ext role", tip: "", width: "9rem", attrs: ["alignLeft", "small"], map: e=>e.role },
            { name: "pub", tip: "is role currently published", width: "2.5rem", attrs: [], map: e=> e.isPublishing ? '✓' : '' },
            { name: "sub", tip: "are we subscribed to role", width: "2.5rem", attrs: [], map: e => ui.createCheckBox( e.isSubscribed, toggleSubscription) }
        ]);
    }
    return view;
}

function initMsgList() {
    let view = ui.getList("share.role.msg.list");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "time", tip: "time of send", width: "5rem", attrs:["small", "fixed"], map: e=> util.toLocalTimeString(e.date) },
            { name: "role", tip: "role of msg sender", width: "4rem", attrs: ["alignLeft", "small"], map: e=>e.role },
            { name: "", tip:"", width: "1rem", attrs: [], map: e=> share._getOwnRole(e.role) ? '>' : '' },
            { name: "message", tip: "message text", width: "22rem", attrs: [], map: e=>e.msg },
        ]);
    }
    return view;
}

function addOwnRole (event) {
    let newRole = ui.getFieldValue(addRoleEntry);
    if (newRole) {
        share.requestRole(newRole); // this will come back to us as a 'newOwner' message
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

/* #endregion share window */

/* #region data window *******************************************************************************/

function createDataIcon() {
    return ui.Icon("./asset/odin_share/shape.svg", (e)=> ui.toggleWindow(e,'share.data'), "shared data");
}

function createDataWindow() {
    return ui.Window("Shared Data", "share.data", "./asset/odin_share/share.svg")(
        ui.LayerPanel("sharedData", toggleShareLayer),

        ui.ColumnContainer()(
            ui.TreeList("share.data.dir.list", 17, "33rem", selectShareEntry),
            ui.RowContainer("end")(
                ui.Button("clear local", clearLocalItems, "6rem"),
                ui.Button("store local", saveLocalItems, "6rem"),
                ui.Button("load local", loadLocalItems, "6rem"),
                ui.HorizontalSpacer(2),
                (deleteBtn = ui.Button("del", removeItem)),
                ui.HorizontalSpacer(2),
                ui.Button("∅", clearItemEntities)
            )
        ),
        ui.Panel("item", true)(
            ui.RowContainer()(
                (keyEntry = ui.TextInput( "key","share.data.obj.key", "20rem", {isFixed: true, placeHolder: "enter item key", changeAction: keyChanged})),
                (complChoice = ui.Choice( "compl", "share.data.obj.compl", completeKey, "8rem")),
            ),
            ui.RowContainer()(
                (editorChoice = ui.Choice("editor", "share.data.editor", null, "12rem")),
                ui.Button( "run", runSelectedEditor),
                ui.HorizontalSpacer(0.6),
                (localCb = ui.CheckBox("local", null, "share.data.obj.cb-local", true)),
                ui.HorizontalSpacer(0.4),
                (typeChoice = ui.Choice( "type", "share.data.obj.type", selectType, "8rem"))
            )
        ),
        ui.Panel("item source", false)(
            (commentEntry = ui.TextInput( "comment", "share.data.obj.cmt", "28.5rem", {isFixed: true, placeHolder: "enter (optional) item comment"})),
            (dataEntry = ui.TextArea("share.data.obj.text", "33rem", "8lh", {isFixed: true, changeAction: dataChanged})),
            ui.RowContainer("end")(
                (saveBtn = ui.Button("save", saveItem))
            )
        ),
        ui.Panel("view parameters", false)(
            ui.RowContainer()(
                ui.CheckBox("label stats", toggleLabelStats, null, renderOpts.labelStats),
                ui.CheckBox("fill", toggleFillAreas, null, renderOpts.fill),
                ui.HorizontalSpacer(0.6),
                ui.ColorField("color", "share.data.color", true, colorChanged),
            ),

            ui.Slider("fill alpha", "share.data.fill.alpha", fillAlphaChanged),
            ui.Slider("line width", "share.data.line_width", lineWidthChanged),
            ui.Slider("point size", "share.data.point_size", pointSizeChanged),
        )
    );
}

function initViewParameters () {
    let e = ui.getSlider("share.data.line_width");
    ui.setSliderRange(e, 1, 10, 1, util.f_0);
    ui.setSliderValue( e, renderOpts.lineWidth);

    e = ui.getSlider("share.data.fill.alpha");
    ui.setSliderRange(e, 0, 1, 0.1, util.f_1);
    ui.setSliderValue( e, renderOpts.fillAlpha);

    e = ui.getSlider("share.data.point_size");
    ui.setSliderRange(e, 1, 10, 1, util.f_0);
    ui.setSliderValue( e, renderOpts.pointSize);

    ui.setField( ui.getField("share.data.color"), renderOpts.color.toCssHexString());
}

function clearItemEntities () {
    shareDataSource.entities.removeAll();
    ui.updateListItems(dirView);
}

function toggleFillAreas (event) {
    renderOpts.fill = ui.isCheckBoxSelected(event);
    // TODO - re-render
}

function toggleLabelStats (event) {
    renderOpts.labelStats = ui.isCheckBoxSelected(event);
}

function colorChanged (event) {
    let clrSpec = event.target.value;
    if (clrSpec) {
        let color = Cesium.Color.fromCssColorString(clrSpec);
        if (color) {
            renderOpts.color = color;
            odinCesium.requestRender();
        } else { alert("invalid color spec: ", clrSpec) }
    }
}

function fillAlphaChanged (event) {
    renderOpts.fillAlpha = ui.getSliderValue(event.target);
    odinCesium.requestRender();
}

function lineWidthChanged (event) {
    renderOpts.lineWidth = ui.getSliderValue(event.target);
    odinCesium.requestRender();
}

function pointSizeChanged (event) {
    renderOpts.pointSize = ui.getSliderValue(event.target);
    odinCesium.requestRender();
}

function initDirView() {
    let view = ui.getList("share.data.dir.list");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "show", tip: "render selected item", width: "2.5rem", attrs:[], map: e=> itemRenderCb(e) },
            { name: "loc", tip: "item is local", width: "2rem", attrs: [], map: e => itemScope(e) },
            { name: "owner", tip: "owner of item", width: "7rem", attrs: [], map: e => itemOwner(e) },
            { name: "type", tip: "item type", width: "6rem", attrs: ["small"], map: e=> itemType(e) }
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


// is this is a dir or a sealed prefix it is global. Otherwise check the 'global' property of a value
function itemScope (e) {
    return (e.value && e.isLocal) ? 'loc' : '';
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

//--- shared item selections

// tree list selection in dirView
function selectShareEntry(event) {
    let e = ui.getSelectedListItem(dirView);
    if (e) { // data item selected
        if (e.value) {
            let key = e.key;

            selItem = e;

            ui.setField( keyEntry, key);
            ui.clearChoiceItems( complChoice); // nothing to complete
            ui.setChoiceItems( typeChoice, typeCandidates, 0);
            ui.setField( commentEntry, e.value.comment);
            setDataEntry( JSON.stringify(e.value.data, 0, 2));
            updateTypeCandidates(key,e);
            return;
        }
    } 

    selItem = null;

    let node = ui.getSelectedTreeNode( dirView);
    if (node) { // parent (non-data) node selected
        let key = node.collectNamesUp('/');
        ui.setField( keyEntry, key);
        updateKeyCompletions( key);
        updateTypeCandidates( key);
        ui.clearTextAreaContent(dataEntry);
        return;
    }

    // nothing selected at all, reset entries
    ui.setField( keyEntry, null);
    ui.clearChoiceItems( complChoice);
    ui.clearChoiceItems( typeChoice);
    ui.setField( commentEntry, null);
    ui.clearTextAreaContent(dataEntry);
}

function updateKeyCompletions(key) {
    if (key) {
        let choices = [];
        for (var e of schema.keyCompletions) {
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

function updateTypeCandidates(key, selItem = null) {
    typeCandidates = selItem ? [selItem.value.type] : getTypeCandidates(key);

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
        for (var e of schema.keyTypes) {
            if (e.glob && key.match(e.glob)) {
                for (let t of e.types) candidates.push(t);
            }
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

        updateKeyCompletions(key);
        updateTypeCandidates(key);
    } 
}

function selectType (event){
    let type = ui.getSelectedChoiceValue( typeChoice);
    let key = ui.getFieldValue( keyEntry);
    if (!selItem && type) setTemplate( key, type);
    updateEditorChoices();
}

function setTemplate (key, dataType) {
    let templ = getTypeTemplate( key, dataType);
    setDataEntry( templ);
}

function getTypeTemplate(key, dataType) {
    for (var e of schema.keyTemplates) { // overrides our static type templates
        if (e.glob && key.match(e.glob)) {
            return e.template;
        }
    }

    return main.typeTemplate( dataType);
}

function setDataEntry (src) {
    ui.setTextAreaContent( dataEntry, src);
}

// textarea change (triggered when loosing focus)
function dataChanged(event) {
}

// triggered when keyEntry changed
function keyChanged (event) {
    let key = ui.getFieldValue(keyEntry);

    if (!ui.selectNodePath( dirView, key)) { // key was not in our current list (NOTE this causes a select null)
        ui.setField( keyEntry, key);
        updateKeyCompletions(key);
        updateTypeCandidates(key);
        ui.setField( commentEntry, null);

        if (typeCandidates.length == 0) {
            typeCandidates = main.ALL_TYPES;
            ui.setChoiceItems( typeChoice, typeCandidates, 0);
            setTemplate( key, typeCandidates[0]);

        } else if (typeCandidates.length == 1) {
            setTemplate( key, typeCandidates[0]);
        }
    }
}

function updateEditorChoices () {
    let selType = ui.getSelectedChoiceValue( typeChoice);
    if (selType) { // we only set editors once we know the type
        let editors = main.getShareEditorEntriesForItemType( selType);
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
        if (sharedItem == selItem) selItem = undefined;
        share.removeSharedItem( sharedItem.key);
        shareDataSource.entities.removeById( sharedItem.key); // in case it is showing
        odinCesium.requestRender();

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
        try {
            let data = JSON.parse(dataSrc);
            let templ = (dataType == main.JSON) ? null : getTypeTemplate(key, dataType);
            if (!main.checkType( dataType, data, templ)) {
                window.alert("entered data does not correspond with template");
                return;
            }

            pendingSavedKeys.add( key);
            share.setSharedItem( key, dataType, data, isLocal, comment);

        } catch (error) {
            window.alert("invalid JSON: " + error);
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

/* #endregion data window */

/* #region data entities ****************************************************************************/

function runSelectedEditor(event) {
    let key = ui.getNonEmptyFieldValue(keyEntry);

    function valueCallback (data) {
        let src = JSON.stringify(data, 0, 2);
        setDataEntry( src);
        saveItem(null);
    }

    if (isValidItemKey(key)) {
        let e = ui.getSelectedChoiceValue(editorChoice);
        if (e) {
            shareDataSource.entities.removeById(key); // don't keep the entity while we are editing
            // TODO - we have to update the dir entry show status here

            ui.selectChoiceItem( typeChoice, e.type); // if we run the editor we select a type
            e.editor( selItem ? selItem.value.data : null, valueCallback);
        } else {
            window.alert("no editor selected");
        }
    } else {
        window.alert("no valid item key to edit");
    }
}

function isValidItemKey (key) {
    return (key && key.length > 0 && key.indexOf(NAME_PLACEHOLDER) < 0);
}

function getItemLabelText (e) {
    let s = (renderOpts.labelPath ? e.key : e.name());

    if (renderOpts.labelStats) {
        let data = e.value.data;
        switch (e.value.type) {
            case "GeoLine": s += lineStats(data); break;
            case "GeoLineString": s += polylineStats(data); break;
            case "GeoRect": s += rectStats(data); break;
            case "GeoPolygon": s += polygonStats(data); break;
            case "GeoCircle": s += circleStats(data); break;
        }
    }

    return s;
}

function lineStats (geoLine) {
    let dist = util.distanceBetweenGeoPoints( geoLine.start, geoLine.end);
    return `\n${util.lengthString( dist, odinCesium.isMetric)}`;
}

function polylineStats (geoLineString) {
    let dist = util.distanceOverGeoPoints(geoLineString.points);
    return `\n${util.lengthString( dist, odinCesium.isMetric)}`;
}

function rectStats (geoRect) {
    let isMetric = odinCesium.isMetric;
    let area = util.geoRectArea(geoRect);
    let h = util.distanceBetweenGeoPos( geoRect.west, geoRect.north, geoRect.west, geoRect.south);
    let w = area / h;
    let perim = 2 * (w + h);

    let as = util.areaString(area, isMetric);
    let ws = util.lengthString( w, isMetric);
    let hs = util.lengthString( h, isMetric);
    let ps = util.lengthString( perim, isMetric);

    return `\n${as}\n${ws} × ${hs}\n${ps}`;
}

function circleStats (geoCircle) {
    let rs = util.lengthString( geoCircle.radius, odinCesium.isMetric);
    let as = util.areaString( Math.PI * geoCircle.radius*geoCircle.radius, odinCesium.isMetric);
    let ps = util.lengthString( 2*Math.PI * geoCircle.radius, odinCesium.isMetric);

    return `\n${as}\n${rs}\n${ps}`;
}

function polygonStats (geoPolygon) {
    let perim = util.distanceOverGeoPoints(geoPolygon.exterior);
    let area = util.geoPolygonArea( geoPolygon.exterior);

    let as = util.areaString( area, odinCesium.isMetric);
    let ps = util.lengthString( perim, odinCesium.isMetric);

    return `\n${as}\n${ps}`;
}

function getItemEntity (e) {
    if (e.value) {
        switch (e.value.type) {
            case "GeoPoint": case "GeoPoint3": return createPointEntity(e);
            case "GeoLine": return createLineEntity(e);
            case "GeoLineString": return createLineStringEntity(e);
            case "GeoRect": return createRectEntity(e);
            case "GeoPolygon": return createPolygonEntity(e);
            case "GeoCircle": return createCircleEntity(e);
            default: return null;
        }
    }  
}

function createPointEntity(e) {
    let d = e.value.data;
    return new Cesium.Entity({
        id: e.key,
        name: e.key,
        position: Cesium.Cartesian3.fromDegrees( d.lon, d.lat, 0),
        label: entityLabel( getItemLabelText(e)),
        point: {
            pixelSize: new Cesium.CallbackProperty( ()=>renderOpts.pointSize, false),
            color: new Cesium.CallbackProperty( ()=>renderOpts.color.withAlpha( renderOpts.fillAlpha), false),
            outlineColor: new Cesium.CallbackProperty( ()=>renderOpts.color, false),
            outlineWidth: 1,
            heightReference: Cesium.HeightReference.CLAMP_TO_GROUND
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

    let colorMaterial = new Cesium.ColorMaterialProperty();
    colorMaterial.color = new Cesium.CallbackProperty( ()=>renderOpts.color, false);

    return new Cesium.Entity({
        id: e.key,
        name: e.key,
        position: midPoint,
        label: entityLabel( getItemLabelText(e)),
        polyline: {
            positions: points,
            clampToGround: true,
            material: colorMaterial,
            width: new Cesium.CallbackProperty( ()=>renderOpts.lineWidth, false)
        }
    });
}

function createLineStringEntity(e){
    let d = e.value.data;
    let points = d.points.map( p=> Cesium.Cartesian3.fromDegrees( p.lon, p.lat));
    let idx = Math.floor(points.length/2);
    let pos = Cesium.Cartesian3.fromDegrees( (d.points[idx].lon + d.points[idx+1].lon)/2, (d.points[idx].lat + d.points[idx+1].lat)/2);

    let colorMaterial = new Cesium.ColorMaterialProperty();
    colorMaterial.color = new Cesium.CallbackProperty( ()=>renderOpts.color, false);

    return new Cesium.Entity({
        id: e.key,
        name: e.key,
        position: pos,
        label: entityLabel( getItemLabelText(e)),
        polyline: {
            positions: points,
            clampToGround: true,
            material: colorMaterial,
            width: new Cesium.CallbackProperty( ()=>renderOpts.lineWidth, false)
        }
    });
}

function createRectEntity(e) {
    let d = e.value.data;
    let center =  Cesium.Cartesian3.fromDegrees( (d.east + d.west)/2, (d.north + d.south)/2);

    let colorMaterial = new Cesium.ColorMaterialProperty();
    colorMaterial.color = new Cesium.CallbackProperty( ()=>renderOpts.color, false);

    let fillColorMaterial = new Cesium.ColorMaterialProperty();
    fillColorMaterial.color = new Cesium.CallbackProperty( ()=>renderOpts.color.withAlpha(renderOpts.fillAlpha), false);

    // Cesium rect properties do not support outlines that are clampToGround so we turn this into a polygon
    let points = [];
    points.push( new Cesium.Cartesian3.fromDegrees( d.west, d.north));
    points.push( new Cesium.Cartesian3.fromDegrees( d.east, d.north));
    points.push( new Cesium.Cartesian3.fromDegrees( d.east, d.south));
    points.push( new Cesium.Cartesian3.fromDegrees( d.west, d.south));
    points.push( points[0]);

    return new Cesium.Entity({
        id: e.key,
        name: e.key,
        position: center,
        label: entityLabel( getItemLabelText(e)),

        polygon: {
            hierarchy: points,
            heightReference: Cesium.CLAMP_TO_GROUND,
            fill: new Cesium.CallbackProperty( ()=>renderOpts.fill, false),
            material: fillColorMaterial,
            zIndex: 2,
        },
        polyline: {
            positions: points,
            clampToGround: true,
            material: colorMaterial,
            width: new Cesium.CallbackProperty( ()=>renderOpts.lineWidth, false)
        }
    });
}

function createCircleEntity(e) {
    let d = e.value.data;
    let center = Cesium.Cartesian3.fromDegrees( d.lon, d.lat);

    let colorMaterial = new Cesium.ColorMaterialProperty();
    colorMaterial.color = new Cesium.CallbackProperty( ()=>renderOpts.color, false);

    let fillColorMaterial = new Cesium.ColorMaterialProperty();
    fillColorMaterial.color = new Cesium.CallbackProperty( ()=>renderOpts.color.withAlpha(renderOpts.fillAlpha), false);

    let outline = odinCesium.circleOutline( center, d.radius);

    return new Cesium.Entity( {
        id: e.key,
        name: e.key,
        position: center,
        label: entityLabel( getItemLabelText(e)),
        point: {
            pixelSize: new Cesium.CallbackProperty( ()=>renderOpts.pointSize, false),
            color: new Cesium.CallbackProperty( ()=>renderOpts.color.withAlpha( renderOpts.fillAlpha), false),
            outlineColor: new Cesium.CallbackProperty( ()=>renderOpts.color, false),
            outlineWidth: 1,
            heightReference: Cesium.HeightReference.CLAMP_TO_GROUND
            //distanceDisplayCondition: config.pointDC, 
        },
        polyline: {
            positions: outline,
            clampToGround: true,
            material: colorMaterial,
            width: new Cesium.CallbackProperty( ()=>renderOpts.lineWidth, false)
        },
        polygon: {
            hierarchy: outline,
            heightReference: Cesium.CLAMP_TO_GROUND,
            fill: new Cesium.CallbackProperty( ()=>renderOpts.fill, false),
            material: fillColorMaterial,
        }
        //ellipse: {
        //    heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
        //    height: 0,
        //    fill: new Cesium.CallbackProperty( ()=>renderOpts.fill, false),
        //    material: fillColorMaterial,
        //    semiMajorAxis: d.radius,
        //    semiMinorAxis: d.radius
        //}
    });
}

function createPolygonEntity(e){
    let d = e.value.data;
    let exterior = d.exterior.map( p=> Cesium.Cartesian3.fromDegrees( p.lon, p.lat));
    exterior.push( exterior[0]); // close the polygon 

    let cp = util.centerLonLat(d.exterior);
    let center = Cesium.Cartesian3.fromDegrees( cp.lon, cp.lat);

    let colorMaterial = new Cesium.ColorMaterialProperty();
    colorMaterial.color = new Cesium.CallbackProperty( ()=>renderOpts.color, false);

    let fillColorMaterial = new Cesium.ColorMaterialProperty();
    fillColorMaterial.color = new Cesium.CallbackProperty( ()=>renderOpts.color.withAlpha(renderOpts.fillAlpha), false);

    return new Cesium.Entity({
        id: e.key,
        name: e.key,
        position: center,
        label: entityLabel( getItemLabelText(e)),
        polygon: {
            hierarchy: exterior,
            heightReference: Cesium.CLAMP_TO_GROUND,
            fill: new Cesium.CallbackProperty( ()=>renderOpts.fill, false),
            material: fillColorMaterial,
            zIndex: 2,
        },
        polyline: {
            positions: exterior,
            clampToGround: true,
            material: colorMaterial,
            width: new Cesium.CallbackProperty( ()=>renderOpts.lineWidth, false)
        }
    });
}

function entityLabel (key) {
    return {
        text: key,
        scale: 0.8,
        heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
        horizontalOrigin: Cesium.HorizontalOrigin.LEFT,
        verticalOrigin: Cesium.VerticalOrigin.TOP,
        font: renderOpts.labelFont,
        fillColor: renderOpts.color,
        showBackground: true,
        backgroundColor: renderOpts.labelBackground,
        pixelOffset: renderOpts.labelOffset,
        distanceDisplayCondition: renderOpts.labelDC,
    };
}

/* #endregion data entities */

/* #region websocket messages ***********************************************************************/

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

function handleInitSharedItems(sharedItems) { 
    let items = schema.keyCategories.slice(); // add the category entries // FIXME - this has to map objects

    // we get this as a map (JS object with keys as properties)
    for (let e of Object.entries(sharedItems)) { // add the values from the server
        let item = new main.SharedItem( e[0], false, e[1]);
        items.push(item);

        share._set( item.key, item); // store the item in our share object
    }

    items.sort( (a,b)=> a.key.localeCompare(b.key));

    let tree = ExpandableTreeNode.from( items, e=>e.key, e=>e.value == undefined ); // this classifies keyCategories as branch nodes
    for (let e of schema.keyCategories) {
        let node = tree.findNode( e.key);
        if (node) {
            node.setSticky( true);
            node.setExpanded( e.expand);
        }
    }

    ui.setTree( dirView, tree);

    main.notifyShareHandlers( main.SHARE_INITIALIZED);
}

function handleSetShared (msg, isLocal) {
    let key = msg.key;
    let value = msg.value;
    let updatedItem = share.getSharedItem(key); // this is the old item (if any)

    if (value.type == main.JSON) { value.data = JSON.parse( value.data) }
    
    let sharedItem = new main.SharedItem(key,isLocal,value);
    share._set( sharedItem.key, sharedItem);
    let isPending = pendingSavedKeys.has(key);
    if (isPending) {
        let entity = getItemEntity(sharedItem);
        if (entity) {
            shareDataSource.entities.removeById(key); // make sure we don't leave an old one around
            shareDataSource.entities.add( entity);
        }
    }

    if (updatedItem) {
       ui.replaceNodeItem( dirView, updatedItem, sharedItem);
    } else {
        ui.sortInTreeItem( dirView, sharedItem, sharedItem.key);
    }

    if (isPending) {
        pendingSavedKeys.delete(key);
        ui.selectNodePath(dirView, key);
    }

    main.notifyShareHandlers( {setShared: sharedItem} );

    if (selItem && selItem.key == sharedItem.key) {
        selItem = sharedItem;
    }
}

function handleRemoveShared (msg) {
    let sharedItem = share.getSharedItem(msg.key);

    if (sharedItem) {
        let key = sharedItem.key;
        share._delete( key);

        ui.removeTreeItemPath( dirView, key); // if visible this will trigger a select null

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

/* #endregion sebsocket messages */

