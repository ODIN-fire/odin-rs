/**
 * odin_fems.js – ODIN browser data layer for FEMS stations
 */

import { config } from './odin_fems_config.js';

import * as util from "../odin_server/ui_util.js";
import * as data from "../odin_server/ui_data.js";
import * as ui from "../odin_server/ui.js";
import * as ws from "../odin_server/ws.js";
import * as odinCesium from "../odin_cesium/odin_cesium.js";

const MOD_PATH = "odin_fems::service::FemsService";

//------------------------------- types

class StationEntry {
    constructor(meta, obs) {
        this.id = meta.id;
        this.meta = meta;
        this.obs = obs;

        let wObs = this.currentWeatherObs();
        this.wObs = wObs;
        this.pos = Cesium.Cartesian3.fromDegrees(wObs.position.lon, wObs.position.lat, wObs.position.alt);
        this.clr = Cesium.Color.fromCssColorString(wObs.color);

        this.symEntity = this.createSymbolEntity();
        this.windEntity = this.creatwWindEntity();
        this.infoEntity = this.createInfoEntity();
    }

    stationId() {
        return this.id > 99999 ? ".." + (this.id % 1000) : this.id.toString();
    }

    currentWeatherObs() {
        return this.obs.weather_obs[0];
    }

    updateObs(newObs) {
        this.obs = newObs;

        let wObs = this.currentWeatherObs();
        this.wObs = wObs;
        this.pos = Cesium.Cartesian3.fromDegrees(wObs.position.lon, wObs.position.lat, wObs.position.alt);
        this.clr = Cesium.Color.fromCssColorString(wObs.color);
        //... update entities here
    }

    createSymbolEntity () {
        let src = "./asset/odin_fems/station-sym.png";

        let entity = new Cesium.Entity({
            id: this.id,
            position: this.pos,
            billboard: {
                image: src,
                distanceDisplayCondition: config.billboardDC,
                color: config.color,
                //heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
                disableDepthTestDistance: Number.MAX_SAFE_INTEGER,  // otherwise symbol might get clipped by terrain
            },
            label: {
                text: this.stationId(),
                scale: 0.8,
                horizontalOrigin: Cesium.HorizontalOrigin.LEFT,
                verticalOrigin: Cesium.VerticalOrigin.TOP,
                font: config.labelFont,
                fillColor: config.color,
                showBackground: false,
                backgroundColor: config.labelBackground,
                pixelOffset: config.labelOffset,
                distanceDisplayCondition: config.labelDC,
                //heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
                disableDepthTestDistance: Number.POSITIVE_INFINITY
            },
            point: {
                pixelSize: config.pointSize,
                color: config.color,
                outlineColor: config.pointOutlineColor,
                outlineWidth: config.pointOutlineWidth,
                distanceDisplayCondition: config.pointDC,
                //heightReference: Cesium.HeightReference.CLAMP_TO_GROUND
            }
        });
        entity._uiFemsStation = this; // backlink for selection

        dataSource.entities.add(entity);
        return entity;
    }

    creatwWindEntity() {
        let wObs = this.currentWeatherObs();
        let src = this.getWindSymbolSrc( wObs.wndSpd);
        if (src) {
            let entity = new Cesium.Entity({
                id: this.id + "-wind",
                position: this.pos,
                billboard: {
                    image: src,
                    distanceDisplayCondition: config.windDC,
                    color: config.color,
                    heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
                    alignedAxis: Cesium.Cartesian3.UNIT_Z,
                    rotation: util.toRadians( 360 - wObs.wndDir)
                }
            });

            dataSource.entities.add(entity);
            return entity;
        }
    }

    getWindSymbolSrc (spd) {
        let s = 0;
        if (spd < 2.5) return null;
        else if (spd < 7.5)  s = 5;
        else if (spd < 12.5) s = 10;
        else if (spd < 17.5) s = 15;
        else if (spd < 22.5) s = 20;
        else if (spd < 27.5) s = 25;
        else if (spd < 32.5) s = 30;
        else if (spd < 37.5) s = 35;
        else s = 50;

        return `./asset/odin_fems/wind-${s}.png`;
    }

    createInfoEntity () {
        let infoText = this.deviceInfoText();
        if (infoText) {
            let entity = new Cesium.Entity({
                id: this.id + "-info",
                position: this.pos,
                label: {
                    text: infoText,
                    font: config.infoFont,
                    scale: 0.8,
                    horizontalOrigin: Cesium.HorizontalOrigin.LEFT,
                    verticalOrigin: Cesium.VerticalOrigin.TOP,
                    fillColor: config.color,
                    //showBackground: true,
                    //backgroundColor: config.labelBackground, // alpha does not work against model
                    outlineColor: config.color,
                    outlineWidth: 1,
                    pixelOffset: config.infoOffset,
                    distanceDisplayCondition: config.infoDC,
                    heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
                    disableDepthTestDistance: Number.POSITIVE_INFINITY,
                }
            });

            dataSource.entities.add(entity);
            return entity;

        } else {
            return null;
        }
    }

