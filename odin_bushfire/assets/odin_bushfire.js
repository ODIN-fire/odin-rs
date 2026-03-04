/*
 * Copyright (c) 2026, United States Government, as represented by the
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

import { config } from "./odin_bushfire_config.js";

import * as util from "../odin_server/ui_util.js";
import * as data from "../odin_server/ui_data.js";
import * as ui from "../odin_server/ui.js";
import * as ws from "../odin_server/ws.js";
import * as odinCesium from "../odin_cesium/odin_cesium.js";

const MOD_PATH = "odin_bushfire::service::BushfireService";

var perimeterRender = { ...config.perimeterRender };

var fireView = undefined;
var perimeterView = undefined;
var infoView = undefined;

var filterCb = {};
var filter = {}; // the current state of filter checkboxes
var selectedFireId = undefined;

var nFire = 1; // a running counter of new fires (we use as a short display id)

var bushfires = new Map();  // id -> RingBuffer(Bushfire)

createIcon();
createWindow();

initFireView();
initPerimeterView();

var dataSource = odinCesium.createDataSource("bushfire", config.layer.show);

ws.addWsHandler( MOD_PATH, handleWsMessages);
odinCesium.setEntitySelectionHandler( fireSelection);
odinCesium.initLayerPanel("fires", config, showFires);
console.log("bushfires initialized");

//--- websocket message handling

function handleWsMessages(msgType, msg) {
    switch (msgType) {
        case "snapshot":
            handleSnapshotMessage(msg);
            return true;
        case "update":
            handleUpdateMessage(msg);
            return true;
        default:
            return false;
    }
}

function handleSnapshotMessage(snapshot) {
    updateBushfires(snapshot.bushfires);
}

function handleUpdateMessage(update) {
    updateBushfires(update.bushfires);
}

function updateBushfires(newFires) {
    if (newFires.length > 0) {
        for (let f of newFires) {
            let hist = bushfires.get(f.id);
            if (!hist) { // a new one
                f._id = "#" + nFire++;
                hist = new data.CircularBuffer(config.maxEntries);
                bushfires.set(f.id, hist);
            } else { // we saw it before - copy the internal _id
                if (hist.length > 0) {
                    let last = hist.last();
                    f._id = last._id;
                    if (last.entity) {
                        dataSource.entities.remove(last.entity);
                        last.entity = null;
                    }
                } else {
                     f._id = "#" + nFire++;
                }
            }
            f.entity = createSymbolEntity(f);
            hist.push(f);
        }

        updateFireView();
    }
}

//--- view initialization

function createIcon() {
    return ui.Icon("./asset/odin_bushfire/fires-icon.svg", (e)=> ui.toggleWindow(e,'fires'), "bushfire data");
}

function createWindow() {
    let view = ui.Window("Bushfires", "fires", "./asset/odin_bushfire/fires-icon.svg")(
        ui.LayerPanel("fires", toggleShowFires),
        ui.Panel("fires", true)(
            ui.RowContainer("start")(
                ui.Button("all", selAll),
                ui.Button("none", selNone),
                ui.HorizontalSpacer(2),
                (filterCb["ACT"] = ui.CheckBox("ACT", updateFireView, "fires.sel.ACT", true)),
                (filterCb["NSW"] = ui.CheckBox("NSW", updateFireView, "fires.sel.NSW", true)),
                (filterCb["NT"] = ui.CheckBox("NT", updateFireView, "fires.sel.NT", true)),
                (filterCb["QLD"] = ui.CheckBox("QLD", updateFireView, "fires.sel.QLD", true)),
                (filterCb["SA"] = ui.CheckBox("SA", updateFireView, "fires.sel.SA", true)),
                (filterCb["TAS"] = ui.CheckBox("TAS", updateFireView, "fires.sel.TAS", true)),
                (filterCb["VIC"] = ui.CheckBox("VIC", updateFireView, "fires.sel.VIC", true)),
                (filterCb["WA"] = ui.CheckBox("WA", updateFireView, "fires.sel.WA", true))
            ),
            ui.RowContainer("start")(
                (filterCb["Bushfire"] = ui.CheckBox("bush", updateFireView, "fires.sel.bush", true)),
                (filterCb["VegetationFire"] = ui.CheckBox("veg", updateFireView, "fires.sel.veg", true)),
                (filterCb["PowerPoleFire"] = ui.CheckBox("pwr", updateFireView, "fires.sel.pwr", true)),
                (filterCb["CurrentBurntArea"] = ui.CheckBox("burnt", updateFireView, "fires.sel.burnt", true)),
                (filterCb["PrescribedBurn"] = ui.CheckBox("rx", updateFireView, "fires.sel.rx", true)),
                (filterCb["Unknown"] = ui.CheckBox("?", updateFireView, "fires.sel.unknown", true)),
                ui.HorizontalSpacer(2),
                (filterCb["large"] = ui.CheckBox(">100", updateFireView, "fires.sel.1000", true)),
                (filterCb["medium"] = ui.CheckBox("[10-100]", updateFireView, "fires.sel.100", true)),
                (filterCb["small"] = ui.CheckBox("[0-10]", updateFireView, "fires.sel.10", true))
            ),
            (fireView = ui.List("fires.fires", 15, selectFire, null, null, zoomToSelection)),
            (perimeterView = ui.List("fires.perimeter", 5, null, null, null, zoomToSelection)),
            ui.ListControls("fires.perimeter",null,null,null,null,clearAllPerimeters)
        ),
        ui.Panel("fire info", false)(
            (infoView = ui.KvTable("fires.info", 15, "35rem", "35rem"))
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

function initFireView() {
    ui.setListItemDisplayColumns(fireView, ["fit", "header"], [
        { name: "ty", tip: "bushfire type (🔥:bushfire, 〽️:vegetation, ⚡️:power pole, ♨️:burned area, ℞:prescribed, ❓:unknown)", width: "2rem", attrs: [], map: e => typeSymbol(e) },
        { name: "id", tip: "internal id", width: "3rem", attrs: [], map: e => e._id },
        { name: "state", tip: "", width: "3rem", attrs: [], map: e => e.state },
        { name: "name", tip: "fire name", width: "15rem", attrs: [], map: e => util.maxString(e.name, 25) },
        ui.listItemSpacerColumn(0.5),
        { name: "area", tip: "fire area [ha]", width: "4rem", attrs: ["fixed", "alignRight"], map: e => util.f_0.format(e.area) },
        { name: "upd", tip: "number of updates", width: "2rem", attrs: ["fixed", "alignRight"], map: e => bushfires.get(e.id).length },
        { name: "date", tip: "local date of update", width: "7.5rem", attrs:["fixed", "alignRight"], map: e=> util.toLocalMDHMString(e.date) },
    ]);
}

function initPerimeterView() {
    ui.setListItemDisplayColumns(perimeterView, ["fit", "header"], [
        { name: "show", tip: "show perimeter", width: "3rem", attrs: [], map: e => ui.createCheckBox(e.perimeterDs && e.perimeterDs.show, toggleShowPerimeter, null) },
        { name: "area", tip: "fire area [ha]", width: "4rem", attrs: ["fixed", "alignRight"], map: e => util.f_1.format(e.area) },
        { name: "perim", tip: "fire perimeter [km]", with: "4rem", attrs: ["fixed", "alignRight"], map: e => util.f_1.format( e.perimeter) },
        { name: "lat", width: "5.5rem", attrs: ["fixed", "alignRight"], map: (e) => util.f_3.format(e.lat) },
        { name: "lon", width: "5.5rem", attrs: ["fixed", "alignRight"], map: (e) => util.f_3.format(e.lon) },
        { name: "date", tip: "local date of update", width: "8rem", attrs:["fixed", "alignRight"], map: e=> util.toLocalMDHMString(e.date) },
    ]);
}

function typeSymbol(e) {
    switch (e.fireType) {
        case "PrescribedBurn": return "℞";
        case "Bushfire": return "🔥";
        case "VegetationFire": return "〽️";
        case "CurrentBurntArea": return "♨️";
        case "PowerPoleFire": return "⚡️";
        default: return "❓";
    }
}

//--- view update

function updateFireView() {
    setCurrentFilter();
    let list = [];
    dataSource.entities.removeAll();

    for (let hist of bushfires.values()) {
        let latest = hist.last();
        if (latest) {
            if (passFilter(latest)) {
                util.sortIn(list, latest, (a, b) => a.name > b.name);
                dataSource.entities.add(latest.entity);
            }
        }
    }
    ui.setListItems(fireView, list);

    if (selectedFireId) {
        let sel = list.find((e) => e.id == selectedFireId);
        if (sel) {
            ui.setSelectedListItem(sel);
        } else { // not visible anymore
            selectedFireId = undefined;
        }
        updatePerimeterView();
    }
}

function updatePerimeterView() {
    if (selectedFireId) {
        ui.setListItems(perimeterView, bushfires.get(selectedFireId).toReversed());
    } else {
        ui.clearList(perimeterView);
    }
}

function selAll() {
    for (let key of Object.keys(filterCb)){
        ui.setCheckBox(filterCb[key], true);
    }
    updateFireView();
}

function selNone() {
    for (let key of Object.keys(filterCb)){
        ui.setCheckBox(filterCb[key], false);
    }
    updateFireView();
}

function setCurrentFilter() {
    for (let key of Object.keys(filterCb)){
        filter[key] = ui.isCheckBoxSelected(filterCb[key]);
    }
}

function passFilter(fire) {
    if (!filter[fire.state]) return false;
    if (!filter[fire.fireType]) return false;

    let area = fire.area;
    if (area < 10 && !filter.small) return false;
    if (area >=10 && area < 100 && !filter.medium) return false;
    if (area > 100 && !filter.large) return false;

    return true;
}

function createSymbolEntity(fire) {
    let entity = new Cesium.Entity({
        id: fire._id,
        position: Cesium.Cartesian3.fromDegrees( fire.lon, fire.lat),
        billboard: {
            image: "./asset/odin_bushfire/fire.png",
            distanceDisplayCondition: config.billboardDC,
            color: config.billboardColor,
            heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
            disableDepthTestDistance: Number.MAX_SAFE_INTEGER,  // otherwise symbol might get clipped by terrain
            alignedAxis: Cesium.Cartesian3.UNIT_Z,
        },
        label: {
            text: fire._id,
            //scale: 0.8,
            horizontalOrigin: Cesium.HorizontalOrigin.LEFT,
            verticalOrigin: Cesium.VerticalOrigin.TOP,
            font: config.labelFont,
            fillColor: config.labelColor,
            showBackground: true,
            backgroundColor: config.labelBackground,
            backgroundPadding: new Cesium.Cartesian2( 3,3),
            pixelOffset: config.labelOffset,
            distanceDisplayCondition: config.labelDC,
            heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
            disableDepthTestDistance:Number.MAX_SAFE_INTEGER,
        },
        point: {
            pixelSize: config.pointSize,
            color: config.pointColor,
            outlineColor: config.pointOutlineColor,
            outlineWidth: config.pointOutlineWidth,
            //heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
            //disableDepthTestDistance:Number.MAX_SAFE_INTEGER,
            distanceDisplayCondition: config.pointDC,
        }
    });
    entity._uiFire = fire; // backlink for selection

    return entity
}

function zoomToSelection(event) {
    let sel = ui.getSelectedListItem(event);
    if (sel) {
        odinCesium.zoomTo(Cesium.Cartesian3.fromDegrees(sel.lon, sel.lat, config.zoomHeight));
    }
}

function selectFire(e) {
    let fire = ui.getSelectedListItem(fireView);
    if (fire) {
        selectedFireId = fire.id;
        ui.setKvList(infoView, getKvList(fire));
    } else {
        selectedFireId = undefined;
        ui.clearKvList( infoView);
    }
    updatePerimeterView();
}

function getKvList(fire) {
    return [
        ["id", fire.id],
        ["name", fire.name],
        ["agency", fire.agency]
    ];
}

// entity -> list
function fireSelection() {
    let sel = odinCesium.getSelectedEntity();
    if (sel && sel._uiFire) {
        ui.setSelectedListItem(fireView, sel._uiFire);
    }
}

function toggleShowPerimeter(event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        let fire = ui.getListItemOfElement(cb);
        if (ui.isCheckBoxSelected(cb)) {
            if (!fire.perimeterDs) {
                loadPerimeter(fire); // async
            }
        } else {
            if (fire.perimeterDs) {
                odinCesium.removeDataSource(fire.perimeterDs);
                fire.perimeterDs = null;
                odinCesium.requestRender();
            }
        }
    }
}

async function loadPerimeter (fire) {
    let url = `bushfire-data/${fire.filename}`;

    fetch(url).then( (response) => {
        if (response.ok) {
            let data = response.json();
            if (data) {
                //renderOpts.clampToGround = false; // HACK to make outline visible

                Cesium.GeoJsonDataSource.load(data, perimeterRender).then( (ds) => {
                    for (const e of ds.entities.values) {
                        if (e.polygon) {
                            if (!e.polyline) {
                                e.polyline = {
                                    positions: e.polygon.hierarchy._value.positions,
                                    material: perimeterRender.strokeColor,
                                    width: perimeterRender.strokeWidth,
                                    clampToGround: true
                                };
                                e.addProperty('polyline');
                                e.polygon.outline = false;
                                e.polygon.material = perimeterRender.fillColor.withAlpha(perimeterRender.fillOpacity);
                            }
                        }
                    }

                    fire.perimeterDs = ds;
                    ds.show = true;
                    odinCesium.addDataSource(ds);
                    ui.updateListItem(perimeterView,fire);

                    console.log("loaded ", url);
                    //updatePerimeterRendering(false);
                });
            } else console.log("no data for request: ", url);
        } else console.log("request failed: ", url);
    }, (reason) => console.log("failed to retrieve: ", url, ", reason: ", reason));
}

function toggleShowFires(event) {
    let showIt = ui.isCheckBoxSelected(event.target);
    dataSource.show = showIt;
    showPerimeters(showIt);
    odinCesium.requestRender();
}

function showFires(cond) {
    dataSource.show = cond;
    showPerimeters(cond);
    odinCesium.requestRender();
}

function showPerimeters(cond) {
    bushfires.forEach((hist, id, map) => {
        hist.forEach((fire) => {
            if (fire.perimeterDs) {
                fire.perimeterDs.show = cond;
            }
        })
    });
}

function clearAllPerimeters() {
    bushfires.forEach((hist, id, map) => {
        hist.forEach((fire) => {
            if (fire.perimeterDs) {
                odinCesium.removeDataSource(fire.perimeterDs);
                fire.perimeterDs = null;
            }
        })
    });
    ui.clearListItemCheckBoxes(perimeterView);
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
