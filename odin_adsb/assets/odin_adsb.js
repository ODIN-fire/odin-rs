/*
 * Copyright (c) 2025, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The RACE - Runtime for Airspace Concept Evaluation platform is licensed
 * under the Apache License, Version 2.0 (the "License"); you may not use
 * this file except in compliance with the License. You may obtain a copy
 * of the License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

import { config } from "./odin_adsb_config.js";

import * as main from "../odin_server/main.js";
import * as util from "../odin_server/ui_util.js";
import * as data from "../odin_server/ui_data.js";
import * as ui from "../odin_server/ui.js";
import * as ws from "../odin_server/ws.js";
import * as odinCesium from "../odin_cesium/odin_cesium.js";

const MODULE_PATH ="odin_adsb::adsb_service::AdsbService";

/* #region types *******************************************************************************************/

const NO_PATH = "";
const LINE_PATH = "~";
const WALL_PATH = "≈";

// aggregate for track related Entity instances
class TrackAssets {
    constructor(point, symbol, info = null) {
        this.point = point;
        this.symbol = symbol; 
        this.info = info; // additional text label
        this.trajectory = null; // on-demand polyline or wall
    }
}

// object that wraps server-supplied track info with our locally kept trace and display assets
class TrackEntry {
    constructor (trackSource, track) {
        this.track = track;
        this.assets = new TrackAssets(null, null, null);; // all the entities associated with this track
        this.label = track.callsign ? track.callsign : track.icao24.toLowerCase(); // might change
        this.source = trackSource; // backlink required for asset creation

        this.trajectoryPositions = undefined; // set when we request a trajectory - we can't directly feed a CircularBuffer into a Entity polyline / wall
    }

    assetDisplay() {
        let s = "";
        if (this.assets && this.assets.trajectory) {
            let tr = this.assets.trajectory;
            if (tr.polyline) s += LINE_PATH;
            else if (tr.wall) s += WALL_PATH;
            else s += NO_PATH;
        }
        return s;
    }

    isClampedToGround() {
        return (this.track.alt == 0);
    }

    static compare (a,b) {
        if (a.label < b.label) return -1;
        else if (a.label > b.label) return 1;
        else return 0;
    }

    initTrajectoryPositions () {
        this.trajectoryPositions = Array.from( this.track.trace);
    }

    clearTrajectoryPositions () {
        this.trajectoryPositions = undefined;
    }
}

class TrackSource {
    constructor(id) {
        this.id = id;

        this.show = true;
        this.trackEntries = new Map();  // track.icao24->TrackEntry
        this.date = 0; // last change of trackEntries

        this.trackEntryList = new data.SkipList( // id-sorted display list for trackEntryView (needed for efficient trackEntryView update)
            5, // max depth
            (a, b) => a.label < b.label, // sort function
            (a, b) => a.label == b.label // identity function
        );

        // we keep those in different data sources so that we can control Z-order and 
        // bulk enable/disable display more efficiently
        this.symbolDataSource = odinCesium.createDataSource( id, config.layer.show); // display list for Cesium track entities
        this.trackInfoDataSource = odinCesium.createDataSource(id + '-trackInfo', config.layer.show);
        this.trajectoryDataSource = odinCesium.createDataSource(id + '-trajectories', config.layer.show);
        this.pointDataSource = odinCesium.createDataSource(id + '-point', config.layer.show);

        this.modelPrototypes = new Map(); // track.type -> model cache
    }

    setVisible(cond) {
        let isVisible = isAdsbShowing && this.show && cond;

        this.symbolDataSource.show = isVisible;
        this.trackInfoDataSource.show = isVisible;
        this.trajectoryDataSource.show = isVisible;
        this.pointDataSource.show = isVisible;
    }

    getSortedTrackEntries () {
        return Array.from( this.trackEntries.values()).sort( TrackEntry.compare);
    }

    //--- the asset creators (here because they might cache within the TrackSource)

