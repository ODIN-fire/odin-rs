/*
 * Copyright © 2024, United States Government, as represented by the Administrator of 
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
import * as main from "./main.js";
import * as util from "./ui_util.js";
import { ExpandableTreeNode } from "./ui_data.js";


//--- module initialization
// (note that the moduleInitializers are executed on window.onload - after all modules and elements have been loaded and pre-initialized)
// NOTE - this does not hold in case of toplevel awaits - onload might get executed before toplevel awaits return

var loadFunctions = [];
var unloadFunctions = [];
var postLoadFunctions = [];  // called after all loadFunctions have been executed

window.addEventListener('load', e => {
    loadFunctions.forEach(f => f());
    postLoadFunctions.forEach(f => f());
});

window.addEventListener('unload', e => {
    loadFunctions.forEach(f => f());
    console.log("modules terminated.");
});

export function registerLoadFunction(func) {
    loadFunctions.push(func);
}

export function registerUnloadFunction(func) {
    unloadFunctions.push(func);
}

export function registerPostLoadFunction(func) {
    postLoadFunctions.push(func);
}

// FIXME - we don't support server-side generated elements anymore
// this has to be the first moduleInitializer so that all modules can rely on expanded elements
//registerLoadFunction(function initialize() {
//    initializeIcons();
//    initializeWindows();
 
//    _initializeMenus(); /* has to be last in case widgets add menus */
//});

var themeChangeHandlers = [];

//--- fullScreen support

var isFullScreen = false;

export function enterFullScreen() {
    if (!isFullScreen) {
        isFullScreen = true;

        var e = document.documentElement;
        if (e.requestFullscreen) {
            e.requestFullscreen();
        } else if (e.webkitRequestFullscreen) { /* Safari */
            e.webkitRequestFullscreen();
        }
    }
}

export function exitFullScreen() {
    if (isFullScreen) {
        isFullScreen = false;
        if (document.exitFullscreen) {
            document.exitFullscreen();
        } else if (document.webkitExitFullscreen) { /* Safari */
            document.webkitExitFullscreen();
        }
    }
}

export function toggleFullScreen() {
    if (isFullScreen) exitFullScreen();
    else enterFullScreen();
}

//--- theme change support

var themeChangeHandlers = [];

export function registerThemeChangeHandler(handler) {
    if (handler instanceof Function) {
        themeChangeHandlers.push(handler);
    }
}

export function notifyThemeChangeHandlers() {
    themeChangeHandlers.forEach(h => h())
}

//--- MoveableCanvas

export function MoveableCanvas (extraCls, opts) {
    let e = createElement("CANVAS", "ui_moveable_canvas");
    extraCls.forEach( cls=> e.classList.add(cls));

    if (opts.draw) e._uiDraw = opts.draw;
    setStyle(e, opts);
    makeMoveableCanvasDraggable(e);

    document.body.appendChild(e);
    return e;
} 

export function showMoveableCanvas (o) {
    let e = getMoveableCanvas(o);
    if (e)  {
        if (e._uiDraw) e._uiDraw(e);
        _addClass(e, "show");
    }
}

export function hideMoveableCanvas (o) {
    let e = getMoveableCanvas(o);
    if (e)  _removeClass(e, "show");
}

export function closeMoveableCanvas (o) {
    let e = getMoveableCanvas(o);
    if (e) {
        _removeClass(e, "show");
        e.parentElement.removeChild(e);
    }
}

function makeMoveableCanvasDraggable(e) {
    var p1 = e.offsetLeft,
        p2 = e.offsetTop,
        p3 = p1,
        p4 = p2;

    e.addEventListener("mousedown", startDragCanvas);

    function startDragCanvas(mouseEvent) {
        e.style.cursor = "move";
        p3 = mouseEvent.clientX;
        p4 = mouseEvent.clientY;

        document.onmouseup = stopDragCanvas;
        document.onmousemove = dragCanvas;
    }

    function dragCanvas(mouseEvent) {
        mouseEvent.preventDefault();

        p1 = p3 - mouseEvent.clientX;
        p2 = p4 - mouseEvent.clientY;
        p3 = mouseEvent.clientX;
        p4 = mouseEvent.clientY;

        e._uiTop = e.offsetTop - p2;
        e._uiLeft = e.offsetLeft - p1;

        e.style.top = e._uiTop + "px";
        e.style.left = e._uiLeft + "px";
    }

    function stopDragCanvas() {
        e.style.cursor = "default";
        document.onmouseup = null;
        document.onmousemove = null;
    }
}

export function updateMoveableCanvas (o) {
    let e = getMoveableCanvas(o);
    if (e && e._uiDraw) {
        e._uiDraw(e);
    }
}

export function getMoveableCanvas (o) {
    let e = _elementOf(o);
    if (e) {
        if (e.classList.contains("ui_moveable_canvas")) return e;
    }
    return null;
}

//--- windows

var topWindowZ = _rootVarInt('--window-z');
var spotlightZ = _rootVarInt('--spotlight-z');
var windows = [];

export function Window (title, eid, icon) {
    return function (...children) { 
        let e = createElement("DIV", "ui_window");
        
        e.setAttribute("data-title", title);
        if (eid) e.setAttribute("id", eid);
        if (icon) e.setAttribute("data-icon", icon);

        for (const c of children) e.appendChild(c);

        initializeWindow(e); // we have to do this ourselves here
        return e;
    }
}

// this is only called for server-created elements that are already in the DOM
function initializeWindows() {
    for (const e of _getDocumentElementsByClassName("ui_window")) initializeWindow(e);
}

export function initializeWindow(e) {
    console.log("initializing window: ", e.id);

    if (e.children.length == 0 || !e.children[0].classList.contains("ui_titlebar")) {
        createWindowComponents(e, e.dataset.title, true, e.dataset.icon); // document windows are always permanent
    }
    setWindowEventHandlers(e);

    if (e.dataset.onclose) w.closeAction = new Function(e.dataset.onclose);

    for (let c of _getChildren(e)) initializeRecursive(c);
    addWindow(e);

    return e;
}

function initializeRecursive (e) {
    switch (_getUiClass(e)) {
        case "ui_panel": initializePanel(e); break;
        case "ui_container": initializeContainer(e); break;
        case "ui_checkbox": initializeCheckBox(e); break;
        case "ui_radio":  initializeRadio(e); break;
        case "ui_choice": initializeChoice(e); break;
        case "ui_slider": initializeSlider(e); break;
        case "ui_list": initializeList(e); break;
        case "ui_field": initializeField(e); break;
        case "ui_tab_container_wrapper": initializeTabbedContainer(e); break;
        case "ui_listcontrols": initializeListControls(e); break;
        case "ui_clock": initializeClock(e); break;
        case "ui_timer": initializeTimer(e); break;
        case "ui_kvtable": initializeKvTable(e); break;
        case "ui_menuitem": initializeMenuItem(e); break;
        case "ui_progress_bar": initializeProgressBar(e); break;
        // default does not need initialization
    }

    for (const c of _getChildren(e)) initializeRecursive(c);
}

export function createWindow(title, isPermanent, closeAction, icon) {
    let w = createElement("DIV", "ui_window");
    createWindowComponents(w, title, isPermanent, icon);
    setWindowEventHandlers(w);

    if (closeAction) w.closeAction = closeAction;

    return w;
}

export function addWindow(w) {
    document.body.appendChild(w);
}

// this also moves already added child elements
function createWindowComponents(e, title, isPermanent, icon) {
    let tb = createElement("DIV", "ui_titlebar", title);

    if (icon) {
        let img = createElement("IMG", "ui_titlebar_icon");
        img.src = icon;
        tb.appendChild(img);
    }

    let cb = createElement("BUTTON", "ui_close_button", "⛌"); 
    cb.onclick = (event) => {
        let w = event.target.closest('.ui_window');
        if (w) {
            if (isPermanent) closeWindow(w);
            else removeWindow(w);
        }
    };
    cb.setAttribute("tabindex", "-1");
    tb.appendChild(cb);

    let wndContent = createElement("DIV", "ui_window_content");
    _moveChildElements(e, wndContent);

    e.appendChild(tb);
    e.appendChild(wndContent);
}

export function getWindowContent (o) {
    let w = getWindow(o);
    if (w) {
        return _nthChildOf(w,1);
    }
}

function setWindowEventHandlers(e) {
    makeWindowDraggable(e);
    e.onmousedown = function (event) { 
        raiseWindowToTop(e); 
    };
}

function makeWindowDraggable(e) {
    var p1 = e.offsetLeft,
        p2 = e.offsetTop,
        p3 = p1,
        p4 = p2;

    let titlebar = e.getElementsByClassName("ui_titlebar")[0];
    titlebar.onmousedown = startDragWindow;

    function startDragWindow(mouseEvent) {
        raiseWindowToTop(e);
        p3 = mouseEvent.clientX;
        p4 = mouseEvent.clientY;
        document.onmouseup = stopDragWindow;
        document.onmousemove = dragWindow;
    }

    function dragWindow(mouseEvent) {
        mouseEvent.preventDefault();

        p1 = p3 - mouseEvent.clientX;
        p2 = p4 - mouseEvent.clientY;
        p3 = mouseEvent.clientX;
        p4 = mouseEvent.clientY;

        e.style.top = (e.offsetTop - p2) + "px";
        e.style.left = (e.offsetLeft - p1) + "px";
    }

    function stopDragWindow() {
        document.onmouseup = null;
        document.onmousemove = null;
    }
}

// we have two categories of windows that each maintain their own z-index hierarchy: spotlight and normal
// make sure spotlights always stay on top of normal
function updateWindowZorder() {
    let z = topWindowZ;
    let zSpot = spotlightZ;

    for (let i = windows.length - 1; i >= 0; i--) {
        let w = windows[i];
        if (!_containsClass(w, "spotlight")){ 
            w.style.zIndex = z;
            z--;
        } else {
            w.style.zIndex = zSpot;
            zSpot--;
        }
    }
}

function addWindowOnTop(w) {
    windows.push(w);
    updateWindowZorder();
}

export function raiseWindowToTop(o) {
    let w = getWindow(o);
    if (w) {
        var idx = windows.indexOf(w);
        let iTop = windows.length - 1;

        if (idx >= 0 && idx < iTop) {
            for (let i = idx; i < iTop; i++) {
                windows[i] = windows[i + 1];
            }
            windows[iTop] = w;
            updateWindowZorder();
        }
    }
}

function removeWindowFromStack(w) {
    var idx = windows.indexOf(w);
    if (idx >= 0) {
        windows.splice(idx, 1);
        updateWindowZorder();
        w.style.zIndex = -1;
    }
}

export function setWindowSpotlight(o,isSpotlight) {
    let w = getWindow(o);
    if (w) {
        if (isSpotlight) {
            _addClass(w, "spotlight");
            _addClass(w.firstChild, "spotlight"); // the titlebar
        }
        else {
            _removeClass( w, "spotlight");
            _removeClass(w.firstChild, "spotlight"); // the titlebar
        }
    }
}

export function showWindow(o) {
    let e = getWindow(o);
    if (e) {
        _addClass(e, "show");
        addWindowOnTop(e);
    }
}

export function closeWindow(o) {
    let e = getWindow(o);
    if (e) {
        _removeClass(e, "show");
        removeWindowFromStack(e);
        if (e.closeAction) e.closeAction();
    }
}

export function removeWindow(o) {
    let e = getWindow(o);
    if (e) {
        _removeClass(e, "show");
        removeWindowFromStack(e);
        e.parentElement.removeChild(e);
        if (e.closeAction) e.closeAction();
    }
}

export function toggleWindow(event, o) {
    let e = _elementOf(o);
    if (e) {
        if (!_containsClass(e, "show")) {
            if (window.getComputedStyle(e).left == 'auto') { // no placement specified in CSS
                let r = event.target.getBoundingClientRect();
                setWindowLocation(e, r.left, r.bottom);
            }
            showWindow(e);
        } else {
            closeWindow(e);
        }
    }
}
main.exportFuncToMain(toggleWindow);

export function setWindowLocation(o, x, y) {
    let e = getWindow(o);
    if (e) {
        // top right should always be visible so that we can move/hide
        let w = e.offsetWidth;
        let h = e.offsetHeight;
        let sw = window.innerWidth;
        let sh = window.innerHeight;

        if ((x + w) > sw) x = sw - w;
        if ((y + h) > sh) y = sh - h;
        if (y < 0) y = 0;

        e.style.left = x + "px";
        e.style.top = y + "px";
    }
}

export function placeWindow (o, x, y) {
    let e = _elementOf(o);
    if (e) {
        let w = e.offsetWidth;
        let h = e.offsetHeight;

        let wWindow = window.innerWidth;
        let hWindow = window.innerHeight;

        if (typeof x !== 'undefined') {
            if (x + w > wWindow) { x = wWindow - w; }
        } else {
            x = (wWindow - w) / 2;
        }

        if (typeof y !== 'undefined') {
            if (y + h > hWindow) { y = hWindow - h; }
        } else {
            y = (hWindow - h) / 2;
        }
        setWindowLocation(e, x, y);
    }
}

export function setWindowSize(o, w, h) {
    let e = getWindow(o);
    if (e) {
        // if dimensions are given as numbers we assume pixel
        if (util.isNumber(w)) w = w.toString() + "px";
        if (util.isNumber(h)) h = h.toString() + "px";

        e.style.width = w;
        e.style.height = h;
    }
}

export function setWindowResizable(o, isResizable) {
    let e = getWindow(o);
    if (e) {
        if (isResizable) {
            _addClass(e, "resizable");
        } else {
            _removeClass(e, "resizable");
        }
    }
}

export function resetWindowSize (o) {
    let e = getWindow(o);
    if (e) {
        o.style.width = "";
        o.style.height = "";

        let content = _firstChildWithClass(o, "ui_window_content");
        content.style.width = "";
        content.style.height = "";
    }
}

