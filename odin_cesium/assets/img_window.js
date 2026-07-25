/*
 * Copyright 2026 Peter Mehlitz
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://apache.org
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

import * as util from "../odin_server/ui_util.js";
import { ExpandableTreeNode } from "../odin_server/ui_data.js";
import * as ui from "../odin_server/ui.js";
import * as odinCesium from "./odin_cesium.js";

/*
 * a reusable class for configured Cesium ImageryProvider collections
 * we factor this out although it is used for the globe imagery in the standard odin_cesium::ImgLayerService so that
 * we can have specialized imagery modules for data (e.g. various Landfire data sets) without having to replicate this code
 */
export class ImgWindow {

    constructor(id, sources, defaultRender, iconPath, windowTitle) {
        this.id = id;
        this.iconPath = iconPath;
        this.windowTitle = windowTitle;

        this.sources = sources;
        this.defaultRender = defaultRender;

        //--- UI state
        this.window = undefined;
        this.sourceView = undefined;
        this.colorMapView = undefined;

        this.selectedSrc = undefined;
        this.srcTree = ExpandableTreeNode.fromPreOrdered( sources, e => e.pathName);
    }

    registerImagerySources(modName) {
        this.sources.forEach( (src) => {
            if (!src.show) src.show = false;
            src.render = { ...this.defaultRender, ...src.render }; // fill up missing properties with default values
            return odinCesium.registerImagerySource(modName, src);
        });
    }

    loadColorMaps() {
        let promises = [];
        this.sources.forEach((src) => {
            if (!src.colorMap && src.colorMapUrl) {
                let promise = loadColorMap(src);
                if (promise) promises.push( promise);
            }
        });
        return promises;
    }

    elementId(suffix) {
        return (this.id + '.' + suffix);
    }

    createIcon() {
        return ui.Icon(this.iconPath, (e)=> ui.toggleWindow(e,this.id), this.windowTitle.toLowerCase());
    }

    createWindow() {
        let id = this.id;
        let render = this.defaultRender;

        this.window = ui.Window(this.windowTitle, id, this.iconPath)(
            ui.LayerPanel(id, toggleShowImgLayer),
            ui.Panel("sources", true)(
                (this.sourceView = ui.TreeList(this.elementId("source.list"), 15, "25rem", this.selectImgLayerSrc.bind(this))),
                //ui.VarText( " ", "imglayer.source.info", "25rem")
                ui.RowContainer()(
                    ui.Button("clear", this.clearAll.bind(this))
                )
            ),
            ui.Panel("color map", false)(
                (this.colorMapView = ui.List(this.elementId("cm.list"), 15, this.selectImgCmapEntry.bind(this))),
                ui.VarText(null, this.elementId("cm.info"), "25rem")
            ),
            ui.Panel("layer parameters", false)(
                ui.ColumnContainer("align_right")(
                    this.initSlider("alpha",      0, 1.0, 0.1, util.f_1, render.alpha),
                    this.initSlider("brightness", 0, 2.0, 0.1, util.f_1, render.brightness),
                    this.initSlider("contrast",   0, 2.0, 0.1, util.f_1, render.contrast),
                    this.initSlider("hue",        0, 360, 1,   util.f_0, render.hue),
                    this.initSlider("saturation", 0, 2.0, 0.1, util.f_1, render.saturation),
                    this.initSlider("gamma",      0, 2.0, 0.1, util.f_1, render.gamma)
                )
            )
        );

        return this.window;
    }

    initViews() {
        this.initSourceView();
        this.initColorMapView();

        ui.setTree(this.sourceView, this.srcTree);

        odinCesium.registerMouseClickHandler( this.handleMouseClick.bind(this));
    }

    initLayerPanel(config) {
        odinCesium.initLayerPanel( this.id, config, this.showImgLayer.bind(this));
    }

    initSourceView() {
        let view = this.sourceView;
        if (view) {
            ui.setListItemDisplayColumns(view, ["fit", "header"], [
                { name: "exclusive", tip: "only one of each group visible", width: "10rem", attrs: ["alignRight", "small"], map: e=> exclusivesOf(e)},
                ui.listItemSpacerColumn(),
                { name: "show", tip: "toggle visibility", width: "2.1rem", attrs: [], map: e => ui.createCheckBox(e.show, this.toggleShowSource.bind(this)) }
            ]);
        }
        return view;
    }

    initColorMapView() {
        let view = this.colorMapView;
        if (view) {
            ui.setListItemDisplayColumns(view, ["fit", "header"], [
                { name: "color", width: "2rem", attrs: [], map: e=> ui.createColorBox(e.color) },
                { name: "code", width: "3rem", attrs: ["alignRight"], map: e => e.code },
                ui.listItemSpacerColumn(),
                { name: "description", tip: "alt+click on map to look-up", width: "17rem", attrs: [], map: e => e.descr }
            ]);
        }
        return view;
    }

    initSlider(name, min, max, step, fmt, value) {
        let changeAction = (event) => sliderValueChanged(event, this.selectedSrc, name);
        let attrs = { min, max, step, fmt, value };
        return ui.Slider(name, this.elementId('render.'+name), changeAction, attrs);
    }

