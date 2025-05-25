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
import { config } from "./odin_orbital_config.js";

import * as main from "../odin_server/main.js";
import * as util from "../odin_server/ui_util.js";
import * as data from "../odin_server/ui_data.js";
import * as ui from "../odin_server/ui.js";
import * as ws from "../odin_server/ws.js";
import * as odinCesium from "../odin_cesium/odin_cesium.js";

const MODULE_PATH ="odin_orbital::hotspot_service::OrbitalHotspotService";

ws.addWsHandler( MODULE_PATH, handleWsMessages);
main.addShareHandler( handleShareMessage);

var lastInit = undefined; // epoch when we last got initialized through the websocket

// to store SatelliteEntries
var satelliteEntries = [];
var selSat = undefined;
var selSatOnly = false;

// to store OverpassEntries (all of them - not filtered by satellite or region)
var upcomingEntries = [];  
var completedEntries = [];

// the filtered OverpassEntries shown in the upcoming/past lists 
var shownUpcomingEntries = []; 
var shownCompletedEntries = [];

var moveTimer = undefined; // set once we get the first upcoming overpass

var selCompleted = undefined;
var showHistory = false;  // shall we show footprints of past entries?
var followLatest = false; // always show latest completed overpass

var areaAsset = undefined;
var area = undefined;  // bounds as Rectangle
var areaVertices = [];

var zoomHeight = 20000;

// the Cesium assets to display fire pixels
var hsFootprintPrimitive = undefined;  // the surface footprint polygon of a hotspot
var hsPointPrimitive = undefined;   // the brightness/frp points
var hsOutlinePrimitive = undefined; // the surface footprint polyline of a (small) hotspot

var areaInfoText = undefined;

function resetData () {
    satelliteEntries = [];
    upcomingEntries = [];  
    completedEntries = [];
}

class SatelliteEntry {
    constructor(sat) {
        this.satId = sat.satId;
        this.satName = sat.name;
        this.avgSwathWidth = sat.avgSwathWidth;
        this.avgOrbitDuration = sat.avgOrbitDuration;

        this.show = true; // can be changed interactively

        this.prev = 0; // overpass times updated when we get new overpasses and hotspots
        this.next = 0;
    }
}

class OverpassEntry {
    constructor(o) {
        this.satId = o.satId;
        this.start = o.start;
        this.end = o.end;

        this.meanSwathWidth = o.meanSwathWidth; 
        this.meanGpDist = gpSecantFromSwathWidth(o.meanSwathWidth); // cartesian3 distance between ground point and swath horizon

        this.track = undefined; // dynamically loaded ground track points
        this.fname = o.fname; // filename to load ground track from

        this.swathEntity = undefined; // set when we interactively select to show
        this.showSwath = false;

        // the data for this overpass - set when we get a hotspots message
        this.hsList = undefined; 
    }

    stats () {
        if (this.hsList) {
            let hs = this.hsList;
            let total = hs.high + hs.nominal + hs.low;
            return `${hs.high}/${total}`;
        } else {
            return "-";
        }
    }
}

function gpSecantFromSwathWidth (s) {
    let alpha = s / util.meanEarthRadius;
    let beta = (Math.PI - alpha) / 2;
    return util.meanEarthRadius * Math.sin(alpha) / Math.sin(beta);
}

createIcon();
createWindow();

var satelliteView = initSatelliteView();
var upcomingView = initUpcomingView();
var completedView = initPastView();
var hotspotView = initHotspotView();
var sharedAreasView = initAreasView();


var dataSource = new Cesium.CustomDataSource("orbital");
odinCesium.addDataSource(dataSource);

var maxAgeInDays = config.maxAgeInDays;
var timeSteps = config.timeSteps;
var brightThreshold = config.bright.value;
var brightThresholdColor = config.bright.color;
var frpThreshold = config.frp.value;
var frpThresholdColor = config.frp.color;
var pixelSize = config.pixelSize;
var outlineWidth = config.outlineWidth;

var smallFootprintLength = config.smallFootprintLength;
var smallFootprintDist = config.smallFootprintDist;
var footprintDist = config.footprintDist;

initSliders();

ui.setCheckBox("orbital.sel_sat", selSatOnly);
ui.setCheckBox("orbital.show_history", showHistory);

odinCesium.initLayerPanel("orbital", config, showOrbital);
console.log("odin_orbital initialized");

//--- end init

function createIcon() {
    return ui.Icon("./asset/odin_orbital/polar-sat-icon.svg", (e)=> ui.toggleWindow(e,'orbital'), "polar sat hotspots");
}