export function addWindowContent(o, ce) {
    let e = getWindow(o);
    if (e) {
        let wc = _firstChildWithClass(e, "ui_window_content");
        if (wc) {
            wc.appendChild(ce);
        }
    }
}

export function getWindow(o) {
    let e = _elementOf(o);
    if (e) {
        return _nearestElementWithClass(e, "ui_window");
    } else {
        return undefined;
    }
}

//--- tabbed containers

export function TabbedContainer (eid,width) {
    return function (...children) {
        let e = createElement("DIV", "ui_tab_container_wrapper");
        if (eid) e.setAttribute("id", eid);
        if (width) e.style.minWidth = width;

        for (const c of children) e.appendChild(c);

        return e;
    }
}

export function Tab (label,show,eid) {
    return function (...children) {
        let e = createElement("DIV", "ui_tab_container");
        e.setAttribute("data-label", label);
        if (show) e.classList.add("show");
        if (eid) e.setAttribute("id", eid);

        for (const c of children) e.appendChild(c);

        return e;
    }
}

function initializeTabbedContainer (e) {
    if (!e._uiIsInitialized) {
        let thdr = undefined;
        let showTab = undefined;
        let showTc = undefined;
        let fit = e.classList.contains("fit");

        for (let tc of e.children) { // those are the tab_containers
            let show = tc.classList.contains("show");
            if (fit) tc.classList.add("fit");

            let tab = createElement("DIV","ui_tab");
            tab._uiContainer = tc;
            tab.innerText = tc.dataset.label;
            tc._uiTab = tab;
            tab.addEventListener("click", clickTab);

            if (show) { // last one wins
                if (showTab) showTab.classList.remove("show");
                if (showTc) showTc.classList.remove("show");
                showTab = tab;
                showTc = tc;
                tab.classList.add("show");
                e._uiShowing = tc;
            }

            if (!thdr) thdr = createElement("DIV", "ui_tab_header");
            thdr.appendChild(tab);
        }

        if (thdr) e.insertBefore(thdr, e.firstChild);
        e._uiIsInitialized = true;
    }

    return e;
}

function clickTab(event) {
    let tab = event.target;
    let tc = tab._uiContainer;
    if (tc) {
        let tcw = tc.parentElement;
        if (tcw._uiShowing !== tc) {
            let prevTc = tcw._uiShowing;
            prevTc.classList.remove("show");
            prevTc._uiTab.classList.remove("show");

            tc.classList.add("show");
            tc._uiTab.classList.add("show");
            tcw._uiShowing = tc;
        }
    }
}

export function getShowingTabName (o) {
    let e = getTabbedContainer(o);
    if (e) {
        if (e._uiShowing) {
            return e._uiShowing.getAttribute("data-label");
        }
    }
    return null;
}

export function getTabbedContainer(o) {
    let e = _elementOf(o);
    if (e) {
        return _nearestElementWithClass(e, "ui_tab_container_wrapper");
    } else {
        return undefined;
    }
}

//--- containers

function genContainer (containerCls, align, eid, title, isBordered, children, width=undefined) {
    let e = createElement("DIV", "ui_container");

    e.classList.add(containerCls);
    if (title) e.classList.add("titled");
    if (isBordered) e.classList.add("bordered");
    if (align) e.style.alignItems = align;

    if (eid) e.setAttribute("id", eid);
    if (title) e.setAttribute("data-title", title);

    if (width) e.style.width = width;

    for (const c of children) e.appendChild(c);

    return e;
}

export function RowContainer (align=null, eid=null, title=null, isBordered=false, width=undefined) {
    return function (...children) {
        return genContainer( "row", align, eid, title, isBordered, children, width);
    };
}

export function ColumnContainer (align=null, eid=null, title=null, isBordered=false) {
    return function (...children) {
        return genContainer( "column", align, eid, title, isBordered, children);
    };
}

function initializeContainer (e) {
    let pe = e.parentElement;
    if (e.dataset.title && !_containsClass(pe, "ui_container_wrapper")) {
        let title = e.dataset.title;
        let cwe = createElement("DIV","ui_container_wrapper");
        let te = createElement("DIV", "ui_container_title");
        //te.setHTML(title); // not yet supported by Firefox
        te.innerHTML = title;
        cwe.appendChild(te);
        pe.replaceChild(cwe,e);
        cwe.appendChild(e);
    }
}

//--- panels

export function Panel (title, isExpanded=false, eid=null) {
    return function (...children) {
        let e = createElement("DIV", "ui_panel");

        e.classList.add(isExpanded ? "expanded" : "collapsed");
        
        e.setAttribute("data-title", title);
        if (eid) e.setAttribute("id", eid);

        for (const c of children) e.appendChild(c);

        return e;
    }
}

function initializePanel (e) {
    let isExpanded = e.classList.contains("expanded");

    let prev = e.previousElementSibling;
    if (!prev || !prev.classList.contains("ui_panel_header")) {
        let panelTitle = e.dataset.title;
        let panelHeader = createElement("DIV", "ui_panel_header", panelTitle);
        panelHeader.classList.add( isExpanded ? "expanded" : "collapsed");
        panelHeader.addEventListener("click", togglePanelExpansion);
        if (e.id) panelHeader.id = e.id + "-header";
        e.parentElement.insertBefore(panelHeader, e);
    }

    if (!isExpanded) e.style.maxHeight = 0;
}

export function togglePanelExpansion(event) {
    const panelHeader = event.target;
    const panel = panelHeader.nextElementSibling;

    if (panelHeader.classList.contains("expanded")) { // collapse
        panel._uiCurrentHeight = panel.scrollHeight;

        if (!panel.style.maxHeight) { // we have to give max-height an initial value but without triggering a transition
            panel.style.maxHeight = panel._uiCurrentHeight + "px";
            setTimeout(() => { togglePanelExpansion(event); }, 100);
        } else {
            _swapClass(panelHeader, "expanded", "collapsed");
            _swapClass(panel, "expanded", "collapsed");
            panel.style.maxHeight = 0;
            panel.style.visibility = "none";
        }

    } else { // expand
        //let expandHeight = panel._uiCurrentHeight ? panel._uiCurrentHeight : panel.scrollHeight;
        let expandHeight = panel.scrollHeight;

        _swapClass(panelHeader, "collapsed", "expanded");
        _swapClass(panel, "collapsed", "expanded");
        panel.style.maxHeight = expandHeight + "px";
        panel.style.visibility = "visible";
    }

    // should we force a reflow on the parent here?
}

function _resetPanelMaxHeight(ce) {
    let panel = nearestParentWithClass(ce, "ui_panel");
    if (panel && !panel.classList.contains("collapsed")) {
        panel.style.maxHeight = "";
    }
}

//--- icon functions

const iconBox = getIconBox();

export function Icon (src, action, tip=null, eid=null) {
    let e = createElement( "DIV", "ui_icon");

    e.setAttribute("data-src", src);
    if (eid) e.setAttribute("id", eid);
    if (tip) createTooltip(e, tip);

    if (action instanceof Function) e.addEventListener("click", action); else e.onclick = action;

    initializeIcon(e);
    return e;
}

function initializeIcon (e) {
    if (e.children.length == 0) {
        // if icon is not positioned add it to the iconBox
        if (!iconBox) iconBox = getIconBox();
        let parent = e.parentElement;
        if (parent) parent.removeChild(e);
        iconBox.appendChild(e);

        let src = e.dataset.src;

        let svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
        e.appendChild(svg);
        svg.setAttribute("viewBox", "0 0 32 32");

        let use = document.createElementNS('http://www.w3.org/2000/svg', 'use');
        use.classList.add("ui_icon_svg");
        use.setAttribute("href", src + "#layer1");
        svg.appendChild(use);
    }
}

function initializeIcons() {
    for (const icon of _getDocumentElementsByClassName("ui_icon")) initializeIcon(icon);
}

export function setIconOn(event) {
    event.target.classList.add("on");
}

export function setIconOff(event) {
    event.target.classList.remove("on");
}

export function toggleIcon(event) {
    _toggleClass(event.target, "on");
}

function getIconBox() {
    let iconBox = document.getElementById("icon_box");
    if (!iconBox) {
        iconBox = createElement("div", "icon_box");
        iconBox.id = "icon_box";
        document.body.appendChild(iconBox);
    }

    let top = getRootVar("--icon-box-top");
    if (!iconBox.style.top && top) iconBox.style.top = top;

    let right = getRootVar("--icon-box-right");
    if (!iconBox.style.right && right) iconBox.style.right = right;

    let bottom = getRootVar("--icon-box-bottom");
    if (!iconBox.style.bottom && bottom) iconBox.style.bottom = bottom;

    let left = getRootVar("--icon-box-left");
    if (!iconBox.style.left && left) iconBox.style.left = left;

    return iconBox;
}

//--- input element functions

export function Button (text, action, minWidth=null) {
    let e = createElement("INPUT", "ui_button");
    e.type = "button";
    e.tabIndex = 0; // make button tab-traversable (not default on Safari)
    e.value = text;

    if (minWidth) setWidthStyle(e,minWidth);

    if (action instanceof Function) {
        e.addEventListener("click", action);
    } else {
        e.onclick = action;
    }

    return e;
}

export function setInputDisabled(o, isDisabled) {
    let e = _elementOf(o);
    if (e) {
        if (e.tagName == "INPUT") { // buttons
            e.disabled = isDisabled;
        }
    }
}

export function setButtonDisabled(o, isDisabled) {
    setInputDisabled(o, isDisabled);
}

//--- passive text (no user input, only programmatic)

// static label that is not associated with another element
export function Label (text, eid=null, minWidth=0, maxWidth=0) {
    let e = createElement("DIV", "ui_label");
    if (eid) e.setAttribute("id", eid);
    e.innerText = text;

    setWidthStyle(e, minWidth, maxWidth);
    setLineHeight(e);

    return e;
}

function setLineHeight (e){
    if (e.innerText) {
        let h = getRootVar("--field-height");
        e.style.height = h;
        e.style.lineHeight = h;
    } else {
        e.style.height = 0;
        e.style.lineHeight = 0;
    }
}

// non-interactive variable text element that can be set progrmmatically and has zero height if empty 
export function VarText (text=null, eid=null, minWidth=0, maxWidth=0, opts={}) {
    let e = createElement("DIV", "ui_text");

    if (eid) e.setAttribute("id", eid);
    setWidthStyle(e, minWidth, maxWidth);

    if (opts.isPermanent) e.classList.add("permanent");
    if (opts.isFixed) e.classList.add("fixed");
    if (opts.alignRight) e.classList.add("align_right");

    if (text) e.innerText = text;
    setLineHeight(e);
    
    return e;
}

export function getVarText(o) {
    let e = _elementOf(o);
    if (e && e.classList.contains("ui_text")) {
        return e;
    }
    throw "not a text";
}

export function setVarText(o, text) {
    let e = getVarText(o);
    if (e) {
        e.innerText = text;

        if (!_containsClass(e, "permanent")) {
            setLineHeight(e);
        }
    }
}

//--- text area


//--- fields 

function genField (inputType, extraCls, label, eid, changeAction) {
    let e = createElement("DIV", "ui_field");
    extraCls.forEach( cls=> e.classList.add(cls));

    if (label && label.length > 0) e.setAttribute("data-label", label);
    e.setAttribute("data-type", inputType);
    if (eid) e.setAttribute("data-id", eid);
    if (changeAction) {
        if (changeAction instanceof Function) e.addEventListener("change", changeAction); else e.onchange= changeAction;
    }

    return e;
}

export function noChangeAction(event){}

/** width is in CSS units (rem etc), opts = {isFixed: false, alignRight: false, isDisabled: false, placeHolder: null, changeAction: null} */
export function TextInput (label, eid, width, opts = {}) {
    let extraCls = !opts.isDisabled ? ["input"] : [];
    if (opts.isFixed) extraCls.push("fixed");
    if (opts.alignRight) extraCls.push("align_right");
    let e = genField("text", extraCls, label, eid, opts.changeAction);
    if (opts.placeHolder) e.setAttribute("data-placeholder", opts.placeHolder);

    if (width) e.setAttribute("data-width", width);
    if (!label || label.length == 0) e.style.width = width;

    return e;
}

///////////////////  FIXME - those should use opt object
export function TextField (label, eid, isInput, changeAction) {
    let extraCls = isInput ? ["input"] : [];
    return genField("text", extraCls, label, eid, changeAction);
}

export function NumField (label, eid, isInput, changeAction) {
    let extraCls = isInput ? ["fixed","input","alignRight"] : ["fixed","alignRight"];
    return genField("text", extraCls, label, eid, changeAction);
}

export function ColorField (label, eid, isInput, changeAction){
    let extraCls = isInput ? ["input"] : [];
    return genField("color", extraCls, label, eid, changeAction);
}
//////////////////


function initializeField (e) {
    if (e.tagName == "DIV" && e.children.length == 0) {
        let id = e.dataset.id;
        let labelText = e.dataset.label;
        let inputType = e.dataset.type ? e.dataset.type : "text";

        let dataWidth = _inheritableData(e, 'width');
        let labelWidth = _inheritableData(e, 'labelWidth');

        if (id) {
            if (labelText) {
                let label = createElement("DIV", "ui_field_label", labelText);
                if (labelWidth) label.style.width = labelWidth;
                e.appendChild(label);
            }

            let field = createElement("INPUT", e.classList);
            field.id = id;
            field.type = inputType;

            if (_hasBorderedParent(e)) {
                field.style.border = 'none';
                e.style.margin = '0';
            }
            if (dataWidth) field.style.width = dataWidth;
            e.appendChild(field);

            if (e.classList.contains("input") && inputType == "text") {
                field.setAttribute("placeholder", e.dataset.placeholder);
                field.addEventListener("keypress", _checkKeyEvent);
            } else {
                field.readOnly = true;
            }
        }
    }
}

