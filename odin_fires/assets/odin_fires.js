/*
 * Copyright (c) 2023, United States Government, as represented by the
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

/*
 * this is still based on the "Fire Progression" database from https://data-nifc.opendata.arcgis.com/
 * we are moving to the "Historical Operational Data" database (which is structured around polygons/lines/points) the structure will change
 */

import { config } from "./odin_fires_config.js";

import * as util from "../odin_server/ui_util.js";
import * as ui from "../odin_server/ui.js";
import * as ws from "../odin_server/ws.js";
import * as odinCesium from "../odin_cesium/odin_cesium.js";

const MOD_PATH = "odin_fires::fire_service::FireService";


//--- data section

class FireEntry {
    constructor (fireSummary) {
        this.fireSummary = fireSummary;
        fireSummary.perimeters.forEach( p=> {
            p.datetime = Date.parse(p.datetime);  // convert string into timestamp
            p.id = util.toYYYYMMDDhhmmZString(p.datetime);
        });
    }

    // explicit loading/unloading of perimeters assets
    displayPerimeter (perimeter,showIt) {
        let ds = perimeter.ds;
        if (ds) {
            if (ds.show != showIt) {
                ds.show = showIt;
                if (!showIt) { // free resources
                    odinCesium.removeDataSource(ds);
                    perimeter.ds = null;
                }

                odinCesium.requestRender();
                ui.updateListItem(firePerimeterView,perimeter);
            }
        } else {
            loadPerimeter( this, perimeter); // this is async
        }
    }

    // this only toggles visibility but does not load/unload assets
    setPerimeterVisibiity (showIt) {
        this.fireSummary.perimeters.forEach ( p=> {
            if (p.ds) {
                p.ds.show = showIt;
            }
        });
    }
}

var years = [];
var fireEntries = [];

function numberOfFiresInYear (yr) {
    var n = 0;
    fireEntries.forEach( e=> {if (e.fireSummary.year == yr) n++;} );
    return n;
}

function getFireDataItems (e) {
    let fs = e.fireSummary;
    return [
        ["name",fs.name],
        ["unique-id", fs.uniqueId],
        ["irwin-id", fs.irwinId],
        ["inciweb-id", fs.inciwebId],
        ["start", fs.start],
        ["contained", fs.contained],
        ["end", fs.end],
        //... more to follow
    ];
}

async function loadPerimeter (e, perimeter) {
    let url = `fire-data/${e.fireSummary.year}/${e.fireSummary.name}/perimeters/${perimeter.id}.geojson`;
    
    fetch(url).then( (response) => {
        if (response.ok) {
            let data = response.json();
            if (data) {
                let renderOpts = getPerimeterRenderOptions();
                //renderOpts.clampToGround = false; // HACK to make outline visible

                Cesium.GeoJsonDataSource.load(data, renderOpts).then( (ds) => {
                    for (const e of ds.entities.values) {
                        if (e.polygon) {
                            if (!e.polyline) {
                                e.polyline = {
                                    positions: e.polygon.hierarchy._value.positions,
                                    material: renderOpts.strokeColor,
                                    width: renderOpts.strokeWidth,
                                    clampToGround: true
                                };
                                e.addProperty('polyline');
                                e.polygon.outline = false;
                            }
                        }
                    }

                    perimeter.ds = ds;
                    ds.show = true;
                    odinCesium.addDataSource(ds);
                    ui.updateListItem(firePerimeterView,perimeter);

                    console.log("loaded ", url);
                    updatePerimeterRendering(false);
                });
            } else console.log("no data for request: ", url);
        } else console.log("request failed: ", url);
    }, (reason) => console.log("failed to retrieve: ", url, ", reason: ", reason));
}


function releasePerimeterAssets () {
    if (selectedFire) {
        selectedFire.fireSummary.perimeters.forEach( (p)=>{
            if (p.ds) {
                p.ds.show = false;
                odinCesium.removeDataSource(p.ds);
                p.ds = null;
            }
        });
    }
}

//--- websocket (data) interface

function handleWsMessages(msgType, msg) {
    switch (msgType) {
        case "fireSummary":
            handleFireSummaryMessage(msg);
            return true;

        default:
            return false;
    }
}