    createTrackPointAsset (te) {
        let track = te.track;
        let trackColor = config.colors.get( this.id);
        let heightRef = getHeightReference( track);

        let entityOpts = {
            id: track.icao24,
            pos: track.position,
            point: {
                pixelSize: config.pointSize,
                color: trackColor,
                outlineColor: config.pointOutlineColor,
                outlineWidth: config.pointOutlineWidth,
                distanceDisplayCondition: config.pointDC,
                heightReference: heightRef
            },
            label: trackEntityLabelOpts( te, trackColor, heightRef)
        };

        let entity = new Cesium.Entity( entityOpts);
        entity._uiTrackEntry = te; // for entity selection

        return entity;
    }

    createTrackInfoAsset (te) {
        let track = te.track;
        let trackColor = config.colors.get( this.id);
        let infoText = trackInfoLabel( te);

        let entityOpts = {
            id: track.icao24,
            position: track.position,

            label: {
                text: infoText,
                font: config.infoFont,
                scale: 0.8,
                horizontalOrigin: Cesium.HorizontalOrigin.LEFT,
                verticalOrigin: Cesium.VerticalOrigin.TOP,
                fillColor: trackColor,
                //showBackground: true,
                //backgroundColor: config.labelBackground, // alpha does not work against model
                outlineColor: trackColor,
                outlineWidth: 1,
                pixelOffset: config.infoOffset,
                //disableDepthTestDistance: config.minLabelDepth,
                //disableDepthTestDistance: Number.POSITIVE_INFINITY,
                distanceDisplayCondition: config.infoDC
            }
        };

        return new Cesium.Entity( entityOpts);
    }

    createTrackSymbolAsset (te) {
        let track = te.track;

        let trackColor = config.colors.get( this.id);
        let modelUri = config.models.get( track.type);
        let attitude = getTrackAttitude( track);
        let heightRef = getHeightReference( track);

        let entityOpts = {
            id: (track.callsign ? track.callsign : track.icao24),
            position: track.position,
            orientation: attitude,
            label: trackEntityLabelOpts( te, trackColor, heightRef)
        };

        let model = this.modelPrototypes.get( track.type);
        if (!model) {
            entityOpts.model = {
                uri: modelUri,
                color: trackColor,
                //colorBlendMode: Cesium.ColorBlendMode.HIGHLIGHT,
                colorBlendMode: Cesium.ColorBlendMode.MIX,
                colorBlendAmount: 0.7,
                silhouetteColor: config.modelOutlineColor,
                silhouetteSize: config.modelOutlineWidth,
                minimumPixelSize: config.modelSize,
                distanceDisplayCondition: config.modelDC,
                //heightReference: heightRef
            }
        }

        let entity = new Cesium.Entity( entityOpts);
        entity._uiTrackEntry = te; // for entity selection

        if (model) { 
            entity.model = model;
        } else { // cache it
            this.modelPrototypes.set( track.type, entity.model);
        }

        return entity;
    }

    createTrajectoryAsset (te, isWall) {
        let track = te.track;
        let trackColor = config.colors.get( this.id);

        if (isWall) {
            return new Cesium.Entity({
                id: track.icao24,
                wall: {
                    positions: te.trajectoryPositions,
                    show: true,
                    fill: true,
                    material: Cesium.Color.fromAlpha(trackColor, 0.2),
                    outline: true,
                    outlineColor: Cesium.Color.fromAlpha(trackColor, 0.5),
                    outlineWidth: config.pathWidth,
                    distanceDisplayCondition: config.pathDC
                }
            });
        } else {
            let isGroundPath = te.isClampedToGround();
            return new Cesium.Entity({
                id: track.icao24,
                polyline: {
                    positions: te.trajectoryPositions, // posCallback,
                    clampToGround: isGroundPath,
                    width: isGroundPath ? config.path2dWidth : config.pathWidth,
                    material: trackColor,
                    distanceDisplayCondition: config.pathDC
                }
            });
        }
    }
}

/* #endregion types */

/* #region init *************************************************************************************************/

var isAdsbShowing = config.layer.show; // we need to keep track of layer showing

var trackSources = new Map(); // name->TrackSource map : populated on the fly as we receive snapshot/update messages
var selectedTrackSource = undefined;

var trackEntryFilter = noTrackEntryFilter;

createIcon();
createWindow();