function _checkKeyEvent(event) {
    if (event.key == "Enter") {
        document.activeElement.blur();
    }
}

export function isFieldDisabled(o) {
    let e = getField(o);
    if (e) {
        return e.childNodes[1].disabled;
    }
}

export function setFieldDisabled(o, isDisabled) {
    let e = getField(o);
    if (e) {
        e.childNodes[1].disabled = isDisabled;
    }
}

export function setField(o, newContent) {
    let e = getField(o);
    if (e) {
        //console.log("## ", newContent, new Error().stack);
        e.value = newContent ? newContent : "";
    }
}

export function getFieldValue(o) {
    let e = getField(o);
    if (e) {
        return e.value;
    }
    return undefined;
}

export function getNonEmptyFieldValue(o) {
    let e = getField(o);
    return (e && e.value && e.value.length > 0) ? e.value : null;
}

export function focusField (o) {
    let e = getField(o);
    if (e) e.focus();
}

export function selectFieldRange(o, i0, i1) {
    let e = getField(o);
    if (e) {
        e.setSelectionRange(i0, i1);
    }
}

export function triggerFieldChange(o) {
    let e = getField(o);
    if (e) {
        let event = new Event('change', { bubbles: true });
        e.dispatchEvent(event);
    }
}

export function getField(o) {
    let e = _elementOf(o);
    if (e && e.classList.contains("ui_field")) {
        if (e.tagName == "INPUT") return e;
        else if (e.tagName == "DIV") return e.lastElementChild;
    }
    throw "not a field";
}


//--- TextArea

// TODO - this should also support labels, like TextInput (which would also require a wrqpping DIV plus init)
export function TextArea (eid, width, height, opts) {
    let e = createElement("TEXTAREA", "ui_textarea");
    if (eid) e.id = eid;
    if (opts.isDisabled) {
        e.classList.add("readonly");
        e.disabled = true;
    }
    if (opts.isVResizable) e.classList.add("vresize");
    if (opts.isFixed) e.classList.add("fixed");

    if (opts.changeAction) {
        e.addEventListener("change", opts.changeAction);
    }
    
    e.style.minWidth = width;
    e.style.minHeight = height;

    if (opts.readonly) e.setAttribute("readonly", true);

    return e;
}

export function setTextContent(o,newContent) {
    let e = getText(o);
    if (e) {
        //e.setHTML(newContent); // use the sanitizer to avoid XSS (allow static html such as links) - not yet supported by Firefox
        e.innerHTML = newContent;
        _resetPanelMaxHeight(e);
    }
}

export function clearTextContent(o) {
    setTextContent(o,null);
}

export function getText(o) {
    let e = _elementOf(o);
    if (e && e.classList.contains("ui_text")) {
        return e;
    }
    throw "not a text";
}

export function getTextArea(o) {
    let e = _elementOf(o);
    if (e && e.classList.contains("ui_textarea")) {
        return e;
    }
    return undefined;
}

export function setTextAreaContent(o,text) {
    let v = getTextArea(o);
    if (v) {
        v.value = text;
    }
}

export function clearTextAreaContent(o){
    let v = getTextArea(o);
    if (v) {
        v.value = null;
    }
}

export function getTextAreaContent(o) {
    let v = getTextArea(o);
    if (v) {
        return v.value;
    }
}

//--- time & date widgets

var _timer = undefined;
const _timerClients = [];

const MILLIS_IN_DAY = 86400000;

export function Clock (label, eid, tz) {
    let e = createElement("DIV", "ui_clock");
    if (label) e.setAttribute("data-label", label);
    if (eid) e.setAttribute("data-id", eid);
    if (tz) e.setAttribute("data-tz", tz);
    return e;
}

export function Timer (label, eid) {
    let e = createElement("DIV", "ui_timer");
    if (label) e.setAttribute("data-label", label);
    if (eid) e.setAttribute("data-id", eid);
    return e;
}

document.addEventListener("visibilitychange", function() {
    if (document.visibilityState === 'visible') {
        if (_timerClients.length > 0) startTime();
    } else {
        stopTime();
    }
});

function _addTimerClient(e) {
    _timerClients.push(e);
}

export function startTime() {
    if (!_timer) {
        let t = Date.now();
        for (let e of _timerClients) {
            e._uiUpdateTime(t);
        }

        _timer = setInterval(_timeTick, 1000);
    }
}

function _timeTick() {
    if (_timerClients.length == 0) {
        clearInterval(_timer);
        _timer = undefined;
    } else {
        let t = Date.now();
        for (let client of _timerClients) {
            client._uiUpdateTime(t);
        }
    }
}

export function stopTime() {
    if (_timer) {
        clearInterval(_timer);
        _timer = undefined;
    }
}

function initializeTimer(e) {
    if (e.tagName == "DIV" && e.children.length == 0) {
        let id = e.dataset.id;
        let labelText = e.dataset.label;

        if (labelText) {
            let label = createElement("DIV", "ui_field_label", labelText);
            e.appendChild(label);
        }

        let tc = createElement("DIV", "ui_timer_value");
        tc.id = id;
        tc.innerText = "0:00:00";
        e.appendChild(tc);

        tc._uiT0 = 0; // elapsed
        tc._uiTimeScale = 1;
        tc._uiUpdateTime = (t) => { _updateTimer(tc, t); };
        _addTimerClient(tc);
    }
}

function _updateTimer(e, t) {
    if (e._uiT0 == 0) e._uiT0 = t;

    if (isShowing(e)) {
        let dt = Math.round((t - e._uiT0) * e._uiTimeScale);
        let s = Math.floor(dt / 1000) % 60;
        let m = Math.floor(dt / 60000) % 60;
        let h = Math.floor(dt / 3600000);

        let elapsed = h.toString();
        elapsed += ':';
        if (m < 10) elapsed += '0';
        elapsed += m;
        elapsed += ':';
        if (s < 10) elapsed += '0';
        elapsed += s;

        e.innerText = elapsed;
    }
}

export function getTimer(o) {
    let e = _elementOf(o);
    if (e && e.tagName == "DIV") {
        if (e.classList.contains("ui_timer_value")) return e;
        else if (e.classList.contains("ui_timer")) return _firstChildWithClass("ui_timer_value");
    }
    throw "not a timer";
}

export function resetTimer(o, timeScale) {
    let e = getTimer(o);
    if (e) {
        e._uiT0 = 0;
        if (timeScale) e._uiTimeScale = timeScale;
    }
}

function initializeClock(e) {
    if (e.tagName == "DIV" && e.children.length == 0) {
        let id = e.dataset.id;
        let labelText = e.dataset.label;

        if (labelText) {
            let label = createElement("DIV", "ui_field_label", labelText);
            e.appendChild(label);
        }

        let tc = createElement("DIV", "ui_clock_wrapper");
        tc.id = id;

        let dateField = createElement("DIV", "ui_clock_date");
        let timeField = createElement("DIV", "ui_clock_time");

        tc.appendChild(dateField);
        tc.appendChild(timeField);

        e.appendChild(tc);

        var tz = e.dataset.tz;
        if (!tz) tz = 'UTC';
        else if (tz == "local") tz = Intl.DateTimeFormat().resolvedOptions().timeZone;

        let dateOpts = {
            timeZone: tz,
            weekday: 'short',
            year: 'numeric',
            month: 'numeric',
            day: 'numeric'
        };
        tc._uiDateFmt = new Intl.DateTimeFormat('en-US', dateOpts);

        let timeOpts = {
            timeZone: tz,
            hour: 'numeric',
            minute: 'numeric',
            second: 'numeric',
            hourCycle: 'h23',
            timeZoneName: 'short'
        };
        tc._uiTimeFmt = new Intl.DateTimeFormat('en-US', timeOpts);

        tc._uiW0 = 0; // ref wall clock
        tc._uiS0 = 0; // ref sim clock
        tc._uiSday = 0; // last sim clock day displayed
        tc._uiStopped = false;
        tc._uiTimeScale = 1;
        tc._isSet = false;

        tc._uiUpdateTime = (t) => { _updateClock(tc, t); };
        _addTimerClient(tc);
    }
}


function _updateClock(e, t) {
    if (e._uiS0 == 0) { // first time initialization w/o previous uiSetClock
        e._uiS0 = t;
        e._uiSday = t / MILLIS_IN_DAY;
        e._uiW0 = t;

        let date = new Date(t);
        let doy = util.dayOfYear(date);
        e._uiDate = date;
        e.children[0].innerText = e._uiDateFmt.format(date).replaceAll("/","-") + " : " + doy;
        e.children[1].innerText = e._uiTimeFmt.format(date);

    } else {
        if (e._uiW0 == 0) { // first time init with previous uiSetClock
            e._uiW0 = t;
        }

        let s = e._uiS0 + (t - e._uiW0) * e._uiTimeScale;
        let date = new Date(s);
        e._uiDate = date;

        // this means we are lagging behind 1 sec with visible update but save a constant stream of
        // allocations if the clock window is not showing
        if (isShowing(e)) {
            let day = s / MILLIS_IN_DAY;
            if (day != e._uiSday) {
                let doy = util.dayOfYear(date);
                e.children[0].innerText = e._uiDateFmt.format(date).replaceAll("/","-") + " : " + doy;
                e._uiLastDay = day;
            }
            e.children[1].innerText = e._uiTimeFmt.format(date);
        }
    }
}

export function getClockDate(o) {
    let e = getClock(o);
    if (e) {
        _updateClock(e, Date.now()); // make sure _uiDate is updated
        return e._uiDate;
    }
    return undefined;
}

export function getClockEpochMillis(o) {
    let e = getClock(o);
    if (e && e._uiDate) {
        _updateClock(e, Date.now()); // make sure _uiDate is updated
        return e._uiDate.getTime();
    }
    return 0;
}

export function isClockSet(o) {
    let e = getClock(o);
    return (e && e._isSet);
}

export function setClock(o, dateSpec, timeScale, notifyClockMonitors=false) {
    let e = getClock(o);
    if (e) {
        if (typeof dateSpec === "number" && !isNaN(dateSpec) &&dateSpec > 0) { 
            let date = new Date(dateSpec);
            if (date) {
                if (date.valueOf() == dateSpec) {
                    e._isSet = true;
                    e._uiDate = date;
                    e._uiS0 = date.valueOf(); 
                    e._uiSday = e._uiS0 / MILLIS_IN_DAY;

                    e.children[0].innerText = e._uiDateFmt.format(date);
                    e.children[1].innerText = e._uiTimeFmt.format(date);

                    if (timeScale){
                        if (typeof timeScale === "number" && timeScale > 0) {
                            e._uiTimeScale = timeScale;
                        } else {
                            console.log("warning: ignore invalid setClock timeScale ", timeScale);
                            e._uiTimeScale = 1;
                        }
                    }
                    if (notifyClockMonitors) clockMonitors.forEach( func=> func(e));
                } else {
                    console.log("warning: ignore setClock with invalid date/time spec ", dateSpec);
                }
            }
        } else {
            console.log("warning: ignore setClock with invalid date/time spec ", dateSpec);
        }
    }
}

export function getClock(o) {
    let e = _elementOf(o);
    if (e && e.tagName == "DIV") {
        if (e.classList.contains("ui_clock_wrapper")) return e;
        else if (e.classList.contains("ui_clock")) return _firstChildWithClass(e, "ui_clock_wrapper");
    }
    throw "not a clock field";
}

const clockMonitors = [];

export function registerClockMonitor(func) {
    clockMonitors.push(func);
}

export function releaseClockMonitor(func) {
    let i = clockMonitors.findIndex( (f)=> Object.is(f, func));
    if (i >= 0) {
        clockMonitors.splice(i, 1);
    }
}

//--- slider widgets

const sliderResizeObserver = new ResizeObserver(entries => {
    for (let re of entries) { // this is a ui_slider_track
        let e = re.target;
        let trackRect = e.getBoundingClientRect();
        let rangeRect = e._uiRange.getBoundingClientRect();

        e._uiRangeWidth = rangeRect.width;
        e._uiScale = (e._uiMaxValue - e._uiMinValue) / rangeRect.width;
        e._uiRangeOffX = (trackRect.width - rangeRect.width) / 2; // left offset of range in track

        _positionLimits(e, trackRect, rangeRect);
        _positionThumb(e);
    }
});

export function Slider (label, eid, changeAction, trackWidth=undefined) {
    let e = createElement( "DIV", "ui_slider");

    if (eid) e.setAttribute("data-id", eid);
    if (label) e.setAttribute("data-label", label);
    if (trackWidth) e.setAttribute("data-width", trackWidth);
    if (changeAction instanceof Function) {
        e.addEventListener("change", changeAction); 
    } else {
        e.onchange = changeAction;
    }

    return e;
}