    setImgSliderValues (src) {
        let e = ui.getSlider( this.elementId('render.alpha'));
        ui.setSliderValue(e, src.layer.alpha);

        e = ui.getSlider(this.elementId('render.brightness'));
        ui.setSliderValue(e, src.layer.brightness);

        e = ui.getSlider(this.elementId('render.contrast'));
        ui.setSliderValue(e, src.layer.contrast);

        e = ui.getSlider(this.elementId('render.hue'));
        ui.setSliderValue(e, src.layer.hue * 180.0 / Math.PI);

        e = ui.getSlider(this.elementId('render.saturation'));
        ui.setSliderValue(e, src.layer.saturation);

        e = ui.getSlider(this.elementId('render.gamma'));
        ui.setSliderValue(e, src.layer.gamma);
    }

    selectImgLayerSrc() {
        let cmView = this.colorMapView;
        let src = ui.getSelectedListItem( this.sourceView);
        if (src) {
            this.selectedSrc = src;
            //ui.setTextContent("imglayer.source.info", src.info);
            this.setImgSliderValues(src);
            if (src.colorMap) ui.setListItems( cmView, src.colorMap); else ui.clearList( cmView);
        } else {
            //ui.clearTextContent("imglayer.source.info");
            ui.clearList(cmView);
        }
    }

    selectImgCmapEntry() {
        //let ce = ui.getSelectedListItem(this.colorMapView);
        //ui.setTextContent("imglayer.cm.info", ce ? ce.descr : null);
    }

    handleMouseClick(event) {
        if (event.altKey && ui.isTopWindow(this.window)) { // don't do anything if this is not the top window
            let mouseX = event.clientX;
            let mouseY = event.clientY;

            odinCesium.withSceneColor(mouseX, mouseY, (clr) => {
                let selectedSrc = this.selectedSrc;
                if (selectedSrc && selectedSrc.colorMap) {
                    let cm = selectedSrc.colorMap;
                    let diffClr = new Cesium.Color();
                    let minDiff = Number.MAX_VALUE;
                    let bestMatch = -1;

                    for (var i=0; i<cm.length; i++) {
                        Cesium.Color.subtract(cm[i].clr,clr,diffClr);
                        Cesium.Color.multiply(diffClr,diffClr,diffClr);
                        let diff = diffClr.red + diffClr.green + diffClr.blue;
                        if (diff < minDiff) {
                            minDiff = diff;
                            bestMatch = i;
                        }
                    }

                    if (bestMatch >= 0) {
                        ui.setSelectedListItemIndex( this.colorMapView, bestMatch);
                    }
                }
            });
        }
    }

    toggleShowSource(event) {
        let cb = ui.getCheckBox(event.target);
        if (cb) {
            let src = ui.getListItemOfElement(cb);
            if (src) {
                let isSelected = ui.isCheckBoxSelected(cb);
                if (src.show != isSelected) {
                    let layer = src.layer;
                    if (isSelected) {
                        this.hideExclusives(src);
                        layer.show = src.show = true;
                    } else {
                        layer.show = src.show = false;
                    }
                }

                odinCesium.requestRender();
            }
        }
    }

    hideExclusives (src) {
        let exclusive = src.exclusive;
        if (exclusive && exclusive.length > 0) {
            this.sources.forEach( (s)=> {
                if ((s !== src) && s.exclusive && util.haveEqualElements(exclusive, s.exclusive)) {
                    if (s.show) {
                        s.show = false;
                        s.layer.show = false;
                        ui.updateListItem( this.sourceView,s);
                    }
                }
            });
        }
    }

    clearAll (event) {
        for (let s of this.sources) {
            if (s.show) {
                if (!s.exclusive || !s.exclusive.includes("globe")) { // don't hide the globe
                    s.show = false;
                    s.layer.show = false;
                    ui.updateListItem( this.sourceView,s);
                }
            }
        }
    }

    showImgLayer (cond) {

    }
}

//--- utility functions

function sliderValueChanged(event, selectedSrc, propertyName) {
    if (selectedSrc) {
        let e = event.target;
        let v = ui.getSliderValue(e);
        selectedSrc.layer[propertyName] = v;
        odinCesium.requestRender();
    }
}

function exclusivesOf(e) {
    if (e.exclusive && e.exclusive.length > 0) {
        return util.mkString(e.exclusive,',');
    } else {
        return "";
    }
}


function loadColorMap (src) {
    if (src.colorMapUrl) {
        return new Promise(function(resolve) {
            var request = new XMLHttpRequest();
            request.open('GET', src.colorMapUrl);
            //request.responseType = "json";
            request.responseType = "text";

            request.onreadystatechange = function() {
                if (request.readyState === 4) {
                    if (request.response) {
                        let cm = JSON.parse(request.response);
                        src.colorMap = cm;
                        console.log("loaded colorMap for imagery layer", src.colorMapUrl);

                        for (var i=0; i<cm.length; i++) {
                            let ce = cm[i];
                            ce.clr = Cesium.Color.fromCssColorString(ce.color);
                        }
                    }
                }
            };
            request.send();
        });
    }
}

function toggleShowImgLayer(event) {
    console.log("not yet")
}