function createWindow() {
    return ui.Window("Orbiting Satellite Hotspots", "orbital", "./asset/odin_orbital/polar-sat-icon.svg")(
        ui.LayerPanel("orbital", toggleShowOrbital),
        ui.Panel("area", false)(
            (sharedAreasView = ui.TreeList("orbital.areas", 10, "30rem", setAreaFromSelection)),
            ui.RowContainer()(
              ui.TextInput("area","orbital.bounds", "20rem", { placeHolder: "enter lat,lon bounds (WSEN order)", changeAction: setAreaFromInput, isFixed: true }),
              ui.Button("pick", pickArea),
              ui.Button("clear", clearArea),
              ui.Button( "zoom", zoomToArea)
            ),
            (areaInfoText = ui.VarText("", "orbital.bounds-info"))
        ),
        ui.Panel("satellite overpasses:", true)(
            ui.List("orbital.satellites", 5, selectOrbitalSatellite),

            ui.RowContainer("start")(
                ui.RowContainer(null,null,"upcoming overpasses")(
                    ui.List("orbital.upcoming", 5)
                ),
                ui.HorizontalSpacer(0.5),
                ui.RowContainer(null,null,"completed overpasses")(
                    ui.List("orbital.past", 5, selectCompleted)
                )
            ),
            ui.RowContainer()(
                ui.CheckBox("sel sat only", toggleSelSatOnly, "orbital.sel_sat"),
                ui.CheckBox("follow latest", toggleFollowLatest, "orbital.follow_latest"),
                ui.CheckBox("show history", toggleShowHistory, "orbital.show_history"),
                ui.HorizontalSpacer(2),
                ui.ListControls("orbital.past",null,null,null,null,clearOrbits)
            )
        ),
        ui.Panel("hotspots", false)(
            ui.List("orbital.hotspots", 10, null, null, null, zoomToHotspot)
        ),
        ui.Panel("layer parameters", false)(
            ui.RowContainer()(
                ui.ColumnContainer("align_right")(
                    ui.Slider("max age [d]", "orbital.history", setOrbitalHistory),
                    ui.Slider("bright [K]", "orbital.bright", setOrbitalBrightThreshold),
                    ui.Slider("frp [MW]", "orbital.frp", setOrbitalFrpThreshold)
                ),
                ui.ColumnContainer("align_right")(
                    ui.Slider("size [pix]", "orbital.pixsize", setOrbitalPixelSize),
                    ui.Slider("outline [pix]", "orbital.outline", setOrbitalOutlineWidth)
                )
            )
        )
    );
}

function initSatelliteView() {
    let view = ui.getList("orbital.satellites");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "show", tip: "show/hide satellite", width: "3rem", attrs: [], map: e => ui.createCheckBox(e.show, toggleShowSatellite) },
            { name: "sat", tip: "satellite name", width: "4rem", attrs: [], map: e => e.satName },
            { name: "swath", tip: "half swath width [km]", width: "4rem", attrs:["fixed", "alignRight"], map: e => util.f_0.format(e.avgSwathWidth / 1000.0) },
            { name: "rev", tip: "orbital period [min]", width: "3rem", attrs:["fixed", "alignRight"], map: e => util.f_0.format(e.avgOrbitDuration) },
            { name: "next", tip: "next upcoming overpass (local)", width: "8rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMString(e.next) },
            { name: "last", tip: "most recent overpass (local)", width: "8rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMString(e.prev) }

        ]);
    }
    return view;
}

function initUpcomingView() {
    let view = ui.getList("orbital.upcoming");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "swt", tip: "show swath/ground track", width: "2rem", attrs: [], map: e => ui.createCheckBox(e.showSwath, toggleShowSwath) },
            { name: "sat", tip: "satellite name", width: "4rem", attrs: [], map: e => satName(e.satId) },
            { name: "date", tip: "overpass end date", width: "7rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMString(e.end) }        ]);
    }
    return view;
}

function initPastView() {
    let view = ui.getList("orbital.past");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "swt", tip: "show swath/ground track", width: "2rem", attrs: [], map: e => ui.createCheckBox(e.showSwath, toggleShowSwath) },
            { name: "sat", tip: "satellite name", width: "4rem", attrs: [], map: e => satName(e.satId) },
            { name: "hot", tip: "number of high-confidence / total hotspots", width: "6rem", attrs: ["fixed", "alignRight"], map: e => e.stats() },
            { name: "date", tip: "overpass end date", width: "8rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMString(e.end) }
        ]);
    }
    return view;
}

function initHotspotView() {
    let view = ui.getList("orbital.hotspots");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "conf", tip: "hotspot confidence [0:low,1:med,2:high]", width: "2rem", attrs: ["fixed", "alignRight"], map: e => hotspotConfidence(e) },
            { name: "temp", tip: "hotspot brightness [K]", width: "4rem", attrs: ["fixed", "alignRight"], map: e => util.f_0.format(e.temp) },
            { name: "frp", tip: "hotspot fire radiative power [MW]", width: "4.5rem", attrs: ["fixed", "alignRight"], map: e => util.f_2.format(e.frp) },
            { name: "lon", tip: "longitude", width:  "7rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.lon,4)},
            { name: "lat", tip: "latitude", width:  "5rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.lat,4)},
        ]);
    }
    return view;
}