function initializeSlider (e) {
    if (e.children.length == 0) {
        let id = e.dataset.id;
        let labelText = e.dataset.label;
        // default init - likely to be set by subsequent uiSetSliderRange/Value calls
        let minValue = _parseInt(e.dataset.minValue, 0);
        let maxValue = _parseInt(e.dataset.maxValue, 100);
        let trackWidth = e.dataset.width;
        let v = _parseInt(e.dataset.value, minValue);

        if (maxValue > minValue) {
            if (labelText) {
                let label = createElement("DIV", "ui_field_label", labelText);
                e.appendChild(label);
            }

            let track = createElement("DIV", "ui_slider_track");
            track.id = id;
            track._uiMinValue = minValue;
            track._uiMaxValue = maxValue;
            track._uiStep = _parseNumber(e.dataset.inc);
            track._uiValue = _computeSliderValue(track, v);
            track.addEventListener("click", clickTrack);

            let range = createElement("DIV", "ui_slider_range");
            track._uiRange = range;
            track.appendChild(range);

            let left = createElement("DIV", "ui_slider_limit", minValue);
            left.addEventListener("click", clickMin);
            track._uiLeftLimit = left;
            track.appendChild(left);

            let thumb = createElement("DIV", "ui_slider_thumb", "▲");
            track._uiThumb = thumb;
            thumb.addEventListener("mousedown", startDrag);
            track.appendChild(thumb);

            let num = createElement("DIV", "ui_slider_num");
            num.addEventListener("click", clickNum);
            track._uiNum = num;
            track.appendChild(num);

            let right = createElement("DIV", "ui_slider_limit", maxValue);
            right.addEventListener("click", clickMax);
            track._uiRightLimit = right;
            track.appendChild(right);

            if (trackWidth) {
                track.style.width = trackWidth;
            }

            e.appendChild(track);
            sliderResizeObserver.observe(track);

        } else {
            console.log("illegal range for slider " + id);
        }
    }
}

// slider callbacks

function startDrag(event) {
    let offX = event.offsetX; // cursor offset within thumb
    let track = event.target.parentElement;
    let lastValue = track._uiValue;
    let trackRect = track.getBoundingClientRect();

    track.style.cursor = "ew-resize";
    track.addEventListener("mousemove", drag);
    document.addEventListener("mouseup", stopDrag);
    _consumeEvent(event);

    function drag(event) {
        let e = event.currentTarget; // ui_slider_track

        let x = event.clientX - offX - trackRect.x;
        if (x < 0) x = 0;
        else if (x > e._uiRangeWidth) x = e._uiRangeWidth;

        let v = e._uiMinValue + (x * e._uiScale);
        let vv = _computeSliderValue(e, v);
        if (vv != lastValue) {
            lastValue = vv;
            setSliderValue(e, vv);
        }
        _consumeEvent(event);
    }

    function stopDrag(event) {
        track.style.cursor = "default";
        track.removeEventListener("mousemove", drag);
        document.removeEventListener("mouseup", stopDrag);
        _consumeEvent(event);
    }
}

function clickTrack(event) {
    let e = event.target;
    if (e.classList.contains("ui_slider_track")) { // ignore drags causing clicks with the wrong target
        let x = event.offsetX - e._uiRangeOffX;
        let v = e._uiMinValue + (x * e._uiScale);
        let vv = _computeSliderValue(e, v);
        setSliderValue(e, vv);
        _consumeEvent(event);
    }
}

function clickMin(event) {
    let e = event.target.parentElement;
    setSliderValue(e, e._uiMinValue);
    _consumeEvent(event);
}

function clickMax(event) {
    let e = event.target.parentElement;
    setSliderValue(e, e._uiMaxValue);
    _consumeEvent(event);
}

function clickNum(event) {
    let e = event.target;
    let track = e.parentElement;
    let x = event.offsetX + e.getBoundingClientRect().x - track.getBoundingClientRect().x - track._uiRangeOffX;
    let v = track._uiMinValue + (x * track._uiScale);
    let vv = _computeSliderValue(track, v);
    setSliderValue(track, vv);
    _consumeEvent(event);
}

//

function _positionLimits(e, tr, rr) {
    let trackRect = tr ? tr : e.getBoundingClientRect();
    let rangeRect = rr ? rr : e._uiRange.getBoundingClientRect();
    let top = (rangeRect.height + 2) + "px";

    if (e._uiLeftLimit) {
        let style = e._uiLeftLimit.style;
        style.top = top;
        style.left = "4px";
    }

    if (e._uiRightLimit) {
        let rightRect = e._uiRightLimit.getBoundingClientRect();
        let style = e._uiRightLimit.style;
        style.top = top;
        style.left = (trackRect.width - rightRect.width - 4) + "px";
    }
}

function _positionThumb(e) { // e is ui_slider_track
    let dx = ((e._uiValue - e._uiMinValue) / e._uiScale);
    e._uiThumb.style.left = dx + "px"; // relative pos

    if (e._uiNum && !isNaN(e._uiValue)) {
        if (e._uiValue > (e._uiMaxValue + e._uiMinValue) / 2) { // place left of thumb
            let w = e._uiNum.getBoundingClientRect().width;
            e._uiNum.style.left = (dx - w) + "px";
        } else {
            e._uiNum.style.left = (dx + e._uiThumb.offsetWidth) + "px";
        }
    }
}

function _computeSliderValue(e, v) {
    let minValue = e._uiMinValue;
    let maxValue = e._uiMaxValue;

    if (v <= minValue) return minValue;
    if (v >= maxValue) return maxValue;

    let inc = e._uiStep;
    if (inc) {
        if (inc == 1) return Math.round(v);
        else return minValue + Math.round((v - minValue) / inc) * inc;
    } else {
        return v;
    }
}

export function setSliderRange(o, min, max, step, numFormatter) {
    let e = getSlider(o);
    if (e) {
        e._uiMinValue = min;
        e._uiMaxValue = max;
        if (step) e._uiStep = step;

        if (numFormatter) e._uiNumFormatter = numFormatter;
        if (e._uiLeftLimit) e._uiLeftLimit.innerText = _formattedNum(min, e._uiNumFormatter);
        if (e._uiRightLimit) e._uiRightLimit.innerText = _formattedNum(max, e._uiNumFormatter);

        if (_hasDimensions(e)) {
            _positionLimits(e);
            _positionThumb(e);
        }
    }
}

export function setSliderValue(o, v) {
    let e = getSlider(o);
    if (e) {
        let newValue = _computeSliderValue(e, v);
        if (newValue != e._uiValue) {
            e._uiValue = newValue;
            if (_hasDimensions(e)) _positionThumb(e);
            if (e._uiNum) e._uiNum.innerText = _formattedNum(newValue, e._uiNumFormatter);

            let slider = e.parentElement;
            slider.dispatchEvent(new Event('change'));
        }
    }
}

export function getSliderValue(o) {
    let e = getSlider(o);
    if (e) {
        return e._uiValue;
    }
}

export function getSlider(o) {
    let e = _elementOf(o);
    if (e && e.tagName == "DIV") {
        if (e.classList.contains("ui_slider_track")) return e;
        else if (e.classList.contains("ui_slider")) return _firstChildWithClass(e, "ui_slider_track");
        else if (e.parentElement.classList.contains("ui_slider_track")) return e.parentElement;
    }
    throw "not a slider";
}

//--- choices

export function Choice (label, eid, changeAction, width=null) {
    let e = createElement("DIV", "ui_choice");

    e.setAttribute("data-label", label);
    if (eid) e.setAttribute("data-id", eid);
    if (width) e.setAttribute("data-width", width);

    if (changeAction instanceof Function) {
        e.addEventListener("change", changeAction);
    } else {
        e.onclick = changeAction; // expr.
    }

    return e;
}

function initializeChoice (e) {
    if (e.tagName == "DIV" && e.children.length == 0) {
        let id = e.dataset.id;
        let labelText = e.dataset.label;

        if (labelText) {
            let label = createElement("DIV", "ui_field_label", labelText);
            e.appendChild(label);
        }

        let field = createElement("DIV", "ui_choice_value");
        if (e.dataset.width) field.style.width = e.dataset.width;
        field.id = id;
        field._uiSelIndex = -1;
        field._uiItems = [];

        e.appendChild(field);
    }
}

export function isChoiceDisabled(o) {
    return _isDisabled(getChoice(o));
}

export function setChoiceDisabled(o, isDisabled) {
    _setDisabled(getChoice(o), isDisabled);
}

export function setChoiceItems(o, items, selIndex = -1) {
    let e = getChoice(o);
    if (e) {
        let prevChoices = _firstChildWithClass(e.parentElement, "ui_popup_menu");
        if (prevChoices) e.parentElement.removeChild(prevChoices);
        e._uiItems = items;
        e._uiSelIndex = -1;

        if (items && items.length > 0){
            let choice = e.parentElement;
            e._uiSelIndex = Math.min(selIndex, items.length-1);

            var i = 0;
            let menu = createElement("DIV", "ui_popup_menu");
            for (let item of items) {
                let itemLabel = getItemLabel(item);
                let idx = i;
                let mi = createElement("DIV", "ui_menuitem", itemLabel);
                mi.addEventListener("click", (event) => {
                    event.preventDefault();
                    e.innerText = mi.innerText;
                    if (e._uiSelIndex >= 0) { menu.children[e._uiSelIndex].classList.remove('checked'); }
                    e._uiSelIndex = idx;
                    mi.classList.add('checked');
                    choice.dispatchEvent(new Event('change'));
                });
                if (selIndex == i) {
                    mi.classList.add('checked');
                    e.innerText = itemLabel;
                }
                menu.appendChild(mi);
                i += 1;
            }

            choice.appendChild(menu);
            e.addEventListener("click", (event) => {
                event.stopPropagation();
                popupMenu(event, menu);
                event.preventDefault();
            });
        }
    }
}

export function clearChoiceItems(o) {
    let e = getChoice(o);
    if (e && e._uiItems.length > 0) {
        e._uiItems = [];
        e._uiSelIndex = -1;
        e.innerText = null;
        
        e.parentElement.removeChild( e.nextSibling); // this removes the popup menu
    }
}

export function getChoiceItems(o) {
    let e = getChoice(o);
    if (e) {
        return e._uiItems;
    }
}

export function selectChoiceItemIndex(o, selIndex) {
    let e = getChoice(o);
    if (e) {
        let menu = _firstChildWithClass(e.parentElement, "ui_popup_menu");
        if (selIndex >= 0 && selIndex < menu.childNodes.length) {
            let mi = _nthChildOf(menu, selIndex);
            if (e._uiSelIndex >= 0) { menu.children[e._uiSelIndex].classList.remove('checked'); }
            e._uiSelIndex = selIndex;
            mi.classList.add('checked');
            //e.innerText = mi.innerText;
            e.innerText = getItemLabel(e._uiItems[selIndex]);
            e.parentElement.dispatchEvent(new Event("change"));
        }
    }
}

export function selectChoiceItem(o, item) {
    let e = getChoice(o);
    if (e) {
        let idx = e._uiItems.findIndex( it=> it == item);
        if (idx >= 0) selectChoiceItemIndex(o,idx);
    }
}

export function getSelectedChoiceValue(o) {
    let e = getChoice(o);
    if (e) {
        let choiceName =  e.innerText;
        return e._uiItems.find( it=> choiceName === getItemLabel(it));
    }
}

export function getChoice(o) { // TODO - do we really want the ui_choice_value?
    let e = _elementOf(o);
    if (e) {
        if (e.classList.contains("ui_choice_value")) return e;
        else if (e.classList.contains("ui_choice")) return _firstChildWithClass(e, 'ui_choice_value');
    }
    throw "not a choice";
}

//--- checkboxes

export function CheckBox (label, action, eid=null, isSelected=false) {
    let e = createElement("DIV", "ui_checkbox");

    if (isSelected) _addClass(e, "checked");

    e.setAttribute("data-label", label);
    if (eid) e.setAttribute("id", eid);
    if (action instanceof Function) {
        e.addEventListener("click", action);
    } else {
        e.onclick = action;
    }

    return e;
}

function initializeCheckBox(e) {
    let labelText = e.dataset.label;
    if (_hasNoChildElements(e)) {
        _addCheckBoxComponents(e, labelText);
        e.setAttribute("tabindex", "0");
    }
}

function _addCheckBoxComponents(e, labelText) {
    let btn = createElement("DIV", "ui_checkbox_button");
    btn.addEventListener("click", _clickCheckBoxBtn);
    e.appendChild(btn);

    if (labelText) {
        let lbl = createElement("DIV", "ui_checkbox_label", labelText);
        lbl.addEventListener("click", _clickCheckBoxBtn);
        e.appendChild(lbl);
    }
}

export function createCheckBox(initState, clickHandler, labelText = "") {
    let e = createElement("DIV", "ui_checkbox");
    if (initState) _addClass(e, "checked");
    _addCheckBoxComponents(e, labelText);

    if (clickHandler)  {
        e.addEventListener("click", clickHandler); // watch out - these don't get cloned
    }
    
    return e;
}

export function isCheckBoxDisabled(o) {
    return _isDisabled(getCheckBox(o));
}

export function setCheckBoxDisabled(o, isDisabled) {
    _setDisabled(getCheckBox(o), isDisabled);
}

function _clickCheckBoxBtn(event) {
    let checkbox = getCheckBox(event);
    if (checkbox) {
        _toggleClass(checkbox, "checked");
    }
}

export function toggleCheckBox(o) {
    let checkbox = getCheckBox(o);
    if (checkbox) {
        return _toggleClass(checkbox, "checked");
    }
    throw "not a checkbox";
}

export function setCheckBox(o, check = true) {
    let e = getCheckBox(o);
    if (e) {
        if (check) _addClass(e, "checked");
        else _removeClass(e, "checked");
    }
}

export function triggerCheckBoxAction(o) {
    let e = getCheckBox(o);
    if (e) {
        let click = new CustomEvent("click", { target: e });
        console.log("@@ dispatching ", click);
        e.dispatchEvent(click);
    }
}

export function getCheckBoxLabel (o) {
    let e = getCheckBox(o);
    return e ? e.dataset.label : undefined;
}

export function getCheckBox(o) {
    let e = _elementOf(o);
    if (e) {
        let eCls = e.classList;
        if (eCls.contains("ui_checkbox")) return e;
        else if (eCls.contains("ui_checkbox_button")) return e.parentElement;
        else if (eCls.contains("ui_checkbox_label")) return e.parentElement;
    }
    return undefined;
}

export function isCheckbox(o) {
    let e = _elementOf(o);
    return (e && e.classList.contains("ui_checkbox"));
}