function handleFireSummaryMessage (fireSummary) {
    let e = new FireEntry(fireSummary);
    fireEntries.push( e);
    util.sortInUnique(years, fireSummary.year);

    ui.setListItems(fireYearView, years);
}

//--- UI initialization

var fireYearView = undefined;
var selectedYear = undefined;

var fireListView = undefined;
var selectedFire = undefined;

var firePerimeterView = undefined;
var selectedPerimeter = undefined;

var fireInfoView = undefined;

var stepThroughMode = false;
var syncTimelines = false;

var perimeterRender = { ...config.perimeterRender };

createIcon();
createWindow();
fireYearView = initFireYearView();
fireListView = initFireListView();
firePerimeterView = initFirePerimeterView();
fireInfoView = ui.getKvTable("fires.info");
initFirePerimeterDisplayControls();

ws.addWsHandler( MOD_PATH, handleWsMessages);

odinCesium.initLayerPanel("fires", config, showFires);
console.log("fires initialized");

//--- end initialization

function createIcon() {
    return ui.Icon("./asset/odin_fires/fires-icon.svg", (e)=> ui.toggleWindow(e,'fires'), "operational fire data");
}

function createWindow() {
    let nItems = 8;

    let view = ui.Window("Operational Fire Data", "fires", "./asset/odin_fires/fires-icon.svg")(
        ui.LayerPanel("fires", toggleShowFires),
        ui.Panel("fires", true)(
            ui.RowContainer()(
                ui.List("fires.years", 5, selectYear),
                ui.HorizontalSpacer(0.5),
                ui.List("fires.fires", 5, selectFire,null,null,zoomToFire)
            )
        ),
        ui.Panel("fire info", false)(
            ui.KvTable("fires.info", 15, 25,25)
        ),
        ui.Panel("ignition points", false)(
            ui.List("fires.ignitions", 5)
        ),
        ui.Panel("timelines", true) (
            ui.TabbedContainer()(
                ui.Tab("perimeters", true)( ui.List("fires.perimeters", nItems, selectPerimeter) ),
                ui.Tab("containment", false)( ui.List("fires.containment", nItems) ),
                ui.Tab("events", false)( ui.List("fires.events", nItems) ),
                ui.Tab("resources", false)( ui.List("fires.resources", nItems) ),
                ui.Tab("firelines", false)( ui.List("fires.firelines", nItems) ),
                ui.Tab("wind", false)( ui.List("fires.wind", nItems) ),
            ),
            ui.RowContainer()(
                ui.CheckBox("sync timelines", toggleSyncTimelines),
                ui.CheckBox("step through", setStepThrough),
                ui.ListControls("fires.perimeters",null,null,null,null,clearPerimeters)
            )
        ),
        ui.Panel("display parameters", false)(
            ui.Slider("stroke width", "fires.perimeter.stroke_width", perimeterStrokeWidthChanged),
            ui.ColorField("stroke color", "fires.perimeter.stroke_color", true, perimeterStrokeColorChanged),
            ui.ColorField("fill color", "fires.perimeter.fill_color", true, perimeterFillColorChanged),
            ui.Slider("fill opacity", "fires.perimeter.opacity", perimeterFillOpacityChanged),
            ui.Slider("dim factor", "fires.perimeter.dim_factor", perimeterDimFactorChanged),
        )
    );

    return view;
}

function initFireYearView() {
    let view = ui.getList("fires.years");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "year", tip: "fire season", width: "4rem", attrs: ["fixed"], map: e => e },
            { name: "fires", tip: "number of fires", width: "2rem", attrs: ["fixed", "alignRight"], map: e => numberOfFiresInYear(e) }
        ]);
    }
    return view;
}

function initFireListView() {
    let view = ui.getList("fires.fires");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "name", tip: "name of fire", width: "8rem", attrs:[], map: e=> e.fireSummary.name },
            { name: "start", tip: "start date", width: "7rem", attrs:["fixed"], map: e=> e.fireSummary.start },
            { name: "acres", tip: "area in [acre]", width: "4rem", attrs: ["fixed", "alignRight"], map: e => e.fireSummary.acres }
        ]);
    }
    return view;
}