function initAreasView() {
    let view = ui.getList("orbital.areas");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "west", tip: "longitude", width:  "6rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.value.data.west,3)},
            { name: "south", tip: "latitude", width:  "5rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.value.data.south,3)},
            { name: "east", tip: "longitude", width:  "6rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.value.data.east,3)},
            { name: "north", tip: "latitude", width:  "5rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.value.data.north,3)},
        ]);
    }
    return view;
}

function initSliders() {
    let e = ui.getSlider("orbital.history");
    ui.setSliderRange(e, 0, 20, 1, util.f_0);
    ui.setSliderValue(e, maxAgeInDays);

    e = ui.getSlider("orbital.pixsize");
    ui.setSliderRange(e, 0, 8, 1, util.fmax_0);
    ui.setSliderValue(e, pixelSize);

    e = ui.getSlider("orbital.outline");
    ui.setSliderRange(e, 0, 3, 0.5, util.fmax_1);
    ui.setSliderValue(e, outlineWidth);

    e = ui.getSlider("orbital.bright");
    ui.setSliderRange(e, 0, 400, 25, util.fmax_0);
    ui.setSliderValue(e, brightThreshold);

    e = ui.getSlider("orbital.frp");
    ui.setSliderRange(e, 0, 300, 25, util.fmax_0);
    ui.setSliderValue(e, frpThreshold);
}

function toggleShowSatellite(event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let se = ui.getListItemOfElement(cb);
        if (se) {
            se.show = ui.isCheckBoxSelected(cb);
            updateUpcoming();
            updateCompleted();
        }
    }
}

function toggleShowSwath (event) {
    let cb = ui.getCheckBox(event.target);
    let oe = ui.getListItemOfElement(cb);

    oe.showSwath = ui.isCheckBoxSelected(cb);

    if (oe.showSwath) {
        if (oe.showSwath) { // check again - could have been de-selected in the meantime
            showSwath(oe);
        }
    } else {
        // TODO - shall we unload the track here if there is no filter rect set ? If there is a rect we need it to filter overpasses
        if (oe.swathEntity) {
            dataSource.entities.remove(oe.swathEntity);
            oe.swathEntity = undefined;
            odinCesium.requestRender(true);
        }
    }
}

function showSwath (oe) {
    if (!oe.swathEntity) {
        oe.swathEntity = createSwathEntity(oe);
        if (oe.swathEntity) {
            dataSource.entities.add( oe.swathEntity);
            dataSource.show = true;
        }
    }
    //odinCesium.requestRender(true);
    odinCesium.render();
}

function satEntry(satId) {
    return satelliteEntries.find( e=> e.satId == satId);
}

function isSatShowing(satId) {
    let se = satEntry(satId);
    if (se) {
        if (selSatOnly) return Object.is( se, selSat);
        return se.show;
    }

    return false;
}

function satName(satId) {
    let se = satelliteEntries.find( e=> e.satId == satId);
    return se ? se.satName : undefined;
}

function hotspotConfidence (e) {
    // TODO - shall we decode the ordinals ?
    return e.conf ? e.conf : "-";
}

function pastClassifier (he) {
    if (he.nGood > 0) {
        if (he.date > now - util.MILLIS_IN_DAY) return ui.createImage("orbital-asset/fire"); // good pix within 24h
        else return "";
    } else return "";
}

function hotspotClassifier (he) {
    if (he.conf > 1) return ui.createImage("orbital-asset/fire");
    else if (he.conf > 0) return "";
    else return "";
}

function isHotspotInArea (hs) {
    let lat = hs.lat;
    let lon = hs.lon;
    return (lat > area.south) && (lat < area.north) && (lon > area.west) && (lon < area.east);
}

function updateSatellites () {
    for (let se of satelliteEntries) {
        updateSatEntryNext(se);
        updateSatEntryLast(se);
    }
}

function updateSatEntryNext (se) {
    let next = shownUpcomingEntries.find(e => (e.satId == se.satId));
    if (next) {
        se.next = next.end;
        ui.updateListItem(satelliteView, se);
    }
}

function updateSatEntryLast (se) {
    let last = shownCompletedEntries.find( e=> e.satId == se.satId);
    if (last) {
        se.prev = last.end;
        ui.updateListItem(satelliteView, se);
    }
}

function updateUpcoming() {
    shownUpcomingEntries = filterOverpasses( upcomingEntries);
    ui.setListItems(upcomingView, shownUpcomingEntries);
}

