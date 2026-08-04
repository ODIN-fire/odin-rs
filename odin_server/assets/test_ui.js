import * as util from "../odin_server/ui_util.js";
import * as data from "../odin_server/ui_data.js";
import * as ui from "../odin_server/ui.js";

class Entry {
    constructor(date, lat, lon) {
        this.date = date;
        this.lat = lat;
        this.lon = lon;
    }
}

let textEntry = undefined;
let list1View = undefined;
let choice = undefined;

createIcon();
createWindow();

initList1View();

initData();

function createIcon() {
  return ui.Icon("./asset/odin_server/img.svg", (e)=> ui.toggleWindow(e,'test'), "UI Test");
}

function createWindow() {
  return ui.Window("UI Test", "test", "./asset/odin_server/img.svg")(
      ui.Panel("panel 1", true)(
        (textEntry = ui.TextInput("area","orbital.bounds", "20rem", { placeHolder: "enter lat,lon bounds", changeAction: setAreaFromInput, isFixed: true })),
        (list1View = ui.List("list1", 8, selectEntry)),
        (choice = ui.Choice("chose", "test.cb"))
    )
  )
}

function initList1View() {
  ui.setListItemDisplayColumns(list1View, ["fit", "header"], [
      { name: "lat", tip: "latitude", width: "6em", attrs: ["fixed", "alignRight"], map: e => e.lat, edit: editLat },
      { name: "lon", tip: "longitude", width: "7rem", attrs: ["fixed", "alignRight"], map: e => e.lon },
      { name: "date", width: "10rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.date) }
  ]);
}

function initData() {
    ui.setChoiceItems(choice, ["WindNinja", "Wx 10m", "Wx 80m"], 0);

    let list = [
        new Entry(Date.now(), 37.12345, -121.45678)
    ];
    ui.setListItems(list1View, list);

    ui.setField(textEntry, "-121.9, 36.9, -121.6, 37.2");
}

function selectEntry(e) {
    console.log("selected: ", e);
}

function setAreaFromInput() {
    console.log("@@ not yet");
}

function editLat(event) {
    console.log("@@ edit lat: ", event.target.parentElement._uiItem);
}