// await odinCesium.viewerReadyPromise;

var trackSourceView = initTrackSourceView();
var trackEntryView = initTrackEntryView();

odinCesium.setEntitySelectionHandler(trackSelection);
ws.addWsHandler( MODULE_PATH, handleWsMessages);

odinCesium.initLayerPanel("adsb", config, showAdsb);
console.log("odin_adsb initialized");

/* #endregion init */

/* #region UI ****************************************************************************************************/

function createIcon() {
    return ui.Icon("./asset/odin_adsb/adsb-icon.svg", (e)=> ui.toggleWindow(e,'adsb'), "ADS-B aircraft tracking");
}

function createWindow() {
    return ui.Window("Aircraft", "adsb", "./asset/odin_adsb/adsb-icon.svg")(
        ui.LayerPanel("adsb", toggleShowAdsb),
        ui.Panel("track sources", true)(
            ui.List("tracks.sources", 5, selectSource)
        ),
        ui.Panel("tracks", true)(
            ui.TextInput("query","tracks.query", "15rem", {placeHolder: "enter query", changeAction: queryTracks}),
            ui.List("tracks.list", 10, selectTrack),
            ui.RowContainer()(
                ui.CheckBox("show path", toggleShowPath, "tracks.path"),
                ui.Radio("line", changePath, "tracks.line"),
                ui.Radio("wall", changePath, "tracks.wall"),
                ui.Button("Reset", clearAllPaths)
            )
        )
    )
}

function initTrackSourceView() {
    let view = ui.getList("tracks.sources");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "show", tip: "toggle visibility", width: "2.5rem", attrs: [], map: e => ui.createCheckBox(e.show, toggleShowSource) },
            { name: "id", width: "8rem", attrs: ["alignLeft"], map: e => e.id },
            { name: "tracks", tip: "number of tracks", width: "3rem", attrs: ["fixed", "alignRight"], map: e => e.trackEntries.size.toString() },
            { name: "date", width: "6rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalTimeString(e.date) }
        ]);
    }
    return view;
}

function initTrackEntryView() {
    let view = ui.getList("tracks.list");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "", width: "2rem", attrs: [], map: te => te.assetDisplay() },
            { name: "id", width: "6rem", attrs: ["alignLeft"], map: te => te.label },
            { name: "alt", tip: "altitude in ft", width: "4rem", attrs: ["fixed", "alignRight"], map: te => defined_value(te.track.alt) },
            { name: "spd", tip: "ground speed in knots", width: "3rem", attrs: ["fixed", "alignRight"], map: te => defined_value(te.track.spd) },
            { name: "hdg", tip: "heading", width: "3rem", attrs: ["fixed", "alignRight"], map: te => defined_value(te.track.hdg) },
            { name: "date", width: "6rem", attrs: ["fixed", "alignRight"], map: te => util.toLocalTimeString(te.track.date) }
        ]);
    }
    return view;
}

function defined_value (v) {
    return v ? v : "-";
}

// our local show/hide
function toggleShowAdsb (event) { 
    showAdsb( !isAdsbShowing)
}

// the layer window show/hide
function showAdsb (cond) {
    isAdsbShowing = cond;
    for (let src of trackSources.values()) {
        src.setVisible( isAdsbShowing);
    }
    odinCesium.requestRender();
}

/* #endregion UI */

/* #region websocket message handler ***********************************************************************/

function handleWsMessages(msgType, msg) {
    switch (msgType) {
        case "update": handleWsUpdate( msg); break; // this is the frequent case
        case "snapshot": handleWsSnapshot( msg); break; // the initial message
    }
}

function handleWsSnapshot (msg) {
    let ts = getOrAddSource( msg.source);
    ts.trackEntries.clear();

    msg.aircraft.forEach( (track) => {
        track.trace = data.CircularBuffer.fromArray( track.trace, config.maxTraceLength);
        track.position = track.trace.last();

        let te = new TrackEntry( ts, track);
        ts.trackEntries.set( track.icao24, te);

        if (track.date > ts.date) ts.date = track.date;

        addTrack( ts, te);
    });

    ui.updateListItem( trackSourceView, ts);
    odinCesium.requestRender();
}