function updateCompleted() {  // FIXME
    shownCompletedEntries = filterOverpasses( completedEntries);

    if (followLatest) {
        ui.setListItems(completedView, shownCompletedEntries);
        ui.selectFirstListItem( completedView);

    } else { // try to restore selection
        let lastSel = selCompleted;
        if (lastSel && !shownCompletedEntries.includes(lastSel)) lastSel = undefined;

        ui.setListItems(completedView, shownCompletedEntries);

        if (lastSel) ui.setSelectedListItem(completedView,lastSel); // restore selection
    }
}

function filterOverpasses (overpasses) {
    let matches = [];
    for (let oe of overpasses) {
        if (isSatShowing(oe.satId)) {
            if (isAreaOverpass(oe)) {
                matches.push(oe)
            }
        }
    }
    return matches;
}

// note this returns a new array with all overpasses, sorted by ascending overpass end times
function allOverpasses() {
    return completedEntries.toReversed().concat( upcomingEntries); 
}

// the ascending end time ordered list of all showing overpasses (selected satellites)
function allShowingOverpasses() {
    return completedEntries.filter( (oe)=>isSatShowing(oe.satId)).toReversed()
        .concat( upcomingEntries.filter((oe)=>isSatShowing(oe.satId))); 
}

function isAreaOverpass(oe) {
    // check if at least one vertex is within swath of this overpass
    if (areaVertices && areaVertices.length > 0) {
        let track = oe.track;
        if (track) {
            let d2Max = util.pow2(oe.meanGpDist);

            for (let v of areaVertices) {
                let i = findClosestIndex(track, v);
                if (i >= 0) {
                    let d2 = distSquared( v, track[i]);
                    if (d2 <= d2Max) return true;
                }
            }
        } else {
            console.log("warning - overpass has no track", oe.end);
        }
        return false;
    }

    return true; // this is conservative
}

function updateHotspots() {
    // if hsList is still in flight this will be called again after it was loaded
    if (selCompleted && selCompleted.hsList && selCompleted.hsList.hotspots){ 
        ui.setListItems(hotspotView, selCompleted.hsList.hotspots);
    } else {
        ui.clearList(hotspotView);
    }
}

function handleWsMessages(msgType, msg) {
    switch (msgType) {
        case "satellites":
            handleSatelliteMessage(msg); break;
        case "overpasses":
            handleOverpassMessage(msg); break;
        case "hotspots":
            handleHotspotMessage(msg); break;
        default:
            console.log("unknown websock message ", msgType, " ignored");
            break;
    }
}

// we get this message for each websocket initialization and it is always the first (followed by overpasses and then hotspots)
function handleSatelliteMessage(satellites) {
    if (lastInit) { // this is a re-initialization (probably the websocket got re-opened)
        console.log("resetting orbiting satellite data");
        resetData();
    }
    lastInit = Date.now();

    satellites.forEach( sat=> satelliteEntries.push( new SatelliteEntry(sat)));
    ui.setListItems( satelliteView, satelliteEntries);
}

function handleOverpassMessage(overpasses) {
    // this is async - we do need the utcClock to be initialized so this has to go through the respective promise
    odinCesium.withCurrentUtcMillis( (now) => {
        let loadPromises = [];
        for (let o of overpasses) {
            let oe = new OverpassEntry(o);
            sortInOverpass(oe, now);
            loadPromises.push( loadTrack(oe));
        }

        util.withAllPromises( loadPromises, ()=>updateAll());
    });
}

function sortInOverpass (newEntry, now) {
    if (newEntry.end <= now) { // completed overpass - latest on top
        for (let i =0; i<completedEntries.length; i++) {
            if (newEntry.end > completedEntries[i].end) {
                completedEntries.splice(i,0,newEntry);
                return;
            }
        }  
        completedEntries.push( newEntry);

    } else { // upcoming overpass - next on top
        for (let i =0; i<upcomingEntries.length; i++) {
            if (newEntry.end <= upcomingEntries[i].end) {
                upcomingEntries.splice(i,0,newEntry);
                return;
            }
        }
        upcomingEntries.push( newEntry);

        if (!moveTimer) { // start the timer to check for completed overpasses
            moveTimer = setInterval( moveCompletedOverpasses, config.checkInterval);
        }
    }
}

// this returns a future that resolves once we have the full track
function loadTrack (oe) {
    let url = "orbital-data/" + oe.fname;
    return fetch(url).then( (response) => { // TODO - this should handle errors
        if (response.ok) {
            response.json().then( (fullOverpass) => {
                oe.track = fullOverpass.track;
            });
        }
    });
}

// note we have to do this periodically since setTimeout() does not properly account for system suspension/sleep
function moveCompletedOverpasses () {
    odinCesium.withCurrentUtcMillis( (now) => updateCompletedOverpasses(now));
}

