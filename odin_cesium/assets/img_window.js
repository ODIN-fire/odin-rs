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
        this.infoTxt = undefined;
        this.colorMapView = undefined;
        this.cmapInfoTxt = undefined;
        this.pickBtn = undefined;
        this.isPicking = false;    // next globe click looks up the color map entry of the selected source ("pick" button)

        this.selectedSrc = undefined;
        this.srcTree = ExpandableTreeNode.fromPreOrdered( sources, e => e.pathName);
    }

    // maxCameraHeight [m] is an optional default for sources that should not be shown if the camera is above that height
    // (e.g. for high resolution data that would be expensive to display for large areas). Cesium does not support
    // distance display conditions for imagery layers so we have to do this with a camera listener
    registerImagerySources(modName, maxCameraHeight=undefined) {
        this.sources.forEach( (src) => {
            if (!src.show) src.show = false;
            if (src.maxCameraHeight === undefined) src.maxCameraHeight = maxCameraHeight;
            src.render = { ...this.defaultRender, ...src.render }; // fill up missing properties with default values
            return odinCesium.registerImagerySource(modName, src);
        });

        if (this.sources.some( src => src.maxCameraHeight)) {
            odinCesium.viewerReadyPromise.then( (viewer) => {
                let update = () => this.updateCameraHeightVisibility();
                viewer.camera.changed.addEventListener( update);
                viewer.camera.moveEnd.addEventListener( update);
            });
        }
    }

    isWithinCameraHeight(src) {
        return !src.maxCameraHeight || (odinCesium.viewer.camera.positionCartographic.height <= src.maxCameraHeight);
    }

    // the only place where we set the visibility of imagery layers that are shown
    updateLayerVisibility(src) {
        if (src.layer) {
            let show = src.show && this.isWithinCameraHeight(src);
            if (src.layer.show != show) {
                src.layer.show = show;
                odinCesium.requestRender();
            }
        }
    }

    updateCameraHeightVisibility() {
        for (const src of this.sources) {
            if (src.maxCameraHeight && src.show) this.updateLayerVisibility(src);
        }
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
            ui.LayerPanel(id),
            ui.Panel("sources", true)(
                (this.sourceView = ui.TreeList(this.elementId("source.list"), 15, "27rem", this.selectImgLayerSrc.bind(this))),
                ui.RowContainer()(
                    (this.infoTxt = ui.VarText( " ", "source.info", "23rem")),
                    ui.Button("clear", this.clearAll.bind(this))
                )
            ),
            ui.Panel("color map", false)(
                (this.colorMapView = ui.List(this.elementId("cm.list"), 15, this.selectImgCmapEntry.bind(this))),
                ui.RowContainer()(
                    (this.cmapInfoTxt = ui.VarText(null, this.elementId("cm.info"), "25rem")),
                    (this.pickBtn = ui.Button("pick", this.togglePickMode.bind(this)))
                )
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
        odinCesium.registerKeyDownHandler( this.handleKeyDown.bind(this));
    }

    initLayerPanel(config) {
        odinCesium.initLayerPanel( this.id, config, this.showImgLayer.bind(this));
    }

    initSourceView() {
        let view = this.sourceView;
        if (view) {
            ui.setListItemDisplayColumns(view, ["fit", "header"], [
                { name: "exclusive", tip: "only one of each group visible", width: "11rem", attrs: ["alignRight", "small"], map: e=> exclusivesOf(e)},
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
                { name: "description", tip: "pick button or shift+click on map to look-up", width: "20rem", attrs: ["small"], map: e => e.descr }
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
            ui.setVarText( this.infoTxt, src.info);
            this.setImgSliderValues(src);
            if (src.colorMap) ui.setListItems( cmView, src.colorMap); else ui.clearList( cmView);
        } else {
            ui.clearTextContent(this.infoTxt);
            ui.clearList(cmView);
        }
    }

    selectImgCmapEntry() {
        let ce = ui.getSelectedListItem(this.colorMapView);
        ui.setVarText(this.cmapInfoTxt, ce ? ce.descr : null);
    }

    // "pick" button: the next click on the globe looks up the color map entry at the click position (a more obvious
    // alternative to shift+click). Clicking the button again or pressing Esc cancels
    togglePickMode() {
        if (this.isPicking) {
            this.exitPickMode();
        } else if (!this.selectedSrc) {
            ui.setVarText( this.cmapInfoTxt, "select a source to look up first");
        } else {
            this.isPicking = true;
            ui.setElementColors( this.pickBtn, ui.getRootVar("--selected-data-color"), ui.getRootVar("--selection-background"));
            odinCesium.setCursor("crosshair");
            ui.setVarText( this.cmapInfoTxt, "click on globe to look up value");
        }
    }

    exitPickMode() {
        if (this.isPicking) {
            this.isPicking = false;
            ui.resetElementColors( this.pickBtn);
            odinCesium.setDefaultCursor();
        }
    }

    handleKeyDown(event) {
        if (this.isPicking && event.key === "Escape") {
            this.exitPickMode();
            ui.setVarText( this.cmapInfoTxt, null);
        }
    }

    // lookup of the data value / color map entry at the mouse position for the selected source, either after the "pick"
    // button was clicked or with shift+click. If the source provider supports feature picking (e.g. WMS GetFeatureInfo)
    // we get the value from the server, which does not depend on rendering parameters (brightness etc.), layer alpha,
    // lighting or similar colors. Otherwise we fall back to finding the color map entry that best matches the rendered
    // scene color
    handleMouseClick(event) {
        let isPick = this.isPicking;
        if (isPick || (event.shiftKey && ui.isTopWindow(this.window))) { // shift+click only if this is the top window
            this.exitPickMode(); // picks are single shot
            this.lookupAt( this.selectedSrc, event.clientX, event.clientY);
        }
    }

    lookupAt(src, x, y) {
        if (src) {
            if (src.layer && src.layer.show && src.layer.imageryProvider && src.layer.imageryProvider.enablePickFeatures) {
                this.lookupFeatureInfo( src, x, y);
            } else if (src.colorMap) {
                this.lookupSceneColor( src, x, y);
            } else {
                ui.setVarText( this.cmapInfoTxt, "no color map or feature info for this source");
            }
        }
    }

    lookupFeatureInfo(src, x, y) {
        let viewer = odinCesium.viewer;
        let ray = viewer.camera.getPickRay( new Cesium.Cartesian2(x, y));
        let promise = ray ? viewer.imageryLayers.pickImageryLayerFeatures( ray, viewer.scene) : undefined;

        if (promise) {
            promise.then( (features) => {
                // features of other pickable layers at this position are ignored
                let fi = features ? features.find( f => f.imageryLayer === src.layer) : undefined;
                this.showLookupResult( src, (fi && fi.data) ? fi.data.properties : undefined);
            }).catch( (err) => {
                console.log("feature info lookup failed: ", err);
                this.showLookupResult( src, undefined);
            });
        } else {
            this.showLookupResult( src, undefined);
        }
    }

    // props are the GeoJSON feature properties of the GetFeatureInfo response: { value, nodata, name }
    showLookupResult(src, props) {
        let cmView = this.colorMapView;
        let infoId = this.elementId("cm.info");

        if (props) {
            let idx = (src.colorMap && !props.nodata) ? src.colorMap.findIndex( ce => ce.code == String(props.value)) : -1;
            if (idx >= 0) ui.setSelectedListItemIndex( cmView, idx); else ui.clearSelectedListItem( cmView);

            if (props.nodata) {
                ui.setVarText( infoId, "no data");
            } else {
                let v = Number.isInteger(props.value) ? props.value : util.formatFloat( props.value, 2);
                ui.setVarText( infoId, src.colorMap ? `${v}: ${props.name}` : `${props.name} = ${v}`);
            }
        } else {
            ui.clearSelectedListItem( cmView);
            ui.setVarText( infoId, "no value at this position");
        }
    }

    lookupSceneColor(src, x, y) {
        odinCesium.withSceneColor( x, y, (clr) => {
            let cm = src.colorMap;
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
        });
    }

    toggleShowSource(event) {
        let cb = ui.getCheckBox(event.target);
        if (cb) {
            let src = ui.getListItemOfElement(cb);
            if (src) {
                let isSelected = ui.isCheckBoxSelected(cb);
                if (src.show != isSelected) {
                    if (isSelected) this.hideExclusives(src);
                    src.show = isSelected;
                    this.updateLayerVisibility(src);
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
        return new Promise(function (resolve) {
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