export function isCheckBoxSelected(o) {
    let e = getCheckBox(o);
    if (e) {
        return e.classList.contains("checked");
    }
    throw "not a checkbox";
}

//--- radios

export function Radio (label,action,eid=null, isSelected=false) {
    let e = createElement("DIV", "ui_radio");
    if (isSelected) _addClass(e, "selected");

    e.setAttribute("data-label", label);
    if (eid) e.setAttribute("id", eid);
    if (action instanceof Function) e.addEventListener("click", action); else e.onclick = action;
    return e;
}

function initializeRadio (e) {
    let labelText = e.dataset.label;
    if (_hasNoChildElements(e)) {
        _addRadioComponents(e, labelText);
        e.setAttribute("tabindex", "0");
    }
}

function _addRadioComponents(e, labelText) {
    let btn = createElement("DIV", "ui_radio_button");
    btn.addEventListener("click", _clickRadio);
    e.appendChild(btn);

    if (labelText) {
        let lbl = createElement("DIV", "ui_radio_label", labelText);
        lbl.addEventListener("click", _clickRadio);
        e.appendChild(lbl);
    }
}

export function createRadio(initState, clickHandler, labelText) {
    let e = createElement("DIV", "ui_radio");
    if (initState) _addClass(e, "selected");
    _addRadioComponents(e, labelText);
    if (clickHandler) e.addEventListener("click", clickHandler);
    return e;
}

export function isRadioBoxDisabled(o) {
    return _isDisabled(getRadio(o));
}

export function setRadioDisabled(o, isDisabled) {
    _setDisabled(getRadio(o), isDisabled);
}

function _clickRadio(event) {
    let e = getRadio(event);
    if (e) {
        if (!e.classList.contains("selected")) {
            for (let r of e.parentElement.getElementsByClassName("ui_radio")) {
                if (r !== e) {
                    if (r.classList.contains("selected")) r.classList.remove("selected");
                }
            }
            e.classList.add("selected");
        }
    }
}

export function selectRadio(o) {
    let e = getRadio(o);
    if (e) {
        if (!e.classList.contains("selected")) {
            for (let r of e.parentElement.getElementsByClassName("ui_radio")) {
                if (r !== e) {
                    if (r.classList.contains("selected")) r.classList.remove("selected");
                }
            }
            _addClass(e, "selected");
        }
        return true;
    } else {
        return false;
    }
}

export function isRadioSelected(o) {
    let e = getRadio(o);
    if (e) {
        return e.classList.contains("selected");
    }
    throw "not a radio";
}

export function getRadio(o) {
    let e = _elementOf(o);
    if (e) {
        let eCls = e.classList;
        if (eCls.contains("ui_radio")) return e;
        else if (eCls.contains("ui_radio_button")) return e.parentElement;
        else if (eCls.contains("ui_radio_label")) return e.parentElement;
    }
    return undefined;
}

export function getRadioLabel(o) {
    let e = getRadio(o);
    return e ? e.dataset.label : undefined;
}

export function isRadio(o) {
    let e = _elementOf(o);
    return (e && e.classList.contains("ui_radio"));
}

export function clearRadioGroup(o) {
    let e = _elementOf(o);
    let n = 0;
    while (n == 0 && e) {
        for (let r of e.getElementsByClassName("ui_radio")) {
            r.classList.remove("selected");
            n++;
        }
        e = e.parentElement;
    }
}

//--- common interfact for checkboxes and radios

export function getSelector(o) {
    var e = getCheckBox(o);
    if (!e) e = getRadio(o);
    return e;
}

export function isSelector(o) {
    let e = _elementOf(o);
    return (e && (e.classList.contains("ui_checkbox") || e.classList.contains("ui_radio")));
}

export function isSelectorSet(o) {
    let e = _elementOf(o);
    if (e) {
        if (e.classList.contains("ui_checkbox")) return e.classList.contains("checked");
        else if (e.classList.contains("ui_radio")) return e.classList.contains("selected");
    }
    return false;
}

export function setSelector(o, newState) {
    let e = _elementOf(o);
    if (e) {
        if (e.classList.contains("ui_checkbox")) {
            setCheckBox(e, newState);
        } else if (e.classList.contains("ui_radio")) {
            if (newState) selectRadio(e);
            else _removeClass(e, "selected");
        }
    }
}

export function isSelectorDisabled(o) {
    return _isDisabled(getSelector(o));
}

export function setSelectorDisabled(o, isDisabled) {
    _setDisabled(getSelector(o), isDisabled);
}

export function getSelectorLabel(o) {
    let e = getSelector(o);
    return e ? e.dataset.label : undefined;
}

//--- key-value tables (2 column lists, first column contains right aligned labels)

export function KvTable(eid, maxRows, minWidth, maxWidth) {
    let e = createElement("DIV", "ui_kvtable");

    if (eid) e.setAttribute("id", eid);
    e.setAttribute("data-rows", maxRows.toString());
    setWidthStyle(e, minWidth, maxWidth);

    return e;
}

function initializeKvTable (e) {
    if (!e.style.maxHeight) {
        let itemHeight = getRootVar("--list-item-height");
        let itemPadding = getRootVar("--list-item-padding");
    
        let nRows = _intDataAttrValue(e, "rows", 8);
        e.style.maxHeight = `calc(${nRows} * (${itemHeight} + ${itemPadding}))`; // does not include padding, borders or margin
    }
}

export function getKvTable(o) {
    return _nearestElementWithClass(_elementOf(o), "ui_kvtable");
}

export function setKvList (o, kvList, createValueElement = undefined) {
    let e = getKvTable(o);
    if (e) {
        _removeChildrenOf(e);
        if (kvList) {
            kvList.forEach( kv=> {
                let tr = createElement("TR");
                let le = createElement("TD", "ui_field_label");
                le.innerText = kv[0];
                tr.appendChild(le);

                let ve = createElement("TD", "ui_field");
                if (createValueElement) {
                    let ce = createValueElement(kv[1]);
                    ve.appendChild(ce);
                } else {
                    if (util.isNumber(kv[1])) ve.classList.add("fixed");
                    ve.innerText = kv[1];
                }
                tr.appendChild(ve);

                e.appendChild(tr);
            });
        }
        _resetPanelMaxHeight(e);
    }
}

export function clearKvList(o) {
    setKvList(o,null);
}

// no interaction for kv tables


//--- lists (with columns)

function genList (subCls, eid, maxRows, selectAction, clickAction, contextMenuAction, dblClickAction) {
    let e = createElement("DIV", "ui_list");

    if (subCls) e.classList.add(subCls);
    if (eid) e.setAttribute("id", eid);
    e.setAttribute("data-rows", maxRows.toString());

    if (selectAction) {
        e._uiSelectAction = selectAction; // don't use attribute here
        e.addEventListener("selectionChanged", selectAction);
    }

    if (clickAction instanceof Function) e.addEventListener("click",clickAction); else  e.onclick= clickAction;
    if (contextMenuAction instanceof Function) e.addEventListener("contextmenu",contextMenuAction); else e.oncontextmenu= contextMenuAction;
    if (dblClickAction instanceof Function) e.addEventListener("dblclick", dblClickAction); else e.ondblclick= dblClickAction;

    return e;
}

export function List (eid, maxRows, selectAction, clickAction, contextMenuAction, dblClickAction) {
    return genList(null, eid, maxRows, selectAction, clickAction, contextMenuAction, dblClickAction);
}

export function TreeList (eid, maxRows, minWidth, selectAction, clickAction, contextMenuAction, dblClickAction) {
    let e = genList("ui_tree", eid, maxRows, selectAction, clickAction, contextMenuAction, dblClickAction);
    setWidthStyle(e, minWidth);
    return e;
}

export function ListWithPopup (eid, maxRows, selectAction, clickAction, contextMenuAction, dblClickAction) {
    return function (...menuItems) {
        let e = genList(null, eid, maxRows, selectAction, clickAction, contextMenuAction, dblClickAction);

        if (menuItems.length > 0) {
            let popup = PopupMenu()(menuItems);
            e.appendChild(popup)
        }

        return e;
    }
}

function initializeList (e) {
    if (!e._uiItemMap) {
        let itemHeight = getRootVar("--list-item-height");
        let itemPadding = getRootVar("--list-item-padding");
        
        let nRows = _intDataAttrValue(e, "rows", 8);

        e.style.maxHeight = `calc(${nRows} * (${itemHeight} + ${itemPadding}))`; // does not include padding, borders or margin
        e.setAttribute("tabindex", "0");
        if (e.firstElementChild && e.firstElementChild.classList.contains("ui_popup_menu")) _hoistChildElement(e.firstElementChild);

        if (!e._uiSelectAction) {
            let selectAction = _dataAttrValue(e, "onselect"); // single click or programmatic
            if (selectAction) {
                e._uiSelectAction = Function("event", selectAction);
                e.addEventListener("selectionChanged", e._uiSelectAction);
            }
        }

        e.addEventListener("keydown", listKeyDownHandler);

        // element state
        e._uiSelectedItemElement = null; // we add a property to keep track of selections
        e._uiMapFunc = item => item.toString(); // item => itemElement text
        e._uiItemMap = new Map(); // item -> itemElement

        addListWrapper(e); // reparent
    }
}

function listKeyDownHandler(event) {
    let list = getList(event.target);
    if (event.key === 'Escape') { 
        clearSelectedListItem(list);
        stopAllOtherProcessing(event);
    } else if (event.key === 'ArrowDown') {
        if (list._uiSelectedItemElement) selectNextListItem(list); else selectFirstListItem(list);
        stopAllOtherProcessing(event);
    } else if (event.key === 'ArrowUp') {
        if (list._uiSelectedItemElement) selectPrevListItem(list); else selectLastListItem(list);
        stopAllOtherProcessing(event);
    } else if (event.key === 'E') { // expand-all tree list only
        if (list._uiRoot) expandAllNodes(list);
    } else if (event.key === 'C') { // collapse-all tree list only
        if (list._uiRoot) collapseAllNodes(list);
    }
}

// re-parent if not yet in a ui_list_wrapper - we need a parent if there are column headers but also to ensure layout if the content is changed
function addListWrapper(list) {
    let parent = list.parentElement;
    if (!parent.classList.contains("ui_list_wrapper")){
        let listWrapper = createElement("DIV", "ui_list_wrapper");
        parent.insertBefore(listWrapper, list);
        listWrapper.appendChild(list); // reparent
    }
}

// multi-column item display (this will override single column display)
export function setListItemDisplayColumns(o, listAttrs, colSpecs) {
    let e = getList(o);
    if (e) {
        let defaultWidth = getRootVar("--list-item-column-width", "5rem");
        let totalWidth = "";
        let re = createElement("DIV", "ui_list_item");
        let he = (listAttrs.includes("header")) ? createElement("DIV", "ui_list_header") : null;

        colSpecs.forEach(cs => {
            let ce = createElement("DIV", "ui_list_subitem");

            ce._uiMapFunc = cs.map;

            let w = cs.width ? cs.width : defaultWidth;
            ce.style.width = w;
            ce.style.maxWidth = w;

            if (totalWidth) totalWidth += " + ";
            totalWidth += w;

            _setAlignment(ce, cs.attrs);
            if (cs.attrs.includes("fixed")) _addClass(ce, "fixed");
            if (cs.attrs.includes("small")) _addClass(ce, "small");

            re.appendChild(ce);
            if (he) he.appendChild(createSubitemHeader(he, cs, w));
        });

        if (!_containsClass(e, "ui_tree")) {
            e.style.width = `calc(${totalWidth} + var(--scrollbar-track-width) + 2*var(--border-width) + 2*var(--list-item-padding))`;
            if (listAttrs.includes("fit")) _addClass(e, "fit");
        }

        e._uiRowPrototype = re;

        if (he) addListHeader(e,he);
    }
}

function createSubitemHeader(header, cs, w) {
    let e = createElement("DIV", "ui_list_subitem header");
    e.style.flexBasis = w;
    e.innerText = cs.name;
    _setAlignment(e, cs.attrs);

    if (cs.tip) createTooltip(e, cs.tip);

    return e;
}

function addListHeader(list, header) {
    list.parentElement.insertBefore(header,list);
    list._uiHeader = header;
}

export function listItemSpacerColumn(remWidth = 1) {
    return { name: "", width: remWidth + "rem", attrs: [], map: e => " " };
}

// single column item display
export function setListItemDisplay(o, styleWidth, attrs, mapFunc) {
    let e = getList(o);
    if (e) {
        if (mapFunc) e._uiMapFunc = mapFunc;
        if (styleWidth) e.style.width = styleWidth;

        if (attrs.includes("alignLeft")) _addClass(e, "align_left");
        else if (attrs.includes("alignRight")) _addClass(e, "align_right");
        if (attrs.includes("fixed")) _addClass(e, "fixed");
        if (attrs.includes("small")) _addClass(e, "small");
    }
}

function _setSubItemsOf(ie, item) {
    for (let i = 0; i < ie.childElementCount; i++) {
        let ce = ie.children[i];
        let v = ce._uiMapFunc(item);

        // remove old content (we don't accumulate)
        _removeChildrenOf(ce);
        ce.innerText = "";

        if (v instanceof HTMLElement) {
            ce.appendChild(v);
        } else {
            ce.innerText = v;
        }
    }
}

function _setListItem(e, ie, item) {
    if (ie.childElementCount > 0) {
        _setSubItemsOf(ie, item);
    } else {
        ie.innerText = e._uiMapFunc(item,ie);
    }
    ie._uiItem = item;
    e._uiItemMap.set(item, ie);
}

export function getListItemOfElement(e) {
    let li = nearestParentWithClass(e, "ui_list_item");
    return li ? li._uiItem : null;
}