function updateCompletedOverpasses (now) {
    dropOldCompletedOverpasses(now);

    let update = false;
    while (upcomingEntries.length > 0 && upcomingEntries[0].end <= now) {
        completedEntries.splice( 0, 0, upcomingEntries[0]);
        upcomingEntries.splice( 0, 1);
        update = true;
    }

    if (update) {
        updateAll();

        if ((upcomingEntries.length == 0) && moveTimer) { // nothing left to move, stop checking
            clearInterval( moveTimer);
            moveTimer = undefined;
        }
    }
}

function dropOldCompletedOverpasses (now) {
    let maxAgeInMillis = maxAgeInDays * 86400000;
    for (let i = completedEntries.length - 1; i>= 0; i--) {
        if ((now - completedEntries[i].end) > maxAgeInMillis) {
            completedEntries.pop();
        }
    }
}

function updateAll() {
    updateUpcoming();
    updateCompleted();
    updateSatellites();
}

function handleHotspotMessage(hotspots) {
    odinCesium.withCurrentUtcMillis( (now) => { // we don't have overpasses until we have the UTC clock set
        updateCompletedOverpasses(now); // first make sure we have up-to-date completedEntries

        for (let hsList of hotspots) {
            let oe = completedEntries.find( (oe) => { return (oe.satId == hsList.satId) && (oe.end == hsList.end); });
            if (oe) {
                oe.hsList = hsList;
                ui.updateListItem( completedView, oe); // might not show if filtered
                loadHotspots(oe); // trampoline action - load full hsList (with hotspots) from file
            } else {
                console.log("warning: un-associated hotspots for ", hsList,satId, " at ", util.toLocalMDHMString(hsList.end));
            }
        }
    });
}

function loadHotspots (oe) { 
    odinCesium.withTopoTerrain( ()=>{  // no point loading hotspots before we have topo terrain
        let url = "orbital-data/" + oe.hsList.fname;
        fetch(url).then( (response) => {
            if (response.ok) {
                response.json().then( (hsList) => {
                    oe.hsList = hsList;
                    computeHotspotFootprints( hsList.hotspots);
                    return hsList.hotspots.map( (h)=>h.geoPos);
                }).then( (ps)=> {
                    odinCesium.withDetailedSampledTerrain( ps, ()=>{
                        updateHotspots();
                    })
                })
            }
        })
    });
}

function computeHotspotFootprints (hotspots) {
    for (let h of hotspots) {
        let lon = h.lon;
        let lat = h.lat;

        h.geoPos = Cesium.Cartographic.fromDegrees( lon, lat);
        h.area.push( h.area[0]); // close polygon
    }
}

function showOrbital(cond) {
    showHotspotAssets(cond);
    if (areaAsset) areaAsset.show = cond;
    odinCesium.requestRender();
 }
 

function showHotspotAssets(isVisible) {
    if (hsPointPrimitive) hsPointPrimitive.show = isVisible;
    if (hsFootprintPrimitive) hsFootprintPrimitive.show = isVisible;
    odinCesium.requestRender();
}

function showHotspots() {
    if (selCompleted) {
        setHotspotAssets();
    } else {
        clearHotspotAssets();
    }
}

function setHotspotAssets() {
    clearHotspotAssets();

    if (selCompleted) { // if none is selected we don't have anything to display
        let refDate = selCompleted.end;
        let areaGeoms = [];
        let outlineGeoms = [];
        let points = [];

        let areaDCAttr = new Cesium.DistanceDisplayConditionGeometryInstanceAttribute( 0, footprintDist);
        let smallOutlineColorAttr = Cesium.ColorGeometryInstanceAttribute.fromColor( brightThresholdColor);
        let smallOutlineDCAttr = new Cesium.DistanceDisplayConditionGeometryInstanceAttribute(0, smallFootprintDist);
        let smallFootprintPointDC = new Cesium.DistanceDisplayCondition( smallFootprintDist, Number.MAX_VALUE);

        let selIdx = shownCompletedEntries.findIndex( (oe) => Object.is( selCompleted, oe));
        let i0 = showHistory ? shownCompletedEntries.length-1 : selIdx;

        for (let i = selIdx; i<= i0; i++) { // z-order of entities is in order of addition
            let oe = shownCompletedEntries[i];
            if (oe.hsList && oe.hsList.hotspots) {
                let clr = getFootprintColor( oe.end, refDate);
                if (clr) {
                    let clrAttr = Cesium.ColorGeometryInstanceAttribute.fromColor(clr);

                    for (let h of oe.hsList.hotspots) {
                        if (!area || isHotspotInArea( h)) {
                            areaGeoms.push( new Cesium.GeometryInstance({ // we always show the footprint area
                                geometry: new Cesium.PolygonGeometry({
                                    polygonHierarchy: new Cesium.PolygonHierarchy(h.area),
                                }),
                                attributes: {
                                    color: clrAttr,
                                    distanceDisplayCondition: areaDCAttr
                                }
                            }));
                            
                            if (i == selIdx) { // points and (small footprint) outlines are only shown for the selected overpass
                                let position = Cesium.Cartographic.toCartesian(h.geoPos);
                                // points are not Geometries - PointPrimitives do not support clamp-to-ground so we have project to ellipsoid surface
                                if (!odinCesium.isUsingTopoTerrain()) {
                                    Cesium.Ellipsoid.WGS84.scaleToGeodeticSurface( position, position);
                                }

                                let point = {
                                    position,
                                    pixelSize: pixelSize,
                                    color: brightThresholdColor
                                };
                                if (h.frp >= frpThreshold) {
                                    point.outlineWidth = outlineWidth;
                                    point.outlineColor = frpThresholdColor;
                                }
                                points.push(point);

                                if (isSmallFootprintHotspot(h)) { // hide point when we get close, display footprint outline instead
                                    point.distanceDisplayCondition = smallFootprintPointDC;

                                    outlineGeoms.push( new Cesium.GeometryInstance({
                                        geometry: new Cesium.GroundPolylineGeometry({
                                            positions: h.area
                                        }),
                                        attributes: {
                                            color: smallOutlineColorAttr,
                                            distanceDisplayCondition: smallOutlineDCAttr
                                        }
                                    }));
                                }
 
                            }
                        } 
                    }
                }
            }
        }

        setFootprintAssets( areaGeoms);
        setFootprintOutlineAssets( outlineGeoms);
        setPointAssets( points);
    }
}

