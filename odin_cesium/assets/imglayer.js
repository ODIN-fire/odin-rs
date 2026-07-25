import { ImgWindow } from "./img_window.js";
import { config } from "./imglayer_config.js";

let window = new ImgWindow("globe", config.sources, config.render, "./asset/odin_cesium/globe.svg", "Globe Imagery");

window.createIcon();
window.createWindow();
window.initViews();
window.initLayerPanel(config);

window.registerImagerySources("globe");

console.log("globe imagery initialized");