export function getElementOfListItem(o, item) {
    let e = getList(o);
    if (e) {
        return e._uiItemMap.get(item);
    } else return null;
}

export function getNthSubElementOfListItem(o, item, n) {
    let e = getList(o);
    if (e) {
        let it = e._uiItemMap.get(item);
        if (it) {
            let sub = _nthChildOf(it, n);
            if (sub && sub.classList.contains("ui_list_subitem")) return sub;
            else return null;
        } else return null;
    } else return null;
}

function _cloneRowPrototype(proto) {
    let e = proto.cloneNode(true);
    for (let i = 0; i < e.childElementCount; i++) {
        let ce = e.children[i];
        let pe = proto.children[i];

        ce._uiMapFunc = pe._uiMapFunc;
    }
    return e;
}

function _createListItemElement(e, item, rowProto = undefined) {
    let ie = undefined;

    if (rowProto) {
        ie = _cloneRowPrototype(rowProto);
        _setSubItemsOf(ie, item);
    } else {
        ie = createElement("DIV", "ui_list_item", e._uiMapFunc(item));
    }
    ie._uiItem = item;
    e._uiItemMap.set(item, ie);
    ie.addEventListener("click", _selectListItem);
    return ie;
}

export function setListItems(o, items) {
    let e = getList(o);
    if (e) {
        // TODO - do we have to store/restore scrollLeft/scrollTop ?
        _setSelectedItemElement(e, null);

        if (items && items.length > 0) {
            let i = 0;
            let ies = e.children; // the ui_list_item children
            let i1 = ies.length;
            let proto = e._uiRowPrototype;

            items.forEach(item => {
                if (i < i1) { // replace existing element
                    _setListItem(e, ies[i], item);
                } else { // append new element
                    let ie = _createListItemElement(e, item, proto);
                    e.appendChild(ie);
                }
                i++;
            });

            if (e.childElementCount > i) _removeLastNchildrenOf(e, e.childElementCount - i);
        } else {
            clearList(e);
        }
        _resetPanelMaxHeight(e);
    }
}

//--- tree list variation

// tree lists are normal lists in which we wrap item elements into tree node elements. Those
// tree node elements are the child elements of the list and are used to control expand/collapse
// This also means we have tree nodes without data (for each item element parent level)

export function setTree(o,root) {
    let e = getTreeList(o);
    if (e) {
        _setSelectedItemElement(e, null);
        _removeChildrenOf(e);

        if (root && root.constructor && root.constructor.name === 'ExpandableTreeNode') {
            e._uiRoot = root;

            root.expandedDescendants().forEach( node=>e.appendChild(_createNodeElement(e, node)));
            _resetPanelMaxHeight(e);
        }
    }
}

// this returns an ExpandableTreeNode
export function getRootNode(o) {
    let e = getTreeList(o);
    return e ? e._uiRoot : null;
}

function getNodeElement (e, node) {
    for (let ce = e.firstChild; ce; ce = ce.nextElementSibling) {
        if (Object.is(ce._uiNode,node)) return ce;
    }
    return null;
}

export function sortInTreeItem (o, item, pathName, isSticky=false) {
    let e = getTreeList(o);
    if (e && e._uiRoot) {
        let root = e._uiRoot;
        let addedNodes = root.sortInPathName( pathName, item, isSticky);
        let firstNode = addedNodes[0];

        if (firstNode.parent.isExpanded) { // first added node is showing -> insert
            let i = firstNode.visibleIndex();
            let ne = e.firstElementChild;
            while (i--)  ne = ne.nextElementSibling;

            let newElement = _createNodeElement(e, firstNode); // the element to insert
            if (ne) {
                e.insertBefore( newElement, ne);
            } else {
                e.appendChild( newElement);
            }
            _resetPanelMaxHeight(e);
        }

        return addedNodes;
    }
    return null;
}

export function removeTreeItemPath(o,pathName) {
    let e = getTreeList(o);
    if (e && e._uiRoot) {
        let removedNodes = e._uiRoot.removePathName(pathName);
        for (let node of removedNodes) {
            let ce = getNodeElement(e, node); 
            if (ce) { // it was visible
                if (e._uiSelectedNodeElement && Object.is( e._uiSelectedNodeElement._uiNode, node)) {
                    _setSelectedItemElement(e, null);
                }
                e.removeChild(ce);
            }
        }
        if (removedNodes.length > 0) {
            let parentNode = removedNodes[0].parent;
            if (!parentNode.hasChildren()){ // happens if it is a sticky node
                let pe = getNodeElement(e, parentNode);
                let prefixElem = pe.firstElementChild.firstElementChild; // ui_node { ui_node_header { ui_node_prefix, ui_node_name } }
                if (prefixElem && _containsClass( prefixElem, "ui_node_prefix")) {
                    prefixElem.innerText = parentNode.nodePrefix();
                }
            }
            _resetPanelMaxHeight(e);
        }
        return removedNodes;
    }
    return null; // wasn't there
}

function _createNodeElement(e, node) {
    let ne = createElement("DIV", "ui_node");
    ne._uiNode = node;

    let nHeader = createElement("DIV", "ui_node_header");
    let nPrefix = createElement("DIV", "ui_node_prefix", node.nodePrefix());
    nPrefix.addEventListener("click", clickNodePrefix);
    let nName = createElement("DIV", "ui_node_name", node.name);
    nHeader.appendChild(nPrefix);
    nHeader.appendChild(nName);
    ne.appendChild(nHeader);

    let proto = e._uiRowPrototype;
    if (node.data) {
        if (proto) {
            ne.appendChild( _createListItemElement(e, node.data, proto));
        } else { // create an invisible dummy element so that it can be selected
            let ie = createElement("DIV", "ui_list_item");
            ie.style.display = "none";
            ie._uiItem = node.data; // ? do we have to add this to the list._uiItemMap
            ne.appendChild( ie); // add a dummy element
        }
    } else {
        _addClass(nName, "no_data");
    }

    //nName.addEventListener("click", selectNode);  // ??   1x if clicked on name, 2x if outside
    ne.addEventListener("click", selectNode);

    return ne;
}

function selectNode (event) {
    let ne = _nearestElementWithClass(event.target,"ui_node");
    if (ne) {
        let list = nearestParentWithClass(ne, "ui_list");
        if (list._uiSelectedNodeElement) _removeClass(list._uiSelectedNodeElement, "selected");
        list._uiSelectedNodeElement = ne;
        _addClass(ne, "selected");

        let selElem = null;
        if (ne._uiNode.data) {
            let ie = ne.firstChild.nextElementSibling;
            if (ie && _containsClass(ie, "ui_list_item")) selElem = ie;
        } 
         _setSelectedItemElement(list,selElem);
    }
}

export function expandAllNodes (o) {
    let e = getTreeList(o);
    if (e && e._uiRoot) {
        e._uiRoot.firstChild.expandAll();
        repopulateNodes(e);
    }
}

export function collapseAllNodes (o) {
    let e = getTreeList(o);
    if (e && e._uiRoot) {
        e._uiRoot.firstChild.collapseAll();
        repopulateNodes(e);
    }
}

function repopulateNodes (e) {
    _setSelectedItemElement(e, null);
    _removeChildrenOf(e);
    e._uiRoot.expandedDescendants().forEach( node=>e.appendChild(_createNodeElement(e, node)));
    _resetPanelMaxHeight(e);
}

function clickNodePrefix(event) {
    function collapse (e, ne) {
        let node = ne._uiNode;
        let lvl = node.level();
        for (let nne = ne.nextElementSibling; nne && nne._uiNode.level() > lvl; nne = ne.nextElementSibling) {
            if (nne._uiNode.data) { e._uiItemMap.delete(nne._uiNode.data) }
            if (Object.is(nne, e._uiSelectedItemElement)) { clearSelectedListItem(e) }
            nne._uiNode.collapse();
            e.removeChild(nne);
        }
        node.collapse();
    }

    function expand (e, ne) {
        let node = ne._uiNode;
        let nne = ne.nextElementSibling;
        node.expandedDescendants().forEach( dn=> {
            let dne = _createNodeElement(e, dn, e._uiRowPrototype);
            if (nne) e.insertBefore(dne, nne);
            else e.appendChild(dne);
        });
    }

    let ne = _nearestElementWithClass(event.target,"ui_node");
    let e = ne.parentElement;
    _consumeEvent(event);

    if (ne) {
        let node = ne._uiNode;
        if (node.hasChildren()){
            if (node.isExpanded) { 
                if (event.shiftKey) { // expand all children
                    for (let cnode = node.firstChild; cnode; cnode = cnode.nextSibling) {
                        if (!cnode.isExpanded) {
                            let nne = findNodeElement(e, cnode);
                            cnode.expand();
                            cnode.expandAll();
                            expand(e, nne);
                        }
                    }
                } else { // collapse
                    if (event.metaKey) { // collapse all children
                        for (let cnode = node.firstChild; cnode; cnode = cnode.nextSibling) {
                            if (cnode.isExpanded) {
                                let nne = findNodeElement(e, cnode);
                                collapse( e, nne);
                            }
                        }
                    } else { // collapse node
                        collapse( e, ne);
                    }
                }

            } else { // is collapsed -> expand
                node.expand();
                if (event.metaKey) node.expandAll(); // expand whole subtree
                expand(e, ne);
            }

            let nPrefix = ne.firstElementChild.firstElementChild;
            nPrefix.innerText = node.nodePrefix();
        }
    }
}

export function getSelectedTreeNode (o) {
    let e = getTreeList(o);
    return (e && e._uiSelectedNodeElement) ? e._uiSelectedNodeElement._uiNode : null;
}

export function selectNodePath (o,path) {
    let e = getTreeList(o);
    if (e){
        let n = e._uiRoot.findNode(path);
        if (n) {
            if (e._uiSelectedNodeElement) {
                _removeClass(e._uiSelectedNodeElement, "selected");
                e._uiSelectedNodeElement = null;
            }

            let ie = findNodeElement(e, n);
            if (ie) { // it was already expanded
                selectAndShow(e, ie);

            } else {
                let parents = n.parents(); // the nodes from n to root
                let itemElements = e.children;
                let j=0;
                let selElem = null;
                for (let i=1; i < parents.length; i++){
                    let pn = parents[i];
                    if (!pn.isExpanded){
                        pn.isExpanded = true;
                        while (!Object.is( itemElements[j]._uiNode, pn)) j++;

                        let nextElem = j < itemElements.length-1 ? itemElements[j+1] : null; 
                        for (let cn=pn.firstChild; cn; cn = cn.nextSibling) {
                            let ie = _createNodeElement(e, cn);
                            if (nextElem) { e.insertBefore( ie, nextElem) } else { e.appendChild( ie) }
                            if (Object.is( cn, n)) selElem = ie;
                        }
                    }
                }
                selectAndShow(e, selElem);
            }
            return true;

        } else {
            clearSelectedListItem(e);
        }
    }
    return false;
}

function findNodeElement (e, treeNode) {
    let nodeList = e.childNodes;
    for (let i=0; i<nodeList.length; i++) {
        let ne = nodeList[i];
        if (Object.is( treeNode, ne._uiNode)) return ne;
    }
    return null;
}

export function findNodeElementOfItem (e, item) {
    let nodeList = e.childNodes;
    for (let i=0; i<nodeList.length; i++) {
        let ne = nodeList[i];
        if (Object.is( item, ne._uiNode.data)) return ne;
    }
    return null;
}

export function replaceNodeItem (o, oldItem, newItem) {
    let e = getTreeList(o);
    if (e){
        let ne = findNodeElementOfItem(e, oldItem);
        if (ne) {
            ne._uiNode.data = newItem;

            let ie = ne.firstChild.nextElementSibling;
            if (ie && _containsClass(ie, "ui_list_item")) {
                e._uiItemMap.delete( oldItem);
                _setListItem(e, ie, newItem);
            }
        }
    }
}

export function getTreeList (o) {
    let e = getList(o);
    return (e && e.classList.contains("ui_tree")) ? e : null;
}

//--- end tree list


function selectAndShow(e, ie) {
    if (ie) {
        _setSelectedItemElement(e, ie);
        ie.scrollIntoView({behavior: "smooth", block: "center"});
    }
}

export function setSelectedListItem(o, item) {
    let e = getList(o);
    if (e) {
        selectAndShow(e, e._uiItemMap.get(item));
    }
}

export function setSelectedListItemIndex(o, idx) {
    let listBox = getList(o);
    if (listBox) {
        selectAndShow(listBox, _nthChildOf(listBox, idx));
    }
}

export function selectFirstListItem(o) {
    let listBox = getList(o);
    if (listBox) {
        selectAndShow(listBox, listBox.firstChild);
    }
}

export function selectLastListItem(o) {
    let listBox = getList(o);
    if (listBox) {
        selectAndShow(listBox, listBox.lastChild);
    }
}

export function selectNextListItem(o) {
    let listBox = getList(o);
    if (listBox) {
        let sel = listBox._uiSelectedItemElement;
        if (sel) {
            selectAndShow(listBox, nextItemElement(sel));
        }
    }
}

export function clearListItemCheckBoxes(o) {
    let listBox = getList(o);
    if (listBox) {
        for (let cb of listBox.getElementsByClassName("ui_checkbox")) {
            if (isCheckBoxSelected(cb)) {
                setCheckBox(cb, false);
                triggerCheckBoxAction(cb);
            }
        }
    }
}

function nextItemElement (ie) {
    if (_containsClass(ie, "ui_list_item")) {
        let p = ie.parentElement;
        if (p.classList.contains("ui_node")) { // it's a tree list
            if (p.nextElementSibling ){
                let ne = p.nextElementSibling;
                return (ne.childElementCount > 1) ? ne.children[1] : ne;
            } else return null;
        } else {  // normal list
            return ie.nextElementSibling;
        }
    } else {
        let ne = ie.nextElementSibling;
        return (ne && ne.childElementCount > 1) ? ne.children[1] : ne;
    }
}