function initFirePerimeterView() {
    let view = ui.getList("fires.perimeters");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "show", tip: "show/hide perimeter", width: "2.5rem", attrs: [], map: e => ui.createCheckBox(e.ds && e.ds.show, toggleShowPerimeter) },
            { name: "dtg", tip: "local date/time of perimeter", width: "7rem", attrs:["fixed"], map: e=> util.toLocalMDHMString(e.datetime) },
            { name: "acres", tip: "size in acres", width: "4rem", attrs:["fixed", "alignRight"], map: e=> e.acres },
            ui.listItemSpacerColumn(),
            { name: "agency", tip: "source of perimeter", width: "4rem", attrs: [], map: e => e.agency },
            { name: "method", tip: "method of perimeter determination", width: "7rem", attrs: ["small"], map: e => e.method }
        ]);
    }
    return view;
}

function initFireContainmentView() {
    let view = ui.getList("fires.perimeters");
    if (view) {
        ui.setListItemDisplayColumns(view, ["fit", "header"], [
            { name: "dtg", tip: "local date/time of perimeter", width: "7rem", attrs:["fixed"], map: e=> util.toLocalMDHMString(e.datetime) },
            { name: "acres", tip: "size in acres", width: "4rem", attrs:["fixed", "alignRight"], map: e=> e.acres },
            { name: "cnt", tip: "% containment", width: "3rem", attrs:["fixed", "alignRight"], map: e=> e.percent },
            { name: "", tip: "", width: "15rem", attrs:[], map: e => ui.createProgressBar(getSizePercent(e.acres), e.percent) }
        ]);
    }
    return view;
}

function getSizePercent(curSize) {
    if (selectFire) {
        let finalArea = selectedFire.fireSummary.acres;
        return curSize * 100 / finalArea;
    } else {
        return 0;
    }
}

function initFirePerimeterDisplayControls() {
    let e = ui.getSlider('fires.perimeter.stroke_width');
    ui.setSliderRange(e, 0, 5.0, 0.5, util.f_1);
    ui.setSliderValue(e, perimeterRender.strokeWidth);

    e = ui.getSlider('fires.perimeter.opacity');
    ui.setSliderRange(e, 0, 1.0, 0.1, util.f_1);
    ui.setSliderValue(e, perimeterRender.fillOpacity);

    e = ui.getSlider('fires.perimeter.dim_factor');
    ui.setSliderRange(e, 0, 2.0, 0.1, util.f_1);
    ui.setSliderValue(e, perimeterRender.dimFactor);

    e = ui.getField("fires.perimeter.stroke_color");
    ui.setField(e, perimeterRender.strokeColor.toCssHexString());

    e = ui.getField("fires.perimeter.fill_color");
    ui.setField(e, perimeterRender.fillColor.toCssHexString());
}

//--- UI callbacks

function selectYear(event) {
    selectedYear = ui.getSelectedListItem(fireYearView);
    updateFireListView();
    updateFirePerimeterView();
}

function updateFireListView() {
    let fires = fireEntries.filter( e=> e.fireSummary.year == selectedYear );
    fires.sort( (a,b) => { util.defaultCompare(a.fireSummary.start, b.fireSummary.start); });
    ui.setListItems(fireListView, fires);
}

function selectFire(event) {
    selectedFire = ui.getSelectedListItem(fireListView);
    updateFireDataView();
    updateFirePerimeterView();
}

function zoomToFire(event) {
    if (selectedFire) {
        let pos = selectedFire.fireSummary.location;
        odinCesium.zoomTo(Cesium.Cartesian3.fromDegrees(pos.lon, pos.lat, config.zoomHeight));
    }
}

function updateFirePerimeterView() {
    if (selectedFire) {
        ui.setListItems(firePerimeterView, selectedFire.fireSummary.perimeters);
    } else {
        ui.clearList(firePerimeterView);
    }
}

