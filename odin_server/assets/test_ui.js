import * as util from "../odin_server/ui_util.js";
import * as data from "../odin_server/ui_data.js";
import * as ui from "../odin_server/ui.js";
import * as code from "../odin_server/ui_code_input.js";

const NML_TEXT=
`! this is a sample namelist
&PHYSICS
    USE_RADIATION = .TRUE.
    TIMESTEP      = 0.025D0
    SCHEME_NAME   = 'RRTMG'
    LEVELS(1:3)   = 10, 20, 30
    CO2_PPM       = 415.0e0     ! background CO2 in ppm
/`;

const JSON_TEXT=
`{
  "name": "code-input.js demo",
  "version": 2,
  "stable": true,
  "deprecated": false,
  "homepage": null,
  "tags": ["editor", "json", "no-dependencies"],
  "stats": {
    "stars": 481,
    "rating": 4.8,
    "downloads": 1.2e5
  }
}`;

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
let codeArea = undefined;

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
        (choice = ui.Choice("chose", "test.cb")),
        (codeArea = code.CodeInput(null, "nml-highlight", "fortran-namelist", "30rem", "10lh"))
        //(codeArea = code.CodeInput(null, "json-highlight", "JSON", "30rem", "10lh"))
    )
  )
}

function initList1View() {
  ui.setListItemDisplayColumns(list1View, ["fit", "header"], [
      { name: "lat", tip: "latitude", width: "6em", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.lat, 4), edit: editLat },
      { name: "lon", tip: "longitude", width: "7rem", attrs: ["fixed", "alignRight"], map: e => util.formatFloat(e.lon, 4) },
      { name: "date", width: "10rem", attrs: ["fixed", "alignRight"], map: e => util.toLocalMDHMSString(e.date), edit: editDate }
  ]);
}

function initData() {
    ui.setChoiceItems(choice, ["WindNinja", "Wx 10m", "Wx 80m"], 0);

    let list = [
        new Entry(Date.now(), 37.12345, -121.45678),
        new Entry(Date.now() + 3600000, 38.4321, -122.1112)
    ];
    ui.setListItems(list1View, list);

    ui.setField(textEntry, "-121.9, 36.9, -121.6, 37.2");

    //code.setCode(codeInput, NML_TEXT);
    //code.setCode(codeArea, JSON_TEXT);
    codeArea.value = NML_TEXT;
}

function selectEntry(e) {
    console.log("selected: ", e);
}

function setAreaFromInput() {
    console.log("@@ not yet");
}

function editLat(event) {
    ui.editListItemField(event.target, (item, newValue) => {
        let newLat = util.parseLatitude(newValue);
        if (!isNaN(newLat)) {
            item.lat = newLat;
        } else {
            ui.beep();
            alert("invalid latitude input - enter float value");
        }
    });
}

function editDate(event) {
    ui.editListItemField(event.target, (item, newValue) => {
        let newDate = util.parseDtg(newValue);
        if (!isNaN(newDate)) {
            item.date = newDate;
        } else {
            ui.beep();
            alert("invalid date input - enter 'mm/dd HH:MM:SS");
        }
    });
}