function isSmallFootprintHotspot (h) {
    return Math.min( h.scan, h.track) < smallFootprintLength; 
}

function setFootprintAssets (geoms) {
    if (geoms.length > 0) {
        hsFootprintPrimitive = new Cesium.GroundPrimitive({
            geometryInstances: geoms,
            allowPicking: false,
            //asynchronous: true,
            //releaseGeometryInstances: true,
            //vertexCacheOptimize: true,
            
            appearance: new Cesium.PerInstanceColorAppearance({
                faceForward: true,
                flat: true,
                translucent: true,
                //renderState: { depthTest: { enabled: false, } }, // this makes it appear always on top but translucent
            }),
        });       
        odinCesium.addPrimitive(hsFootprintPrimitive);
    }
}

function setFootprintOutlineAssets (geoms) {
    if (geoms.length > 0) {
        hsOutlinePrimitive = new Cesium.GroundPolylinePrimitive({
            geometryInstances: geoms,
            allowPicking: false,
            appearance: new Cesium.PolylineColorAppearance()
        });

        odinCesium.addPrimitive( hsOutlinePrimitive);
    }
}

function setPointAssets (points) {
    if (points.length > 0) {
        hsPointPrimitive = new Cesium.PointPrimitiveCollection({
            blendOption: Cesium.BlendOption.OPAQUE
        });
        points.forEach( p=> {
            // while the Cesium doc does not mention it the PointPrimitive ctor does honor a distanceDisplayCondition
            // and hence we don't have to set it here explicitly
            hsPointPrimitive.add(p);
        });
        odinCesium.addPrimitive(hsPointPrimitive);
    }

    odinCesium.requestRender();
}

function getFootprintColor(oeDate, refDate) {
    let dt = util.hoursFromMillis(refDate - oeDate); // refDate is always > oeDate

    for (let i = 0; i < timeSteps.length; i++) {
        let ts = timeSteps[i];
        if (dt < ts.hours) {
            return ts.color;
        }
    }

    return timeSteps[timeSteps.length - 1].color; // we use the last as the catch-all
}

function createSwathEntity (oe) {
    //let earth = Cesium.Ellipsoid.WGS84;
    let cfg = config;

    // track is already in ECEF Cartesian3 coords (on ellipsoid)
    let pts = util.downSampleWithFirstAndLast(oe.track,10);

    //let cp = earth.scaleToGeodeticSurface(pts[Math.round(pts.length/2)]);
    let cp = pts[Math.round(pts.length/2)];

    let info = `${satName(oe.satId)}\n${util.toLocalDateString(oe.end)}\n${util.toLocalHMTimeString(oe.start)} - ${util.toLocalHMTimeString(oe.end)}`;

    return new Cesium.Entity( {
        position: cp,
        corridor: {
            positions: pts,
            width: 2*oe.meanSwathWidth,
            cornerType: Cesium.CornerType.MITERED,
            material: cfg.swathColor,
            height: 0,
            heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
            //distanceDisplayCondition: cfg.swathDC // Cesium BUG ? does not work correctly
        },
        polyline: {
            positions: pts,
            material: cfg.trackColor,
            clampToGround: true,
            //distanceDisplayCondition: cfg.swathDC
        },
        label: {
            text: info,
            font: cfg.font,
            fillColor: cfg.labelColor,
            //heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
            //distanceDisplayCondition: cfg.swathDC
        }
    });
}