function handleWsUpdate (msg) {
    let ts = getOrAddSource( msg.source);

    if (msg.removed) {
        msg.removed.forEach( (icao24) => {
            let te = ts.trackEntries.get( icao24);
            if (te) {
                if (ts.trackEntries.delete(icao24)){
                    removeAssets(ts, te);
                    if (Object.is( ts, selectedTrackSource) && trackEntryFilter(te)) {
                        ui.removeListItem(trackEntryView, te); // might not be showing
                    }
                }
            }
        });
    }

    msg.updated.forEach( (track) => { // this is actually for updated and new entries
        let labelChanged = false;

        let te = ts.trackEntries.get( track.icao24);
        if (te) { // update
            if (te.track.date > track.date) return; // ignore
            track.trace = te.track.trace; // copy from previous so that we don't allocate a new array
            track.trace.push( track.position); // add the current position to the trace

            te.track = track;
            if (track.callsign && te.label != track.callsign) {
                labelChanged = true;
                te.label = track.callsign;
            }
            updateTrack( ts, te, labelChanged);

        } else { // a new track reported in an update
            track.trace = new data.CircularBuffer( config.maxTraceLength);
            track.trace.push( track.position); // add the current position to the trace

            te = new TrackEntry( ts, track);
            ts.trackEntries.set( track.icao24, te);
            addTrack(ts, te);
        }

        if (track.date > ts.date) ts.date = track.date; // update source date
    });

    ui.updateListItem( trackSourceView, ts);
}

function getOrAddSource (id) {
    let ts = trackSources.get(id);
    if (!ts) { 
        ts = new TrackSource(id);
        trackSources.set( id, ts);
        setSourceViewItems();
    }
    return ts; 
}

function setSourceViewItems () {
    let srcList = Array.from(trackSources.values()).sort( (a,b) => {
        if (a.id < b.id) return -1;
        else if (a.id > b.id) return 1;
        else return 0;
    });
    ui.setListItems( trackSourceView, srcList);
}

function getTrackAttitude (track) {
    let pos = track.position ? track.position : track.trace.last(); 

    let hdg = track.hdg ? track.hdg : 0.0;
    let pitch = track.pitch ? track.pitch : 0.0;
    let roll = track.roll ? track.roll : 0.0;

    let hpr = Cesium.HeadingPitchRoll.fromDegrees( hdg, pitch, roll);

    return Cesium.Transforms.headingPitchRollQuaternion(pos, hpr);
}


/* #endregion websocket message handler */

/* #region track update (flicker avoidance) *************************************************************************/

function addTrack (ts, te) {
    let assets = te.assets;

    if (ts.show) {
        if (trackEntryFilter(te)) {
            assets.point = ts.createTrackPointAsset(te);
            assets.symbol = ts.createTrackSymbolAsset(te);
            assets.info = ts.createTrackInfoAsset(te);
            // trajectory only created on demand

            // this is why we need to keep the trackEntryList - to be able to insert single list items
            let idx = ts.trackEntryList.insert(te);
            if (Object.is( ts, selectedTrackSource)) {
                ui.insertListItem(trackEntryView, te, idx);
            }

            if (assets.symbol) addTrackSymbolEntity( ts.symbolDataSource, assets.symbol);
            if (assets.info) addTrackInfoEntity( ts.trackInfoDataSource, assets.info);
            if (assets.point) addTrackPointEntity( ts.pointDataSource, assets.point);
        }
    }
}

function updateTrack( ts, te, labelChanged) {
    let track = te.track;
    let assets = te.assets;
    let pos = track.position;

    let trackEntryList = ts.trackEntryList;

    if (isTrackTerminated(track)) {
        removeTrackEntry(ts, te);

    } else { // update
        if (assets.symbol) updateTrackSymbolAsset(te);
        if (assets.info) updateTrackInfoAsset(te);
        if (assets.point) updateTrackPointAsset(te);
        if (assets.trajectory) updateTrajectoryAsset(te);

        if (trackEntryFilter(te)) {
            if (labelChanged) {
                trackEntryList.remove(te);
                if (Object.is( ts, selectedTrackSource)) {
                    ui.removeListItem(trackEntryView, te);
                }

                let idx = trackEntryList.insert(te);

                if (Object.is( ts, selectedTrackSource)) {
                    ui.insertListItem(trackEntryView, te, idx);
                }
            } else {
                if (Object.is( ts, selectedTrackSource)) {
                    ui.updateListItem(trackEntryView, te);
                }
            }
        }
    }
}