function selectPerimeter(event) {
    let prevPerimeter = selectedPerimeter;
    selectedPerimeter = ui.getSelectedListItem(firePerimeterView);
    if (stepThroughMode) {
        if (selectedPerimeter) {
            if (prevPerimeter && prevPerimeter.ds) selectedFire.displayPerimeter(prevPerimeter,false);
            ui.updateListItem(firePerimeterView,prevPerimeter);

            if (selectedFire) selectedFire.displayPerimeter(selectedPerimeter,true);
            ui.updateListItem(firePerimeterView,selectedPerimeter);
        }
    } 
}

function updateFireDataView() {
    let kvList = selectedFire ? getFireDataItems(selectedFire) : null;
    ui.setKvList(fireInfoView, kvList);
}

function toggleShowPerimeter(event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let perimeter = ui.getListItemOfElement(cb);
        if (perimeter && selectedFire) {
            selectedFire.displayPerimeter(perimeter, ui.isCheckBoxSelected(cb));
        }
    }
}

function setStepThrough(event) {
    stepThroughMode = ui.isCheckBoxSelected(event.target);
}

function toggleSyncTimelines(event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        syncTimelines = ui.isCheckBoxSelected(cb);
    }
}

function clearPerimeters () {
    ui.clearSelectedListItem(firePerimeterView);
    ui.clearListItemCheckBoxes(firePerimeterView);

    releasePerimeterAssets();
    odinCesium.requestRender();
}

function toggleShowFires(event) {
    let showIt = ui.isCheckBoxSelected(event.target);
    fireEntries.forEach( (fe)=>fe.setPerimeterVisibility( showIt));
    odinCesium.requestRender();
}

function showFires(cond) {
    fireEntries.forEach( fe=> {
        fe.setPerimeterVisibiity( cond)
    });
    odinCesium.requestRender();
}

//--- interactive display parameters

function perimeterStrokeWidthChanged(event) {
    perimeterRender.strokeWidth = ui.getSliderValue(event.target);
    updatePerimeterRendering(false);
}

function perimeterStrokeColorChanged(event) {
    let clrSpec = event.target.value;
    if (clrSpec) {
        perimeterRender.strokeColor = Cesium.Color.fromCssColorString(clrSpec);
        updatePerimeterRendering(false);
    }
}

function perimeterFillOpacityChanged(event) {
    perimeterRender.fillOpacity = ui.getSliderValue(event.target);
    updatePerimeterRendering(false);
}

function perimeterFillColorChanged(event) {
    let clrSpec = event.target.value;
    if (clrSpec) {
        perimeterRender.fillColor = Cesium.Color.fromCssColorString(clrSpec);
        updatePerimeterRendering(false);
    }
}

function perimeterDimFactorChanged(event) {
    perimeterRender.dimFactor = ui.getSliderValue(event.target);
    updatePerimeterRendering(false);
}

function updatePerimeterRendering(onlyPrevious=true) {
    if (selectedFire) {
        let dimFactor = 1.0;
        let skipFirst = onlyPrevious;

        let perimeters = selectedFire.fireSummary.perimeters;
        for (let i=perimeters.length-1; i>=0; i--) { // go in reverse
            let ds = perimeters[i].ds;
            if (ds && ds.show) {
                if (skipFirst) {
                    skipFirst = false;
                } else {
                    let renderOpts = getPerimeterRenderOptions(dimFactor);
                    ds.entities.values.forEach( e=> {
                        if (e.polygon) {
                            e.polygon.material = renderOpts.fill;
                            //e.polygon.outline = true;
                            //e.polygon.outlineWidth = renderOpts.strokeWidth;
                            //e.polygon.outlineColor = renderOpts.stroke;
                        }
                        if (e.polyline) {
                            e.polyline.width = renderOpts.strokeWidth;
                            e.polyline.material = renderOpts.stroke;
                        }
                    });
                }
                dimFactor *= perimeterRender.dimFactor;
            }
        }

        odinCesium.requestRender();
    }
}

function getPerimeterRenderOptions(dimFactor=1.0) {
    return {
        fill: perimeterRender.fillColor.withAlpha( perimeterRender.fillOpacity * dimFactor),
        stroke: perimeterRender.strokeColor.withAlpha(dimFactor),
        strokeWidth: perimeterRender.strokeWidth
    };
}