function clearHotspotAssets() {
    if  (hsFootprintPrimitive || hsPointPrimitive || hsOutlinePrimitive){
        if (hsFootprintPrimitive) {
            odinCesium.removePrimitive(hsFootprintPrimitive);
            hsFootprintPrimitive = undefined;
        }

        if (hsOutlinePrimitive) {
            odinCesium.removePrimitive(hsOutlinePrimitive);
            hsOutlinePrimitive = undefined;     
        }
    
        if (hsPointPrimitive) {
            odinCesium.removePrimitive(hsPointPrimitive);
            hsPointPrimitive = undefined;
        }

        odinCesium.requestRender();
    }
}

//--- interaction

function clearOrbits () {
    ui.clearSelectedListItem(upcomingView);
    ui.clearListItemCheckBoxes(upcomingView);

    ui.clearSelectedListItem(completedView);
    ui.clearListItemCheckBoxes(completedView);

    clearHotspotAssets();
    odinCesium.requestRender();
}

function clearHotspots() {
    ui.clearSelectedListItem(completedView);
    clearHotspotAssets();
    odinCesium.requestRender();
}

function resetDisplayParams() {
    timeSteps = structuredClone(config.timeSteps);
    brightThreshold = structuredClone(config.bright);
    frpThreshold = structuredClone(config.frp);
}

function selectOrbitalSatellite(event) {
    selSat = ui.getSelectedListItem(satelliteView);
    if (selSatOnly) {
        updateUpcoming();
        updateCompleted();
    }
}

function toggleSelSatOnly(event) {
    selSatOnly = ui.isCheckBoxSelected(event);
    if (selSat) {
        updateUpcoming();
        updateCompleted();
    }
}

function toggleFollowLatest(event){
    followLatest = ui.isCheckBoxSelected(event);
    if (followLatest) {
        ui.selectFirstListItem( completedView);
    }
}

function toggleShowOrbital(event) {
    showHotspotAssets(ui.isCheckBoxSelected(event.target));
}

function selectCompleted(event) {
    selCompleted = ui.getSelectedListItem(completedView);
    updateHotspots(); 
    showHotspots();
}

function zoomToHotspot(event) {
    let h = ui.getSelectedListItem(event);
    if (h) {
        odinCesium.zoomTo(Cesium.Cartesian3.fromDegrees(h.lon, h.lat, zoomHeight));
    }
}

function toggleShowHistory(event) {
    showHistory = ui.isCheckBoxSelected(event.target);
    if (selCompleted) {
        showHotspots();
    }
}

//--- interactive area selection

function clearArea() {
    if (area) {
        ui.setField("orbital.bounds", null);
        area = undefined;
        areaVertices = undefined;
    }
    if (areaAsset) {
        odinCesium.removeEntity(areaAsset);
        areaAsset = undefined;
    }

    ui.setVarText(areaInfoText, null);

    updateAll();
    showHotspots();
    odinCesium.requestRender();
}

function pickArea(event) { // mouse selection
    odinCesium.enterGeoRect( (rect) => {
        ui.setField("orbital.bounds", util.degreesToString([rect.west, rect.south, rect.east, rect.north], util.fmax_3));
        setArea(rect)
    });
}

function setAreaFromInput(event) { // text field input (WSEN)
    let input = event.target.value;
    if (input && input.length > 0) {
        let a = input.split(',');
        let west = parseFloat(a[0]);
        let south = parseFloat(a[1]);
        let east = parseFloat(a[2]);
        let north = parseFloat(a[3]);

        if (isNaN(west) || isNaN(south) || isNaN(east) || isNaN(north)) {
            alert("invalid input (need west,south,east,north as comma separated degrees");
        } else {
            rect = new main.GeoRect( west,south,east,north);
            setArea(rect)
        }
    }
}

function setAreaFromSelection(event){
    let sharedItem = ui.getSelectedListItem(sharedAreasView);
    if (sharedItem) {
        let rect = sharedItem.value.data;
        ui.setField("orbital.bounds", util.degreesToString([rect.west, rect.south, rect.east, rect.north], util.fmax_3));
        setArea( rect);

    } else {
        clearArea();
    }
}

function setArea (rect) {
    area = rect;
    areaVertices = odinCesium.cartesian3ArrayFromDegreesRect(rect);

    setAreaInfo();
    setAreaAsset();

    updateAll();
}

