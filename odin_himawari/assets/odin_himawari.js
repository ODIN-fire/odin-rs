/*
 * Copyright © 2025, United States Government, as represented by the Administrator of
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

import { config } from "./odin_himawari_config.js";

import * as main from "../odin_server/main.js";
import * as util from "../odin_server/ui_util.js";
import * as data from "../odin_server/ui_data.js";
import * as ui from "../odin_server/ui.js";
import * as ws from "../odin_server/ws.js";
import * as odinCesium from "../odin_cesium/odin_cesium.js";

const MOD_PATH = "odin_himawari::service::HimawariHotspotService";

//------- data definitions

var dataSource = odinCesium.createDataSource("himawari", config.layer.show);

var dataSets = new data.CircularBuffer( config.maxDataSets);  // complete list in ascending time (latest entry is appended)
var selectedDataSet = undefined;
var selectedHotspot = undefined;

var dataSetView = undefined;
var hotspotView = undefined;
var historyView = undefined;

var followLatest = config.followLatest;
var pointSize = config.pointSize;

//------- module initialization

createIcon();
createWindow();

initDataSetView();
initHotspotView();
initHistoryView();
initSliders();

ws.addWsHandler(MOD_PATH, handleWsMessages);
odinCesium.setEntitySelectionHandler(handleHotspotSelection);
odinCesium.initLayerPanel("himawari", config, showHimawari);
console.log("odin_himawari initialized");

//------- websocket message processing

function handleWsMessages(msgType, msg) {
    switch (msgType) {
        case "hotspots":
            handleHotspotsMsg(msg);
            break;
    }
}

function handleHotspotsMsg (ds) {
    dataSets.sortIn(ds, (a, b) => a.date < b.date);
    updateDataSetView();
}

//------- UI window setup

function createIcon() {
    return ui.Icon(
        "./asset/odin_himawari/himawari-icon.svg",
        (e) => ui.toggleWindow(e, "himawari"),
        "Himawari hotspots",
    );
}

function createWindow() {
    return ui.Window(
        "Himawari Hotspots",
        "himawari",
        "./asset/odin_himawari/himawari-icon.svg",
    )(
        ui.LayerPanel("himawari", toggleShowHimawari),
        ui.Panel("data sets", true)(
            (dataSetView = ui.List("himawari.dataSets", 6, selectDataSet)),
            ui.RowContainer()(
                ui.CheckBox( "follow latest", toggleFollowLatest, "himawari.followLatest", config.followLatest),
                ui.ListControls("himawari.dataSets"),
            )
        ),
        ui.Panel("hotspots", true)(
            (hotspotView = ui.List( "himawari.hotspots", 6, selectHotspot, null, null, zoomToHotspot)),
        ),
        ui.Panel("filter", false)(
            ui.RowContainer()(
                ui.Button( "all", showAllHotspots),
                ui.ColumnContainer("start", null, "level", true)(
                    ui.CheckBox("flame 🔥", updateHotspotView, "himawari.filter.level_flaming", true, "7rem", "1rem"),
                    ui.CheckBox("smolder ♨️", updateHotspotView, "himawari.filter.level_smoldering", true, "7rem", "1rem"),
                    ui.CheckBox("cold", updateHotspotView, "himawari.filter.level_cold", true, "7rem", "1rem")
                ),
                ui.ColumnContainer("start", null, "reliability", true)(
                    ui.CheckBox("high ☆", updateHotspotView, "himawari.filter.rel_high", true, "7rem", "1rem"),
                    ui.CheckBox("normal ✓", updateHotspotView, "himawari.filter.rel_normal", true, "7rem", "1rem"),
                    ui.CheckBox("low", updateHotspotView, "himawari.filter.rel_low", true, "7rem", "1rem")
                ),
                ui.ColumnContainer("start", null, "quality", true)(
                    ui.CheckBox("norm ✓", updateHotspotView, "himawari.filter.qf_normal", true, "7rem", "1rem"),
                    ui.CheckBox("saturated ✸", updateHotspotView, "himawari.filter.qf_saturated", true, "7rem", "1rem"),
                    ui.CheckBox("low", updateHotspotView, "himawari.filter.qf_low", true, "7rem", "1rem")
                )
            )
        ),
        ui.Panel("history", true)(
            (historyView = ui.List( "himawari.history", 8, null, null, null, null)),
        ),
        ui.Panel("layer parameters", false)(
            ui.Slider("point size [pix]", "himawari.pointSize", setPointSize),
        ),
    );
}

function initDataSetView() {
    if (dataSetView) {
        ui.setListItemDisplayColumns(
            dataSetView,
            ["fit", "header"],
            [
                { name: "total", tip: "total number of hotspots", width: "3rem", attrs: ["fixed", "alignRight"], map: (e) => e.hotspots.length },
                { name: "lvl^", tip: "number of flaming hotspots", width: "3rem", attrs: ["fixed", "alignRight"], map: (e) => e.nFlaming },
                { name: "rel^", tip: "number of high reliability hotspots", width: "3rem", attrs: ["fixed", "alignRight"], map: (e) => e.nHigh },
                { name: "qf^", tip: "number of normal quality hotspots", width: "3rem", attrs: ["fixed", "alignRight"], map: (e) => e.nNormal },
                { name: "date", tip: "acquisition date", width: "8rem", attrs: ["fixed", "alignRight"], map: (e) => util.toLocalMDHMString(e.date) },
                { name: "recv", tip: "acquisition date", width: "4rem", attrs: ["fixed", "alignRight"], map: (e) => util.toLocalHMTimeString(e.received) },
            ],
        );
    }
}

function initHotspotView() {
    if (hotspotView) {
        ui.setListItemDisplayColumns(
            hotspotView,
            ["fit", "header"],
            [
                { name: "lvl", tip: "fire pixel level (flame,smold,cold)", width: "2rem", attrs: [], map: (e) => pixelLevel(e) },
                { name: "rel", tip: "reliability (high ☆,norm ✓,low)", width: "2rem", attrs: [], map: (e) => pixelReliability(e) },
                { name: "qf", tip: "quality (norm ✓,saturated ✸,low)", width: "2rem", attrs: [], map: (e) => pixelQuality(e) },
                { name: "frp", tip: "fire radiative power [MW]", width: "4rem", attrs: ["fixed", "alignRight"], map: (e) => util.f_2.format(e.frp) },
                { name: "area", tip: "area [km²]", width: "4rem", attrs: ["fixed", "alignRight"], map: (e) => util.f_2.format(e.area) },
                { name: "vlc", tip: "number of volcanoes", width: "2rem", attrs: ["fixed", "alignRight"], map: (e) => e.volcano },
                { name: "lat", width: "5rem", attrs: ["fixed", "alignRight"], map: (e) => util.f_3.format(e.position.lat) },
                { name: "lon", width: "5.5rem", attrs: ["fixed", "alignRight"], map: (e) => util.f_3.format(e.position.lon) },
            ],
        );
    }
}

function initHistoryView() {
    if (historyView) {
        ui.setListItemDisplayColumns(
            historyView,
            ["fit", "header"],
            [
                { name: "lvl", tip: "fire pixel level (flame,smold,cold)", width: "2rem", attrs: [], map: (e) => pixelLevel(e.hotspot) },
                { name: "rel", tip: "reliability (high ☆,norm ✓,low)", width: "2rem", attrs: [], map: (e) => pixelReliability(e.hotspot) },
                { name: "qf", tip: "quality (norm ✓,saturated ✸,low)", width: "2rem", attrs: [], map: (e) => pixelQuality(e.hotspot) },
                { name: "frp", tip: "fire radiative power [MW]", width: "4rem", attrs: ["fixed", "alignRight"], map: (e) => util.f_2.format(e.hotspot.frp) },
                { name: "area", tip: "area [km²]", width: "4rem", attrs: ["fixed", "alignRight"], map: (e) => util.f_2.format(e.hotspot.area) },
                { name: "date", tip: "acquisition date", width: "8rem", attrs: ["fixed", "alignRight"], map: (e) => util.toLocalMDHMString(e.date) },
            ],
        );
    }
}

function initSliders() {
    let e = ui.getSlider("himawari.pointSize");
    ui.setSliderRange(e, 0, 8, 1, util.f_0);
    ui.setSliderValue(e, config.pointSize);
}

function pixelLevel (e) {
    if (e.level == "Flaming") return '🔥';
    if (e.level == "Smoldering") return '♨️';
    return '';
}

function pixelReliability (e) {
    if (e.reliability == "High") return '☆';
    if (e.reliability == "Normal") return '✓';
    return '';
}

function pixelQuality (e) {
    if (e.qf == "Normal") return '✓';
    if (e.qf == "Saturated") return '✸';
    return '';
}

function showHimawari(cond) {
    dataSource.show = cond;
    odinCesium.requestRender();
}

function toggleShowHimawari(event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        showHimawari(ui.isCheckBoxSelected(cb));
    }
}

function toggleFollowLatest (event) {
    let cb = ui.getCheckBox(event.target);
    if (cb) {
        followLatest = ui.isCheckBoxSelected(cb);
    }
}

function updateDataSetView () {
    ui.setListItems(dataSetView, dataSets.toReversed());
    if (followLatest) {
        ui.setSelectedListItemIndex(dataSetView, 0);
    }
}

function selectDataSet (event) {
    let ds = event.detail.curSelection;
    if (ds) {
        selectedDataSet = ds;
    } else {
        selectedDataSet = null;
    }
    updateHotspotView();
}

function updateHotspotView () {
    if (selectedDataSet) {
        let hotspots = getFilteredHotspots(selectedDataSet);
        ui.setListItems(hotspotView, hotspots);
        updateHotspotEntities(hotspots);

    } else {
        ui.clearList(hotspotView);
        dataSource.entities.removeAll();
        odinCesium.requestRender();
    }
}

function selectHotspot(event) {
    selectedHotspot = event.detail.curSelection;
    updateHistory(selectedHotspot);
}

function handleHotspotSelection() {
    let sel = odinCesium.getSelectedEntity();
    if (sel && sel._hotspot) {
        let hs = sel._hotspot;
        if (selectedHotspot != hs) {
            ui.setSelectedListItem(hotspotView, hs);
        }
    }
}

function zoomToHotspot(event) {
    let lv = ui.getList(event);
    if (lv) {
        let hs = ui.getSelectedListItem(lv);
        if (hs) {
            let pos = hs.position;
            odinCesium.zoomTo(
                Cesium.Cartesian3.fromDegrees(
                    pos.lon,
                    pos.lat,
                    config.zoomHeight,
                ),
            );
            if (hs.entity) odinCesium.setSelectedEntity(hs.entity);
        }
    }
}

function getFilteredHotspots (ds) {
    let levelFlaming = ui.isCheckBoxSelected(document.getElementById("himawari.filter.level_flaming"));
    let levelSmoldering = ui.isCheckBoxSelected(document.getElementById("himawari.filter.level_smoldering"));
    let levelCold = ui.isCheckBoxSelected(document.getElementById("himawari.filter.level_cold"));

    let relHigh = ui.isCheckBoxSelected(document.getElementById("himawari.filter.rel_high"));
    let relNormal = ui.isCheckBoxSelected(document.getElementById("himawari.filter.rel_normal"));
    let relLow = ui.isCheckBoxSelected(document.getElementById("himawari.filter.rel_low"));

    let qfNormal = ui.isCheckBoxSelected(document.getElementById("himawari.filter.qf_normal"));
    let qfSaturated = ui.isCheckBoxSelected(document.getElementById("himawari.filter.qf_saturated"));
    let qfLow = ui.isCheckBoxSelected(document.getElementById("himawari.filter.qf_low"));

    let hotspots = [];
    for (let hs of selectedDataSet.hotspots) {
        let level = hs.level;
        if (level == "Flaming" && !levelFlaming) continue;
        if (level == "Smoldering" && !levelSmoldering) continue;
        if (level == "Cold" && !levelCold) continue;

        let reliability = hs.reliability;
        if (reliability == "High" && !relHigh) continue;
        if (reliability == "Normal" && !relNormal) continue;
        if (reliability == "Low" && !relLow) continue;

        let qf = hs.qf;
        if (qf == "Normal" && !qfNormal) continue;
        if (qf == "Saturated" && !qfSaturated) continue;
        if (qf == "LowConfidence" && !qfLow) continue;

        hotspots.push(hs);
    }

    return hotspots;
}

function showAllHotspots () {
    ui.setCheckBox("himawari.filter.level_flaming");
    ui.setCheckBox("himawari.filter.level_smoldering");
    ui.setCheckBox("himawari.filter.level_cold");

    ui.setCheckBox("himawari.filter.rel_high");
    ui.setCheckBox("himawari.filter.rel_normal");
    ui.setCheckBox("himawari.filter.rel_low");

    ui.setCheckBox("himawari.filter.qf_normal");
    ui.setCheckBox("himawari.filter.qf_saturated");
    ui.setCheckBox("himawari.filter.qf_low");

    updateHotspotView();
}

function setPointSize(event) {
    pointSize = ui.getSliderValue(event.target);

    dataSource.entities.values.forEach((e) => {
        if (e.point) e.point.pixelSize = pointSize;
    });

    odinCesium.requestRender();
}

function updateHotspotEntities(hotspots) {
    dataSource.entities.removeAll();

    hotspots.forEach((hs) => {
        let e = createHotspotEntity(hs);
        dataSource.entities.add(e);
    });

    odinCesium.requestRender();
}

function createHotspotEntity(hs) {
    let clr = color(hs);

    let e = new Cesium.Entity({
        position: Cesium.Cartesian3.fromDegrees(
            hs.position.lon,
            hs.position.lat,
            hs.position.alt,
        ),
        point: {
            pixelSize: pointSize,
            color: clr,
            outlineColor: outlineColor(hs),
            outlineWidth: outlineWidth(hs),
            distanceDisplayCondition: config.pointDC,
            disableDepthTestDistance: 6378000, // earth radius (don't show on other side of the globe)
        },
        ellipse: {
            semiMinorAxis: 1000,
            semiMajorAxis: 1000,
            material: fillMaterial(hs),
            height: 0,
            heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
            distanceDisplayCondition: config.boundsDC
        }
    });

    e._hotspot = hs; // store so that we can handle pick selection
    hs.entity = e; // watch out - backlink that could cause memory leak
    return e;
}

function color(hs) {
    switch (hs.level) {
        case "Flaming": return config.flamingColor;
        case "Smoldering": return config.smolderingColor;
        default: return config.coldColor;
    }
}

function outlineColor(hs) {
    switch (hs.reliability) {
        case "High": return config.hiReliableColor;
        case "Normal": return config.normReliableColor;
        default: return config.lowReliableColor;
    }
}

function outlineWidth(hs) {
    if (hs.qf == "High") return config.strongOutlineWidth;
    else return config.outlineWidth;
}

function fillMaterial(hs) {
    switch (hs.level) {
        case "Flaming": return config.flamingMaterial;
        case "Smoldering": return config.smolderingMaterial;
        default: return config.coldMaterial;
    }
}

function updateHistory(hs) {
    if (hs) {
        let hotspotHistory = [];
        for (let i = 0; i < dataSets.length; i++) {
            let ds = dataSets.reverseAt(i);
            if (ds) {
                let h = ds.hotspots.find((h) => { return (h.position.lon == hs.position.lon) && (h.position.lat == hs.position.lat) });
                if (h) {
                    hotspotHistory.push({date: ds.date, hotspot: h});
                }
            }
        }
        ui.setListItems(historyView, hotspotHistory);

    } else {
        ui.clearList(historyView);
    }
}