function removeTrackEntry(ts, te) {
    if (trackEntryFilter(te)) {
        ts.trackEntryList.remove(te);
        if (Object.is( ts, selectedTrackSource)) {
            ui.removeListItem(trackEntryView, te);
        }
        ts.trackEntries.delete(te.label);
    }
    removeAssets(ts, te);
}


function isDroppedOrCompleted(track) {
    return (track.status & (0x04 | 0x08)) != 0;
}

function isTrackTerminated(track) {
    return (track.status & 0x0c); // 4: dropped, 8: completed
}

function hasTrackIdChanged(track) {
    return (track.status & 0x20);
}

function noTrackEntryFilter(track) { return true; } // all tracks are displayed


// intercept adding/removing entities to enable the non-flicker hack.
// as of Cesium 1.89 single polylines/walls (and possible other entity properties) within a DataSource
// cause flicker on update when using ConstantProperty, and get corrupted when using CallbackProperty
// (draw object end point inserted at splice point).

function addTrackPointEntity(ds, e) {
    ds.entities.add(e);
}

function removeTrackPointEntity(ds, e) {
    ds.entities.remove(e);
}

function addTrackSymbolEntity(ds, e) {
    ds.entities.add(e);
}

function removeTrackSymbolEntity(ds, e) {
    ds.entities.remove(e);
}

function addTrackInfoEntity(ds, e) {
    ds.entities.add(e);
}

function removeTrackInfoEntity(ds, e) {
    ds.entities.remove(e);
}

function addTrajectoryEntity(ds, e) {
    let e0 = Object.assign({}, e);
    e0.id = e.id + "-0";
    if (e.wall) {
        e0.wall = e.wall.clone();
        e0.wall.positions = e.wall.positions.getValue().slice(0, 2);
    } else {
        e0.polyline = e.polyline.clone();
        e0.polyline.positions = e.polyline.positions.getValue().slice(0, 2);
    }
    ds.entities.add(e0);
    //--- end flicker hack

    ds.entities.add(e);
}

function removeTrajectoryEntity(ds, e) {
    ds.entities.removeById(e.id + "-0"); // flicker hack
    ds.entities.remove(e);
}

function trackSelection() {
    let sel = odinCesium.getSelectedEntity();
    if (sel && sel._uiTrackEntry) {
        let te = sel._uiTrackEntry;
        if (!Object.is( te.trackSource, selectedTrackSource)) {
            if (selectedTrackSource) ui.clearSelectedListItem(trackEntryView);
            ui.setSelectedListItem(trackSourceView, te.trackSource);
        }
        ui.setSelectedListItem(trackEntryView, te);
    } else {
        ui.clearSelectedListItem(trackEntryView);
        //odinCesium.clearSelectedEntity(); // this takes care of the selIndicator re-flicker when showing paths
    }
}

/* #endregion track update */

function getHeightReference(track) {
    if (track.altitude == 0.0) return Cesium.HeightReference.CLAMP_TO_GROUND;
    else return Cesium.HeightReference.NONE; // TODO - should that be RELATIVE_TO_GROUND ?
}


function trackEntityLabelOpts (trackEntry, trackColor, heightRef) {
    return {
        text: trackEntry.label,
        scale: 0.8,
        horizontalOrigin: Cesium.HorizontalOrigin.LEFT,
        verticalOrigin: Cesium.VerticalOrigin.TOP,
        font: config.labelFont,
        fillColor: trackColor,
        //showBackground: true,
        //backgroundColor: config.labelBackground, // alpha does not work against model
        outlineColor: trackColor,
        outlineWidth: 1,
        pixelOffset: config.labelOffset,
        //disableDepthTestDistance: config.minLabelDepth,
        //disableDepthTestDistance: Number.POSITIVE_INFINITY,
        distanceDisplayCondition: config.labelDC,
        heightReference: heightRef
    };
}

