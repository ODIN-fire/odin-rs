import * as util from "../odin_server/ui_util.js";
import * as data from "../odin_server/ui_data.js";
import * as ui from "../odin_server/ui.js";

class Entry {
    constructor(lat, lon) {
        this.lat = lat;
        this.lon = lon;
    }
}

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
        (list1View = ui.List("list1", 8)),
        (choice = ui.Choice("chose", "test.cb"))
    )
  )
}

function initList1View() {
  ui.setListItemDisplayColumns(list1View, ["fit", "header"], [
    { name: "lat", tip: "latitude", width: "2rem", attrs: [], map: e => e.lat },
  ]);
}

function initData() {
    ui.setChoiceItems(choice, ["WindNinja", "Wx 10m", "Wx 80m"], 0);
}