export function selectPrevListItem(o) {
    let listBox = getList(o);
    if (listBox) {
        let sel = listBox._uiSelectedItemElement;
        if (sel) {
            selectAndShow(listBox, prevItemElement(sel));
        }
    }
}

function prevItemElement (ie) {
    if (_containsClass(ie, "ui_list_item")) {
        let p = ie.parentElement;
        if (p.classList.contains("ui_node")) { // it's a tree list
            if (p.previousElementSibling){
                let pe = p.previousElementSibling;
                return (pe.childElementCount > 1) ? pe.children[1] : pe; // could be a header node
            } else return null;
        } else { // normal list
            return ie.previousElementSibling;
        }
    } else {
        let pe = ie.previousElementSibling;
        return (pe && pe.childElementCount > 1) ? pe.children[1] : pe;
    }
}

export function getSelectedListItem(o) {
    let listBox = getList(o);
    if (listBox) {
        let sel = listBox._uiSelectedItemElement;
        if (sel) {
            return sel._uiNode ? sel._uiNode.data : sel._uiItem;
        } else return null;

    } else throw "not a list";
}

export function getSelectedListItemIndex(o) {
    let listBox = getList(o);
    if (listBox) {
        let sel = listBox._uiSelectedItemElement;
        if (sel) {
            return _childIndexOf(sel);
        }
    }
    return -1;
}

export function appendListItem(o, item) {
    let e = getList(o);
    if (e) {
        let proto = e._uiRowPrototype;
        let ie = _createListItemElement(e, item, proto);
        e.appendChild(ie);
    }
}

export function insertListItem(o, item, idx) {
    let e = getList(o);
    if (e) {
        let proto = e._uiRowPrototype;
        let ie = _createListItemElement(e, item, proto);
        if (idx < e.childElementCount) {
            e.insertBefore(ie, e.children[idx]);
        } else {
            e.appendChild(ie);
        }
    }
}

export function removeListItemIndex (o, idx) {
    let e = getList(o);
    if (e) {
        if (idx >= 0 && idx < e.childElementCount) {
            let ie = e.children[idx];
            if (Object.is( ie, e._uiSelectedItemElement)){
                clearSelectedListItem(e);
            }
            e._uiItemMap.delete(ie._uiItem);
            e.removeChild(ie);
        }
    }
}

export function replaceListItemIndex(o, idx, item) {
    let e = getList(o);
    if (e) {
        if (idx >= 0 && idx < e.childElementCount) {
            let ie = e.children[idx];
            e._uiItemMap.delete(ie._uiItem);
            _setListItem(e, ie, item);
        }
    }
}

export function replaceListItem(o, oldItem, newItem) {
    let e = getList(o);
    if (e) {
        let ie = getElementOfListItem(oldItem);
        if (ie) {
            e._uiItemMap.delete(ie._uiItem);
            _setListItem(e, ie, newItem);

            if (Object.is( ie, e._uiSelectedItemElement)){
                clearSelectedListItem(e);
                setSelectedListItem(e, newItem); // this fires change events
            }
        }
    }
}

/*
export function updateSelectedListItem(o) {
    let e = getList(o);
    if (e) {
        let ie = e._uiSelectedItemElement;
        if (ie) {
            _setListItem(e, ie, ie._uiItem);
        }
    }
}
*/

export function updateListItem(o, item) {
    let e = getList(o);
    if (e) {
        let ie = e._uiItemMap.get(item);
        if (ie) {
            _setListItem(e, ie, item);
        }
    } else throw "not a list";
}

export function updateListItems(o) {
    let e = getList(o);
    if (e) {
        e._uiItemMap.forEach( (ie,item) => _setListItem(e, ie, item));
    }
}

export function removeListItem(o, item) {
    let e = getList(o);
    if (e) {
        let ie = e._uiItemMap.get(item);
        if (ie) {
            if (e._uiSelectedItemElement === ie) _setSelectedItemElement(e, null);
            e.removeChild(ie);
        }
    }
}

export function removeLastNListItems(o, n) {
    let e = getList(o);
    if (e) {
        _removeLastNchildrenOf(e, n);
    }
}

// NOTE if this is a tree item elements hava a node element as parent (both are 'selected')
function _setSelectedItemElement(listBox, itemElement, srcEvent) {
    let prevItem = null;
    let nextItem = null;

    let prevItemElem = listBox._uiSelectedItemElement;

    if (prevItemElem && itemElement && Object.is( prevItemElem,itemElement)) { // no changes of non-empty selection
        prevItem = itemElement._uiNode ? itemElement._uiNode.data : itemElement._uiItem;
        nextItem = prevItem;

    } else {
        // remove current selection attrs
        if (prevItemElem) _removeClass( prevItemElem, "selected");
        if (listBox._uiSelectedNodeElement) _removeClass( listBox._uiSelectedNodeElement, "selected");

        if (!Object.is( prevItemElem,itemElement)) {
            if (itemElement) {
                nextItem = itemElement._uiNode ? itemElement._uiNode.data : itemElement._uiItem;

                listBox._uiSelectedItemElement = itemElement;
                _addClass( itemElement, "selected");

                if (_containsClass( itemElement.parentElement, "ui_node")) {
                    listBox._uiSelectedNodeElement = itemElement.parentElement;
                    _addClass( itemElement.parentElement, "selected");
                }
            } else {
                listBox._uiSelectedItemElement = null;
            }
        }
    }

    if (listBox._uiSelectAction) {
        let event = new CustomEvent("selectionChanged", {
            bubbles: true,
            detail: {
                curSelection: nextItem,
                prevSelection: prevItem,
                src: srcEvent
            }
        });

        listBox.dispatchEvent(event);
    }
}

function _selectListItem(event) {
    let itemElement = event.currentTarget;
    if (itemElement.classList.contains("ui_list_item")) {
        let listBox = nearestParentWithClass(itemElement, "ui_list");
        if (listBox) {
            _setSelectedItemElement(listBox, itemElement, event);
        }
    }
}

export function clearSelectedListItem(o) {
    let e = getList(o);
    if (e) {
        if (e._uiSelectedNodeElement) {
            _removeClass(e._uiSelectedNodeElement, "selected");
            e._uiSelectedNodeElement = undefined;
        }
        _setSelectedItemElement(e, null);
    }
}

export function clearList(o) {
    let e = getList(o);
    if (e) {
        _setSelectedItemElement(e, null);
        _removeChildrenOf(e);
    }
}

export function getList(o) {
    let e = _elementOf(o);
    if (e) {
        let eCls = e.classList;
        if (eCls.contains("ui_list")) return e;
        else if (eCls.contains("ui_list_item")) return e.parentElement;
        else if (eCls.contains("ui_list_subitem")) return e.parentElement.parentElement;
    }

    return undefined;
}

//--- list controls (first,up,down,last,clear buttons)

export function ListControls (listId, first=null,up=null,down=null,last=null,clear=null) {
    let e = createElement("DIV", "ui_listcontrols");

    // we can't use dataset for these as dataset values are stringified (we store funtions)
    e._listId = listId;
    if (first) e._first = first;
    if (up) e._up = up;
    if (down) e._down = down;
    if (last) e._last = last;
    if (clear) e._clear = clear;

    return e;
}

function initializeListControls(e) {
    if (e.tagName == "DIV" && _hasNoChildElements(e)) {
        // note that this window might not yet be linked into the document so we have to dig the list up from our parent
        let w = getWindow(e); // this gets the nearest window
        if (w) {
            let le = _findChildWithId(w, e._listId);
            if (le && le.classList.contains("ui_list")) {
                e.appendChild( createListControlButton("⊼", e._first ? e._first : ()=>selectFirstListItem(le)));
                e.appendChild( createListControlButton("⋀︎", e._up ? e._up : ()=>selectPrevListItem(le)));
                e.appendChild( createListControlButton("⋁︎", e._down ? e._down : ()=>selectNextListItem(le)));
                e.appendChild( createListControlButton("⊻", e._last ? e._last : ()=>selectLastListItem(le)));
                e.appendChild( createListControlButton("∅", e._clear ? e._clear : ()=>clearSelectedListItem(le))); // alternatives: ⌫ ∅ ⎚
            }
        }
    }
}

function createListControlButton ( text, onClickAction) {
    return Button(text,onClickAction);
}

//--- tooltips

function createTooltip (e, text) {
    let ett = createElement("DIV", "ui_tooltip", text);
    document.body.appendChild(ett);
    e._uiTooltip = ett;

    _addClass( e, "tooltipped");

    e.addEventListener("mouseenter", (event) =>{
        let e = event.target;
        let ett = e._uiTooltip;
        if (ett) {
            let cr = e.getBoundingClientRect(); // client rect
            let tcr = ett.getBoundingClientRect(); // tooltip client rect

            let x = cr.x + cr.width/2 - tcr.width/2; // center on client
            ett.style.left = x + "px";

            let y = cr.y - tcr.height - 8; // try above client
            if (y > 0) {
                _addClass(ett, "above");
            } else {
                _addClass(ett, "below");
                y = cr.y + cr.height + 8;
            }
            ett.style.top = y + "px";

            ett.style.visibility = "visible";

            tcr = ett.getBoundingClientRect();
        }
    });

    e.addEventListener("mouseleave", (event) =>{
        let e = event.target;
        let ett = e._uiTooltip;
        if (ett) {
            ett.style.visibility = "hidden";
            ett.style.left = "-1000px";
        }
    });
}

//--- buttons

export function getButton (o) {
    let e = _elementOf(o);
    if (e) {
        let eCls = e.classList;
        if (eCls.contains("ui_button")) return e;
    }
    return undefined;
}

//--- menus

var _uiActivePopupMenus = [];
window.addEventListener("click", _windowPopupHandler);

export function PopupMenu (eid=null) {
    return function (children) {
        const e = createElement("DIV", "ui_popup_menu");
        if (eid) e.setAttribute("id", eid);

        for (const c of children) e.appendChild(c);

        //initializePopupMenu(e); // done by enclosing Window
        return e;
    }
}

function createMenuItem (text, action=null, eid=null, isChecked=false, isDisabled=false) {
    const e = createElement("DIV", "ui_menuitem");

    if (isDisabled) e.classList.add("disabled");
    if (isChecked) e.classList.add("checked");

    if (action) e.setAttribute("onclick", action);
    if (eid) e.setAttribute("id", eid);
    e.appendChild( document.createTextNode(text)); 

    return e;
}

export function MenuItem (text, action=null, eid=null, isChecked=false, isDisabled=false) {
    return createMenuItem(text, action, eid, isChecked, isDisabled);
}

export function SubMenu (text, action=null, eid=null, isChecked=false, isDisabled=false) {
    return function (...subItems) {
        const e = createMenuItem(text, action, eid, isChecked, isDisabled);
        for (const c of subItems) e.appendChild(c);

        return e;
    }
}


function _initializeMenus() {
    for (let e of document.getElementsByClassName("ui_menuitem")) {
        initializeMenuItem(e);
    }
}

function initializePopupMenu (e) {
    // nothing yet for ourselves
    for (let c of _getChildren(e)) initializeRecursive(c);
}

function initializeMenuItem(e) {
    e.addEventListener("mouseenter", _menuItemHandler);
    e.addEventListener("click", (event) => {
        event.stopPropagation();
        for (let i = _uiActivePopupMenus.length - 1; i >= 0; i--) {
            _uiActivePopupMenus[i].style.visibility = "hidden";
        }
        _uiActivePopupMenus = [];
    });

    if (e.classList.contains("disabled")) {
        e._uiSuspendedOnClickHandler = e.onclick;
        e.onclick = null;
    }
}

function _windowPopupHandler(event) {
    for (let p of _uiActivePopupMenus) {
        p.style.visibility = "hidden";
    }
    _uiActivePopupMenus = [];
}

function _menuItemHandler(event) {
    let mi = event.target;
    let sub = _firstChildWithClass(mi, "ui_popup_menu");
    let currentTop = _peekTop(_uiActivePopupMenus);

    if (currentTop !== mi.parentNode) {
        _uiActivePopupMenus.pop();
        currentTop.style.visibility = "hidden";
    }

    if (sub) {
        let rect = mi.getBoundingClientRect();
        let left = (rect.right + sub.scrollWidth <= window.innerWidth) ? rect.right : rect.left - sub.scrollWidth;
        let top = (rect.top + sub.scrollHeight <= window.innerHeight) ? rect.top : rect.bottom - sub.scrollHeight;

        sub.style.left = left + "px";
        sub.style.top = top + "px";
        sub.style.visibility = "visible";

        _uiActivePopupMenus.push(sub);
    }
}

export function popupMenu(event, o) {
    event.preventDefault();
    let popup = getPopupMenu(o);
    if (popup) {
        let left = _computePopupLeft(event.pageX, popup.scrollWidth);
        let top = _computePopupTop(event.pageY, popup.scrollHeight);

        popup.style.left = left + "px";
        popup.style.top = top + "px";
        popup.style.visibility = "visible";

        _uiActivePopupMenus.push(popup);
    }
}
main.exportFuncToMain(popupMenu);


export function getPopupMenu(o) {
    let e = _elementOf(o);
    if (e) {
        let eCls = e.classList;
        if (eCls.contains("ui_popup_menu")) return e;
        else if (eCls.contains("ui_popup_menu_item")) return e.parentNode;
    }

    throw "not a menu";
}

export function getMenuItem(o) {
    let e = _elementOf(o);
    if (e) {
        let eCls = e.classList;
        if (eCls.contains("ui_menuitem")) return e;
    }
    throw "not a menuitem";
}