function updateTrackSymbolAsset (te) {
    let sym = te.assets.symbol;
    sym.position = te.track.position;
    sym.orientation = getTrackAttitude( te.track);
}

function updateTrackPointAsset (te) {
    let pt = te.assets.point;
    pt.position = te.track.position;
}

function updateTrackInfoAsset (te) {
    let info = te.assets.info;
    info.label.text = trackInfoLabel(te);
    info.position = te.track.position;
}

function updateTrajectoryAsset(te) {
    let entity = te.assets.trajectory;
    if (entity) {
        let pos = te.track.position;
        if (pos) {
            te.trajectoryPositions.push( pos);
        }

        let traj = entity.wall ? entity.wall : entity.polyline;
        traj.positions = te.trajectoryPositions;
        odinCesium.requestRender();
    }
}

const DOWN = '▽'; // '-' '▽' '↓' '⊤' '⇩'
const LEFT = '◁'; // '-' '◁' '↶' '⟲'
const UP = '△'; // '+' '△' '↑' '⊥'
const RIGHT = '▷'; // '+' '▷' '↷' '⟳'
const SAME = ' '; // ' ' '⊣' '⎢'

function trackInfoLabel(trackEntry) {
    let track = trackEntry.track;

    let fl = util.toRightAlignedString( Math.round(track.alt/100), 3); // in 100ft
    let hdg = util.toRightAlignedString(Math.round(track.hdg), 3);
    let spd = util.toRightAlignedString(Math.round(track.spd), 3);

    return `${fl} fl\n${hdg} °\n${spd} kn`;
}

function removeAssets(ts, te) {
    let assets = te.assets;
    if (assets.symbol) removeTrackSymbolEntity(ts.symbolDataSource, assets.symbol);
    if (assets.info) removeTrackInfoEntity(ts.trackInfoDataSource, assets.info);
    if (assets.trajectory) {
        removeTrajectoryEntity(ts.trajectoryDataSource, assets.trajectory);
        te.clearTrajectoryPositions();
    }
    if (assets.point) removeTrackPointEntity(ts.pointDataSource, assets.point);
}

function createTrajectoryAssetPositions(trackEntry) {
    let trace = trackEntry.trace;
    let positions = new Array(trace.size);
    let i = 0;
    trace.forEach(t => {
        positions[i++] = Cesium.Cartesian3.fromDegrees(t.lon, t.lat, t.alt);
    });
    return positions;
}

//--- track queries

const idQuery = /^ *id *= *(.+)$/;
// ..and more to follow

function getTrackEntryFilter(query) {
    if (!query || query == "*") {
        return noTrackEntryFilter;
    } else {
        let res = query.match(idQuery);
        if (res) {
            let idQuery = util.glob2regexp(res[1]);
            return (idQuery == '*') ? noTrackEntryFilter : te => te.label.match(idQuery);
        }
        return null;
    }
}

//--- interaction (those cannot be called without a DOM event argument)

function queryTracks(event) {
    let input = ui.getFieldValue(event);

    let newFilter = getTrackEntryFilter(input);
    if (newFilter) {
        trackEntryFilter = newFilter;
        resetTrackEntryList();
        resetTrackEntryAssets();
    }
}

function resetTrackEntryList() {
    let ts = selectedTrackSource;
    if (ts) {
        ts.trackEntryList.clear();
        ts.trackEntries.forEach((te, id, map) => {
            if (trackEntryFilter(te)) ts.trackEntryList.insert(te);
        });
        ui.setListItems(trackEntryView, ts.trackEntryList);
    } else {
        ui.clearList( trackEntryView);
    }
}