function setAreaInfo () {
    if (area) {
        let du = util.distanceBetweenGeoPos( area.west, area.north, area.east, area.north);
        let dv = util.distanceBetweenGeoPos( area.west, area.north, area.west, area.south);
        let sqAcres = util.fmax_0.format(util.squareMetersToAcres( du * dv));
        let duMi = util.fmax_1.format(util.metersToUsMiles(du));
        let dvMi = util.fmax_1.format(util.metersToUsMiles(dv));
        ui.setVarText(areaInfoText, `${duMi} × ${dvMi} miles, ${sqAcres} acres`);
    }
}

function setAreaAsset() {
    if (areaAsset) {
        odinCesium.removeEntity(areaAsset);
    }

    areaAsset = new Cesium.Entity({
        polyline: {
            positions: areaVertices,
            clampToGround: true,
            width: 1,
            material: Cesium.Color.YELLOW
        },
        selectable: false
    });
    odinCesium.addEntity(areaAsset);
}


function zoomToArea(event) {
    if (area) {
        let rect = Cesium.Rectangle.fromDegrees( area.west, area.south, area.east, area.north);
        let cameraPos = odinCesium.viewer.camera.getRectangleCameraCoordinates(rect);
        odinCesium.zoomTo(cameraPos);
    }
}

//--- layer parameters

function setOrbitalBrightThreshold(event) {
    brightThreshold = ui.getSliderValue(event.target);
    if (hsPointPrimitive) {
        showHotspots(ui.getSelectedListItemIndex(completedView));
    }
}

function setOrbitalFrpThreshold(event) {
    frpThreshold = ui.getSliderValue(event.target);
    if (hsPointPrimitive) {
        showHotspots(ui.getSelectedListItemIndex(completedView));
    }
}

function setOrbitalPixelSize(event) {
    pixelSize = ui.getSliderValue(event.target);
    if (hsPointPrimitive) {
        const len = hsPointPrimitive.length;
        for (let i = 0; i < len; ++i) {
            hsPointPrimitive.get(i).pixelSize = pixelSize;
        }
        odinCesium.requestRender();
    }
}

function setOrbitalOutlineWidth(event) {
    outlineWidth = ui.getSliderValue(event.target);
    if (hsPointPrimitive) {
        const len = hsPointPrimitive.length;
        for (let i = 0; i < len; ++i) {
            hsPointPrimitive.get(i).outlineWidth = outlineWidth;
        }
        odinCesium.requestRender();
    }
}

function setOrbitalHistory(event) {
    maxAgeInDays = ui.getSliderValue(event.target);
    if (hsFootprintPrimitive && showHistory) {
        showHotspots();
    }
}

// both ps and p are Cartesian3
function findClosestIndex (ps, p) {
    let len = ps.length;

    // corner cases
    if (len == 0) { return -1; }
    if (len == 1) { return 0 } // only choice
    if (len == 2) { return distSquared( ps[1], p) > distSquared( ps[0], p) ? 0 : 1 }

    let l = 1;
    let r = len-2;
    let i = Math.trunc(r/2);

    let di = distSquared( ps[i], p);
    let dl = di - distSquared( ps[i-1], p);
    let dr = distSquared( ps[i+1], p) - di;

    while (Math.sign(dl) == Math.sign(dr)) {
        if (dr < 0.0) {  // bisect right
            l = i;
          } else {  // bisect left
            r = i;
          }
          let i_last = i;
          i = Math.trunc((l + r)/2);
          if (i == i_last) { break; }
    
          di = distSquared( ps[i], p);
          dl = di - distSquared( ps[i-1], p);
          dr = distSquared( ps[i+1], p) - di;
    }

    return i;
}

// both p and q are Cartesian3
function distSquared (p, q) {
    let dx = p.x - q.x;
    let dy = p.y - q.y;
    let dz = p.z - q.z;

    return (dx*dx + dy*dy + dz*dz);
}

//--- shared items

var shareInitialized = false;

function handleShareMessage (msg) {
    if (msg.SHARE_INITIALIZED) { // we get that no matter what the share implementation is
        shareInitialized = true;
        updateSharedAreas();

    } else if (shareInitialized) { // if we aren't initialized yet there is no need for updating the view
        if (msg.setShared) {
            let sharedItem = msg.setShared;
            if (sharedItem.key.match(BBOX_PATTERN)) {
                updateSharedAreas(); // TODO - don't do a sledge hammer approach (just add/delete items)
            }
        }
    }
}

function updateSharedAreas() {
    let areas = getSharedAreas();
    let tree = data.ExpandableTreeNode.from( areas, e=>e.key );
    ui.setTree( sharedAreasView, tree);
}

const BBOX_PATTERN = util.glob2regexp("{bbox/**,**/bbox/**,**/bbox}"); // any pathname with 'bbox' in it

function getSharedAreas() {
    let areas = [];
    let items = main.getAllMatchingSharedItems( BBOX_PATTERN);
    for (let item of items) {
        if (item.value.type == "GeoRect") {
            areas.push( item);
        }
    }

    return areas;
}