export function isMenuItemDisabled(o) {
    let e = getMenuItem(o);
    if (e) {
        return e.classList.contains("disabled");
    }
    throw "not a menuitem";
}

export function setMenuItemDisabled(o, isDisabled = true) {
    let e = getMenuItem(o);
    if (e) {
        if (isDisabled) {
            e.classList.add("disabled");
            e._uiSuspendedOnClickHandler = e.onclick;
            e.onclick = null;
        } else {
            e.classList.remove("disabled");
            e.onclick = e._uiSuspendedOnClickHandler;
        }
    }
}

export function toggleMenuItemCheck(event) {
    let mi = event.target;
    if (mi && mi.classList.contains("ui_menuitem")) {
        return _toggleClass(mi, "checked");
    } else {
        throw "not a menuitem";
    }
}

//--- color 

export function createColorBox(clrSpec) {
    let e = createElement("DIV", "ui_color_box");
    e.style.backgroundColor = clrSpec;
    return e;
}

export function createColorInput(initClr, size, action) {
    let e = document.createElement("input");
    e.type = "color";
    e.setAttribute("value", initClr);
    e.style.width = size;
    e.style.height = size;
    e.style.backgroundColor = initClr;
    //e.onchange = action;
    e.oninput = action;
    return e;
}

//--- progress bars (simple nested DIVs)

export function createProgressBar (percent0=0, percent1=0) {
    let e = createElement("DIV", "ui_progress_bar");
    e._uiP0 = Math.round(percent0);
    e._uiP1 = Math.round(percent1);
    addProgressBarComponents(e);
    return e;
}

function addProgressBarComponents (e) {
    let e0 = createElement("DIV", "ui_progress_0");
    let p0 = e._uiP0;
    e0.style.width = `${e._uiP0}%`;
    e.appendChild(e0);

    let e1 = createElement("DIV", "ui_progress_1");
    e1.style.width = `${e._uiP1}%`;
    e0.appendChild(e1);
}

export function ProgressBar (eid,percent0=0,percent1=0) {
    let e = createProgressBar(percent0,percent1);
    if (eid) e.id = eid;
    return e;
}

function initializeProgressBar(e) {
    if (_hasNoChildElements(e)) {
        addProgressBarComponents(e);
    }
}

export function setProgress2Bar (o, percent0=0, percent1=0) {
    let e = getProgressBar(o);
    if (e) {
        e._uiP0 = Math.round(percent0);
        e._uiP1 = Math.round(percent1);

        let p0 = e.firstElementChild();
        if (p0) {
            p0.style.width = `${e._uiP0}%`;
            let p1 = p0.firstElementChild();
            p1.style.width = `${e._uiP1}%`;
        }
    }
}

export function getProgressBar(o) {
    let e = _elementOf(o);
    if (e && e.tagName == "DIV") {
        if (e.classList.contains("ui_progress_bar")) return e;
        else if (e.classList.contains("ui_progress_0")) return e.parentElement;
        else if (e.classList.contains("ui_progress_1")) return e.parentElement.parentElement;
    }
    throw "not a progress bar";
}

//--- common panels

export function LayerPanel (windowId, showAction, isExpanded=false) {
    return Panel("layer", isExpanded, `${windowId}.layer`)(
        VarText( null, `${windowId}.layer-descr`)
    )
}

//--- images & ImageViewerWindow

export function createImage(src, placeholder, w, h) {
    let e = document.createElement("img");
    e.src = src;
    if (placeholder) e.alt = placeholder;
    if (w) e.width = w;
    if (h) e.height = h;
    return e;
}


//--- spacers

export function HorizontalSpacer (minWidthInRem=2, maxWidthInRem=undefined) {
    let e = createElement("DIV", "spacer");
    e.style.minWidth = minWidthInRem + "rem";
    if (maxWidthInRem) {
        e.style.maxWidth = maxWidthInRem + "rem";
    }
    return e;
}

//--- general event processing

export function stopAllOtherProcessing(event) {
    event.stopImmediatePropagation();
    event.stopPropagation();
    event.preventDefault();
}

export function isShowing(e) {
    if ((e.offsetParent === null) /*|| (e.getClientRects().length == 0)*/ ) return false; // shortcut
    let style = window.getComputedStyle(e);
    if (style.visibility !== 'visible' || style.display === 'none') return false;

    e = e.parentElement;
    while (e) {
        style = window.getComputedStyle(e);
        if (style.visibility !== 'visible' || style.display === 'none') return false;

        // we could also check for style.maxHeight == 0
        if (e.classList.contains('collapsed')) return false;
        e = e.parentElement;
    }

    return true;
}

//--- general utility functions

function _elementOf(o) {
    if (typeof o === 'string' || o instanceof String) {
        return document.getElementById(o);
    } else if (o instanceof HTMLElement) {
        return o;
    } else {
        let tgt = o.target;
        if (tgt && tgt instanceof HTMLElement) return tgt;

        return undefined;
    }
}

function _swapClass(element, oldCls, newCls) {
    element.classList.remove(oldCls);
    element.classList.add(newCls);
}

function _toggleClass(element, cls) {
    if (element.classList.contains(cls)) {
        element.classList.remove(cls);
        return false;
    } else {
        element.classList.add(cls);
        return true;
    }
}

function _addClass(element, cls) {
    if (!element.classList.contains(cls)) element.classList.add(cls);
}

function _removeClass(element, cls) {
    element.classList.remove(cls);
}

function _containsClass(element, cls) {
    return element.classList.contains(cls);
}

function _getUiClass (e) {
    for (let c of e.classList) {
        if (c.startsWith("ui_")) return c;
    }
    return null;
}

function _containsAnyClass(element, ...cls) {
    let cl = element.classList;
    for (c of cls) {
        if (cl.contains(c)) return true;
    }
    return false;
}

function setStyle (e, opts) {
    if (opts.color) e.style.color = opts.color;
    if (opts.background) e.style.background = opts.background;
    if (opts.border) e.style.border = opts.border;
    if (opts.left) e.style.left = _numStyleValue( opts.left, "px");
    if (opts.top) e.style.top = _numStyleValue( opts.top, "px");
    if (opts.right) e.style.right = _numStyleValue( opts.right, "px");
    if (opts.bottom) e.style.bottom = _numStyleValue( opts.bottom, "px");
    if (opts.width) e.style.width = _numStyleValue(opts.width, "px");
    if (opts.height) e.style.height = _numStyleValue(opts.height, "px");
}

function _numStyleValue (v, suffix) {
    if (typeof v === 'number') return v +suffix;
    else return v;
}

function setWidthStyle (e, minWidth=null, maxWidth=null) {
    let style = "";
    if (maxWidth) style += `max-width:${maxWidth};`;
    if (minWidth) style += `min-width:${minWidth};`;
    if (style.length > 0) e.setAttribute("style", style);
}

function _setAlignment(e, attrs) {
    if (attrs.includes("alignLeft")) {
        _addClass(e, "align_left");
    } else if (attrs.includes("alignRight")) {
        _addClass(e, "align_right");
    }
}

export function getRootVar(varName, defaultValue = undefined) {
    let v = getComputedStyle(document.documentElement).getPropertyValue(varName);
    if (v) return v;
    else return defaultValue;
}

function _rootVarInt(varName, defaultValue = 0) {
    let v = getComputedStyle(document.documentElement).getPropertyValue(varName);
    if (v) {
        return parseInt(v, 10);
    } else {
        return defaultValue;
    }
}

function _rootVarFloat(varName, defaultValue = 0.0) {
    let v = getComputedStyle(document.documentElement).getPropertyValue(varName);
    if (v) {
        return parseFloat(v);
    } else {
        return defaultValue;
    }
}

function _dataAttrValue(element, varName, defaultValue = "") {
    let data = element.dataset;
    if (data) {
        let v = data[varName];
        if (v) return v;
    }
    return defaultValue;
}

function _intDataAttrValue(element, varName, defaultValue = 0) {
    let data = element.dataset;
    if (data) {
        let v = data[varName];
        if (v) return parseInt(v);
    }
    return defaultValue;
}

function _nearestElementWithClass(e, cls) {
    while (e) {
        if (e.classList.contains(cls)) return e;
        e = e.parentElement;
    }
    return undefined;
}

export function nearestParentWithClass(e, cls) {
    return _nearestElementWithClass(e.parentElement, cls);
}

function _getChildren(e){
    let list = Array(e.childElementCount);
    for (let i=0; i<e.childElementCount; i++) list[i] = e.children[i];
    return list;
}

function _getDocumentElementsByClassName(cls) {
    let res = document.getElementsByClassName(cls);
    let list = Array(res.length);
    for (let i=0; i<res.length; i++) list[i] = res.item(i);
    return list;
}

function _childIndexOf(element) {
    var i = -1;
    var e = element;
    while (e != null) {
        e = e.previousElementSibling;
        i++;
    }
    return i;
}

function _firstChildWithClass(element, cls) {
    var c = element.firstChild;
    while (c) {
        if (c instanceof HTMLElement && c.classList.contains(cls)) {
            return c;
        }
        c = c.nextElementSibling;
    }
    return undefined;
}

function _findChildWithId (e, id) {
    if (e.id == id) return e;

    var ce = e.firstChild;
    while (ce) {
        let ee = _findChildWithId(ce, id);
        if (ee) return ee;

        ce = ce.nextElementSibling;
    }

    return null;
}

function _nthChildOf(element, n) {
    if (n >= element.childNodes.length) return null;

    var i = 0;
    var c = element.firstChild;
    while (c) {
        if (c instanceof HTMLElement) {
            if (i == n) return c;
            i++;
        }
        c = c.nextElementSibling;
    }
    return null;
}

function indexOfElement (e) {
    let idx = 0;
    while (e.previousElementSibling) {
        idx++;
        e = e.previousElementSibling;
    }

    return idx;
}

function _removeLastNchildrenOf(element, n) {
    let i = 0;
    while (i < n && element.firstChild) {
        let le = element.lastElementChild;
        element.removeChild(le);
        i++;
    }
}

function _removeChildrenOf(element, keepFilter = undefined) {
    let keepers = [];

    while (element.firstElementChild) {
        let le = element.lastElementChild;
        if (keepFilter && keepFilter(le)) keepers.push(le);
        element.removeChild(le);
    }

    if (keepers.length > 0) {
        for (e of keepers.reverse()) {
            element.appendChild(e);
        }
    }
}

function _isDisabled(e) {
    return _containsClass(e, "disabled");
}

function _setDisabled(e, isDisabled) {
    if (isDisabled) _addClass(e, "disabled");
    else _removeClass(e, "disabled");
}


// FIXME - this assumes the popup dimensions are much smaller than the window dimensions

function _computePopupLeft(pageX, w) {
    return (pageX + w > window.innerWidth) ? pageX - w : pageX;
}

function _computePopupTop(pageY, h) {
    return (pageY + h > window.innerHeight) ? pageY - h : pageY;
}

function _peekTop(array) {
    let len = array.length;
    return (len > 0) ? array[len - 1] : undefined;
}

function _hasNoChildElements(element) {
    return element.children.length == 0;
}

export function createElement(tagName, clsList = undefined, txtContent = undefined) {
    let e = document.createElement(tagName);
    if (clsList) e.classList = clsList;
    if (txtContent) e.innerText = txtContent;
    return e;
}

function _moveChildElements(oldParent, newParent) {
    while (oldParent.firstElementChild){
        newParent.appendChild(oldParent.firstElementChild);
    } 
}

function _hoistChildElement(e) {
    let parent = e.parentElement;
    if (parent.nextElementSibling) {
        parent.parentElement.insertBefore(e, parent.nextElementSibling);
    } else {
        parent.parentElement.appendChild(e);
    }
}

function _createDate(dateSpec) {
    if (dateSpec) {
        if (typeof dateSpec === "string") {
            if (dateSpec == "now") return new Date(Date.now());
            else return new Date(Date.parse(dateSpec));
        } else if (typeof dateSpec === "number") {
            return new Date(dateSpec); // epoch millis
        }
    }
    return undefined;
}

function _hasBorderedParent(e) {
    let p = e.parentElement;
    if (p) {
        let cl = p.classList;
        return (cl.contains('ui_container') && cl.contains('bordered'));
    }
    return false;
}

function _parentDataSet(e) {
    let p = e.parentElement;
    return (p) ? p.dataset : undefined;
    _
}

function _inheritableData(e, key) {
    do {
        let v = e.dataset[key];
        if (v) return v;
        e = e.parentElement;
    } while (e);
}

function _parseInt(s, defaultValue) {
    if (s && s.length > 0) return parseInt(s);
    else return defaultValue;
}

function _parseNumber(s, defaultValue) {
    if (s && s.length > 0) {
        if (s.contains('.')) return parseFloat(s);
        else return parseInt(s);
    } else {
        return defaultValue;
    }
}

function _hasDimensions(e) {
    return (e.getBoundingClientRect().width > 0);
}

function _formattedNum(v, fmt) {
    return fmt ? fmt.format(v) : v.toString();
}

function _consumeEvent(event) {
    event.preventDefault();
    event.stopPropagation();
}

export function positionRight(e, len) {
    e.style.position = 'absolute';
    e.style.right = len;
    e.style.display = 'inline-block';
}

function getItemLabel (item) {
    if (item.label) return util.evalProperty(item.label);
    else if (item.name) return util.evalProperty(item.name);
    else if (item.id) return util.evalProperty(item.id);
    else return item.toString();
}

export function setElementColors (e, clr, backClr) {
    if (e) {
        e.style.color = clr;
        e.style.backgroundColor = backClr;
    }
}

export function resetElementColors(e) {
    if (e) {
        e.style.color = null;
        e.style.backgroundColor = null;
    }
}

export function initElem (e, initFunc) {
    if (e) {
        initFunc(e);
        return e;
    }
}