function resetTrackEntryAssets() {
    let ts = selectedTrackSource;
    if (ts) {
        ts.trackEntries.forEach((te, id, map) => {
            let assets = te.assets;

            if (trackEntryFilter(te)) {
                if (!assets.point) {
                    assets.point = ts.createTrackPointAsset(te);
                    addTrackPointEntity(ts.pointDataSource, assets.point);
                }
                if (!assets.symbol) {
                    assets.symbol = ts.createTrackSymbolAsset(te);
                    addTrackSymbolEntity(ts.symbolDataSource, assets.symbol);
                }
                if (!assets.info) {
                    assets.info = ts.createTrackInfoAsset(te);
                    addTrackInfoEntity(ts.trackInfoDataSource, assets.info);
                }
                // no trajectory until we ask for it

            } else { // filtered, check if we need to remove from viewer entities
                if (assets.point) {
                    removeTrackPointEntity(ts.pointDataSource, assets.point);
                    assets.point = null;
                }
                if (assets.symbol) {
                    removeTrackSymbolEntity(ts.symbolDataSource, assets.symbol);
                    assets.symbol = null;
                }
                if (assets.info) {
                    removeTrackInfoEntity(ts.trackInfoDataSource, assets.info);
                    assets.info = null;
                }
                if (assets.trajectory) {
                    removeTrajectoryEntity(ts.trajectoryDataSource, assets.trajectory);
                    assets.trajectory = null;
                }
            }
        });
    }
    odinCesium.requestRender();
}

function selectTrack(event) {
    let te = event.detail.curSelection;
    if (te) {
        if (te.assets.symbol) odinCesium.setSelectedEntity(te.assets.symbol);
        if (te.assets.trajectory) {
            ui.setCheckBox("tracks.path", true);
            if (te.assets.trajectory.wall) ui.selectRadio("tracks.wall");
            else ui.selectRadio("tracks.line");
        } else {
            ui.setCheckBox("tracks.path", false);
            ui.clearRadioGroup("tracks.line");
        }
    } else { // nothing selected
        ui.setCheckBox("tracks.path", false);
        ui.clearRadioGroup("tracks.line");
    }
}

function toggleShowPath(event) {
    let te = ui.getSelectedListItem(trackEntryView);
    if (te) {
        if (ui.isCheckBoxSelected(event)) {
            setPath(te);
        } else {
            clearPath(te);
        }
    }
}

function setPath (te) {
    if (te.assets.trajectory) {
        removeTrajectoryEntity(te.source.trajectoryDataSource, te.assets.trajectory);
    }

    let isWall = ui.isRadioSelected("tracks.wall");
    if (!isWall) ui.selectRadio("tracks.line");

    te.initTrajectoryPositions();
    te.assets.trajectory = te.source.createTrajectoryAsset(te, isWall);
    addTrajectoryEntity(te.source.trajectoryDataSource, te.assets.trajectory);
    odinCesium.requestRender();
}

function clearPath (te) {
    if (te.assets.trajectory) {
        removeTrajectoryEntity(te.source.trajectoryDataSource, te.assets.trajectory);
        te.assets.trajectory = null;
        te.clearTrajectoryPositions();
        ui.clearRadioGroup("tracks.line");
    }
}

function changePath(event) {
    let te = ui.getSelectedListItem(trackEntryView);
    if (te) {
        setPath(te)
    }
}

function clearAllPaths() {
    trackSources.forEach( (ts) => {
        ts.trackEntries.forEach( (te,id)=> {
            if (te.assets.trajectory) {
                removeTrajectoryEntity( ts.trajectoryDataSource, te.assets.trajectory);
                te.assets.trajectory = null;
                te.clearTrajectoryPositions();
            }
        })
    });
    odinCesium.clearSelectedEntity();
    odinCesium.requestRender();

    ui.updateListItems( trackEntryView);
    ui.setCheckBox("tracks.path", false);
    ui.clearRadioGroup(ui.getRadio("tracks.wall"));

}

function selectSource(event) {
    selectedTrackSource = event.detail.curSelection;
    resetTrackEntryList();
}

function toggleShowSource(event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let tse = ui.getListItemOfElement(cb);
        if (tse) {
            tse.setVisible(ui.isCheckBoxSelected(cb));
        }
    }
}

function showTracks(showIt) {
    trackSources.forEach(tse => {
        tse.setVisible(showIt);
        ui.updateListItem(trackSourceView, tse);
    });
}

function toggleShowTracks(event) {
    console.log("not yet")
}