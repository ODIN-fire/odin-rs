import { ImgWindow } from "../odin_cesium/img_window.js";
import { config } from "./odin_landfire_config.js";

let window = new ImgWindow("landfire", config.sources, config.render, "./asset/odin_landfire/landfire.svg", "Landfire");

window.createIcon();
window.createWindow();
window.initViews();
window.initLayerPanel(config);

window.registerImagerySources("odin_landfire");
window.loadColorMaps();

console.log("odin_landfire initialized");