    deviceInfoText () {
        let data = this.currentWeatherObs();
        return data ? `${data.temp} F\n${data.rh} %\n${data.wndDir} °\n${data.wndSpd} mph` : null;
    }
}

//--------------------------- initialization

var stationEntries = new Map();

var stationView = undefined;
var fcWeatherView = undefined;
var fcNfdrsView = undefined;
var fcFuelView = [undefined,undefined,undefined,undefined,undefined]; // V,W,X,Y,Z fuel models

var selectedStation = undefined;

createIcon();
createWindow();

var dataSource = odinCesium.createDataSource("fems", config.layer.show);

initStationView();
initFcWeatherView();
initFcNfdrsView();
for (let i = 0; i < 5; i++) initFcFuelView( fcFuelView[i], i);

ws.addWsHandler( MOD_PATH, handleWsMessages);
odinCesium.setEntitySelectionHandler( stationSelection);
odinCesium.initLayerPanel("fems", config, showFems);
console.log("fems initialized");

//-------------------- websocket message handling (data update)

export function handleWsMessages(msgType, msg) {
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

function handleSnapshotMessage(msg) {
    for (let station of msg.stations) {
        let id = station.meta.id;
        let e = new StationEntry(station.meta, station.obs);
        stationEntries.set(id, e);
    }

    let stationList = [...stationEntries.values()].sort((a, b) => {
        if (a.meta.name < b.meta.name) return -1;
        else if (a.meta.name > b.meta.name) return 1;
        else return 0;
    });
    ui.setListItems(stationView, stationList);
}

function handleUpdateMessage(obs) {
    let id = obs.id;
    let e = stationEntries.get( id);
    if (e) {
        e.updateObs(obs);
        updateObsViews(e);
    }
}

//------------------ UI

function createIcon() {
    return ui.Icon("./asset/odin_fems/fems.svg", (e)=> ui.toggleWindow(e,'fems'), "FEMS stations");
}

function createWindow() {
    let maxFcRows = 7;

    return ui.Window("FEMS stations", "fems", "./asset/odin_fems/fems.svg")(
        ui.LayerPanel("fems", toggleShowFems),
        ui.Panel("stations", true)(
            stationView = ui.List("fems.stations", 8, selectStationEntry, null, null, zoomToStation)
        ),
        ui.Panel("weather", false)(
            fcWeatherView = ui.List("fems.fc.weather", maxFcRows),
        ),
        ui.Panel("nfdrs", false)(
            fcNfdrsView = ui.List("fems.fc.nfdrs", maxFcRows)
        ),
        ui.Panel("fuel models", false)(
            ui.TabbedContainer()(
                ui.Tab("fuel model V",false)( fcFuelView[0] = ui.List("fems.fc.fuel.v", maxFcRows)),
                ui.Tab("fuel model W",false)( fcFuelView[1] = ui.List("fems.fc.fuel.w", maxFcRows)),
                ui.Tab("fuel model X",false)( fcFuelView[2] = ui.List("fems.fc.fuel.x", maxFcRows)),
                ui.Tab("fuel model Y",true )( fcFuelView[3] = ui.List("fems.fc.fuel.y", maxFcRows)),
                ui.Tab("fuel model Z",false)( fcFuelView[4] = ui.List("fems.fc.fuel.z", maxFcRows))
            )
        )
    );
}

function initStationView() {
    ui.setListItemDisplayColumns(stationView, ["fit", "header"], [
        { name: "id", tip: "station id", width: "5rem", attrs: ["fixed"], map: e => e.stationId() },
        { name: "name", tip: "station name", width: "10rem", attrs: [], map: e => util.maxString(e.meta.name, 15) },

        ui.listItemSpacerColumn(0.5),
        { name: "temp", tip: "temperature [F]", width: "3rem", attrs: ["fixed", "alignRight"], map: e => e.wObs.temp },
        { name: "hum", tip: "humidity [%]", width: "2.5rem", attrs: ["fixed", "alignRight"], map: e => e.wObs.rh },
        { name: "dir", tip: "wind direction [°]", width: "2.5rem", attrs: ["fixed", "alignRight"], map: e => util.f_0.format(e.wObs.wndDir) },
        { name: "spd", tip: "wind speed [mph]", width: "2rem", attrs: ["fixed", "alignRight"], map: e => util.f_0.format(e.wObs.wndSpd) },

        ui.listItemSpacerColumn(0.5),
        { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.wObs.date) }
    ]);
}

function initFcWeatherView() {
    ui.setListItemDisplayColumns( fcWeatherView, ["fit", "header"], [
        { name: "clr", tip: "color code", width: "2rem", attrs: [], map: e => ui.createColorBox(e.color) },

        { name: "temp", tip: "temperature [F]", width: "3rem", attrs: ["fixed", "alignRight"], map: e => e.temp },
        { name: "hum", tip: "humidity [%]", width: "2.5rem", attrs: ["fixed", "alignRight"], map: e => e.rh },

        ui.listItemSpacerColumn(0.8),
        { name: "prcp", tip: "hourly precipitation [inch]", width: "3rem", attrs: ["fixed", "alignRight"], map: e => util.formatNullable(e.hPrecip, util.f_2) },
        { name: "sr", tip: "solar radiation [W/m²]", width: "3rem", attrs: ["fixed", "alignRight"], map: e => util.formatNullable(e.sr, util.f_0) },

        ui.listItemSpacerColumn(0.8),
        { name: "wd", tip: "wind direction [°]", width: "2.5rem", attrs: ["fixed", "alignRight"], map: e => util.f_0.format(e.wndDir) },
        { name: "ws", tip: "wind speed [mph]", width: "2rem", attrs: ["fixed", "alignRight"], map: e => util.f_0.format(e.wndSpd) },

        ui.listItemSpacerColumn(0.5),
        { name: "gd", tip: "gust direction [°]", width: "2.5rem", attrs: ["fixed", "alignRight"], map: e => util.formatNullable(e.gstDir, util.f_0) },
        { name: "gs", tip: "gust speed [mph]", width: "2rem", attrs: ["fixed", "alignRight"], map: e => util.formatNullable(e.gstSpd, util.f_0) },

        ui.listItemSpacerColumn(0.8),
        { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.date) }
    ]);
}

function initFcNfdrsView() {
    ui.setListItemDisplayColumns( fcNfdrsView, ["fit", "header"], [
        { name: "kbdi", tip: "Keech-Bryam drought index [0-800]", width: "2.5rem", attrs: ["fixed", "alignRight"], map: e => e.kbdi },
        { name: "gsi", tip: "growth season index", width: "3.5rem", attrs: ["fixed", "alignRight"], map: e => util.f_2.format(e.gsi) },

        ui.listItemSpacerColumn(0.5),
        { name: "1h", tip: "dead fuel moisture 1h [%]", width: "3rem", attrs: ["fixed", "alignRight"], map: e => e.dfm1h },
        { name: "10h", tip: "dead fuel moisture 10h [%]", width: "3rem", attrs: ["fixed", "alignRight"], map: e => e.dfm10h },
        { name: "100h", tip: "dead fuel moisture 100h [%]", width: "3rem", attrs: ["fixed", "alignRight"], map: e => e.dfm100h },

        ui.listItemSpacerColumn(0.5),
        { name: "herb", tip: "herbacious live fuel moisture [%]", width: "3rem", attrs: ["fixed", "alignRight"], map: e => e.lfmHerb },
        { name: "wood", tip: "woody live fuel moisture [%]", width: "3rem", attrs: ["fixed", "alignRight"], map: e => e.lfmWood },

        ui.listItemSpacerColumn(0.8),
        { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.date) }
    ]);
}

function initFcFuelView( view, idx) {
    ui.setListItemDisplayColumns(view, ["fit", "header"], [
        { name: "bi", tip: "burning index", width: "4rem", attrs: ["fixed", "alignRight"], map: e => util.f_2.format(e.bi[idx]) },
        ui.listItemSpacerColumn(0.8),
        { name: "ic", tip: "ignition component", width: "4rem", attrs: ["fixed", "alignRight"], map: e => util.f_2.format(e.ic[idx]) },
        { name: "sc", tip: "spread component", width: "4rem", attrs: ["fixed", "alignRight"], map: e => util.f_2.format(e.sc[idx]) },
        { name: "erc", tip: "energy release component", width: "4rem", attrs: ["fixed", "alignRight"], map: e => util.f_2.format(e.erc[idx]) },
        ui.listItemSpacerColumn(0.8),
        { name: "date", width: "9rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.date) }
    ]);
}

function showFems(cond) {
    dataSource.show = cond;
    odinCesium.requestRender();
}

function toggleShowFems(event) {
    dataSource.show = !dataSource.show;
    odinCesium.requestRender();
}

// entity -> list
function stationSelection() {
    let sel = odinCesium.getSelectedEntity();
    if (sel && sel._uiFemsStation) {
        if (!Object.is(sel._uiFemsStation, selectedStation)) {
            ui.setSelectedListItem(stationView, sel._uiFemsStation);
        }
    }
}

function selectStationEntry(event) {
    let e = event.detail.curSelection;
    if (e) {
        selectedStation = e;
        updateObsViews(e);

    } else {
        ui.clearList(fcWeatherView);
        ui.clearList(fcNfdrsView);
        for (let i = 0; i < 5; i++) ui.clearList(fcFuelView[i]);
    }
}

function updateObsViews(e) {
    if (e == selectedStation) {
        ui.setListItems(fcWeatherView, e.obs.weather_obs);
        ui.setListItems(fcNfdrsView, e.obs.nfdrs_obs);
        for (let i = 0; i < 5; i++) ui.setListItems(fcFuelView[i], e.obs.nfdrs_obs);
    }
}

function zoomToStation() {
    if (selectedStation) {
        let wObs = selectedStation.wObs;
        if (wObs) {
            let pos = Cesium.Cartesian3.fromDegrees(wObs.position.lon, wObs.position.lat, config.zoomHeight);
            odinCesium.zoomTo(pos);
            odinCesium.setSelectedEntity(selectedStation.symbol);
        }
    }
}
