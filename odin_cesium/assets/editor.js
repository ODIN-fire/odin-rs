/**
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

/// module with CesiumJS and odin_server UI functions for interactive editing of
/// points, lines, polylines / polygons, rects and circles

import { config } from "./editor_config.js";
import * as main from "../odin_server/main.js";
import * as util from "../odin_server/ui_util.js";
import * as ui from "../odin_server/ui.js";
import * as cesium from "./odin_cesium.js";

const viewer = cesium.viewer;
const ellipsoid = Cesium.Ellipsoid.WGS84;

/* #region test *****************************************************************************/

/*
/// cesium_test driver
main.exportFuncToMain( function test() {
    function processResult (res) {
        console.log("edit result:", res);
    }

    let points = [
        { lon: -122.0, lat: 40.0 },
        { lon: -119.0, lat: 38.0 },
        { lon: -120.5, lat: 37.0 }
    ];
    //points = [];

    console.log("start editing");

    //cesium.enterGeoPoint(processResult);
    //cesium.enterGeoLine(processResult);
    //cesium.enterGeoLineString(processResult);
    //cesium.enterGeoPolygon(processResult);
    //cesium.enterGeoRect(processResult);
    //cesium.enterGeoCircle(processResult)

    
    //let editor = main.getDefaultShareEditorForItemType( main.GEO_LINE_STRING);
    //if (editor) {
    //    let input = new main.GeoLineString( points);
    //    editor( input, processResult);
    //} else console.log("no editor");
    
    //let editor = main.getDefaultShareEditorForItemType( main.GEO_POLYGON);
    //if (editor) {
    //    let input = new main.GeoPolygon( points);
    //    editor( input, processResult);
    //} else console.log("no editor");

    //let editor = main.getDefaultShareEditorForItemType( main.GEO_LINE);
    //if (editor) {
    //    editor( null, processResult);
    //} else console.log("no editor");

    //let editor = editGeoPoint;
    //editor( null, processResult);
    
    //let editor = editGeoRect;
    //editor( null, processResult);

    let editor = editGeoCircle;
    editor( null, processResult);
});
*/

/* #endregion test */

/* #region editor Window classes ************************************************************/

/// the UI window components & layout
/// this is guaranteed to have {window, pointList, lonField, latField, altField, legField, totalField} properties
/// subclasses might add more
class PolyEditorWindow {
    constructor (title, enter, cancel, setPoint, insPoint, delPoint, selPoint, setMetric) {
        let fieldOpts = { alignRight: true, isFixed: true, placeHolder: "0.0" };
        let fieldAttrs = ["fixed","alignRight"];
    
        this.window = ui.Window( title, "editor", "./asset/odin_cesium/editor.svg")(
            (this.pointList = ui.List("editor.points", 8, selPoint)),
            ui.RowContainer()(
                (this.lonField = ui.TextInput("", "editor.lon", "6.5rem", fieldOpts)),
                (this.latField = ui.TextInput("", "editor.lat", "6.4rem", fieldOpts)),
                (this.altField = ui.TextInput("", "editor.alt", "6.4rem", fieldOpts)),
                ui.Button("set", setPoint),
                ui.Button("del", delPoint),
            ),
            this._statsContainer(),
            ui.RowContainer("center")(
                (this.metricCb = ui.CheckBox("metric", setMetric, null, cesium.isMetric)),
                ui.HorizontalSpacer(3),
                ui.Button("cancel", cancel),
                ui.Button("save", enter)
            )
        );
        
        ui.setListItemDisplayColumns( this.pointList, ["fit", "header"], [
            { name: "idx", tip: "point index", width: "2rem", attrs: fieldAttrs, map: p => p.idx },
            { name: "lon", tip: "longitude [deg]", width: "7rem", attrs: fieldAttrs, map: p => util.formatFloat(p.lon,5) },
            { name: "lat", tip: "latitude [deg]", width: "6.5rem", attrs: fieldAttrs, map: p => util.formatFloat(p.lat,5) },
            { name: "alt", tip: "altitude [m,ft]", width: "5rem", attrs: fieldAttrs, map: p => this._altDisplay(p.alt) },
            { name: "dist", tip: "distance [km,mi]", width: "6rem", attrs: fieldAttrs, map: p => this._distDisplay(p.dist) }
        ]);    
    }
  
    _altDisplay (alt) {
        if (!this.isMetric()) alt = util.metersToFeet(alt);
        return util.formatGroupedFloat( alt, 0);
    }

    _distDisplay (dist) {
        dist = this.isMetric() ? dist / 1000 : util.metersToUsMiles(dist);
        return util.formatGroupedFloat( dist, 2);
    }

    // override if there are more stats
    _statsContainer() {
        return ui.RowContainer("start")(
            (this.legField = ui.TextInput("leg dist", "editor.leg", "6rem", {isDisabled: true, isFixed: true, alignRight: true} )),
            (this.totalField = ui.TextInput("total dist", "editor.total", "6rem", {isDisabled: true, isFixed: true, alignRight: true} ))
        );
    } 

    openAt (x, y) {
        ui.addWindow( this.window);
        ui.placeWindow( this.window, x, y);
        ui.setWindowSpotlight( this.window, true);
        ui.showWindow( this.window);
    }
  
    close () {
        ui.closeWindow( this.window);
    }

    getPointFromFields() {
        let lat = Number.parseFloat( ui.getFieldValue( this.latField));
        if (!util.checkLat(lat)) { alert("missing of invalid latitude degrees"); return null; }
    
        let lon = Number.parseFloat( ui.getFieldValue( this.lonField));
        if (!util.checkLon(lon)) { alert("missing of invalid longitude degrees"); return null; }
    
        let v = ui.getNonEmptyFieldValue( this.altField);
        let alt = v ? Number.parseFloat( v) : 0.0;
        if (Number.isNaN(alt)) {
            alt = 0.0;
        } else {
            if (alt < 0) { alert("invalid altitude (>0)", alt); return null; }
            if (!this.isMetric()) alt = util.feetToMeters(alt);
        }
        
        let p = new Cesium.Cartesian3.fromDegrees( lon, lat, alt);
        p.lon = lon;  p.lat = lat;  p.alt = alt;
    
        return p;
    }

    getSelectedPoint() {
        return ui.getSelectedListItem( this.pointList);
    }

    setPoints (points) {
        ui.setListItems( this.pointList, points);
    }

    removePointIndex (idx) {
        ui.removeListItemIndex( this.pointList, idx);
    }

    isMetric() {
        return ui.isCheckBoxSelected(this.metricCb);
    }
}

class PolygonEditorWindow extends PolyEditorWindow {
    constructor (title, enter, cancel, setPoint, insPoint, delPoint, selPoint, setMetric) {
        super(title, enter, cancel, setPoint, insPoint, delPoint, selPoint, setMetric);
    }

    _statsContainer() {
        return ui.RowContainer("start")(
            (this.legField = ui.TextInput("leg dist", "editor.leg", "5rem", {isDisabled: true, isFixed: true, alignRight: true} )),
            (this.totalField = ui.TextInput("total dist", "editor.total", "6rem", {isDisabled: true, isFixed: true, alignRight: true} )),
            (this.areaField = ui.TextInput("area", "editor.area", "8rem", {isDisabled: true, isFixed: true, alignRight: true} ))
        );
    }
}

/* #endregion editor window classes */

/* #region editor classes *******************************************************************/


// initial window position (updated upon close)
var xLeft = 100;
var yTop = 100;

export function editPolyline (points, processResult) {
    new PolyEditor( "Edit Polyline", points, processResult).open();
}


/// shared item editor func for GeoLine
export function editGeoPoint (geoPoint, processResult) {
    function procRes (editedPoints) {
        if (geoPoint) {
            processResult( geoPoint.toRounded());
        } else {
            processResult( main.GeoPoint.fromRoundedLonLatDegrees(editedPoints[0].lon, editedPoints[0].lat));
        }
    }

    let init = geoPoint ? [Object.assign({}, geoPoint)] : [];
    new PointEditor( "Edit GeoPoint", init, procRes).open();
}
main.addShareEditor( main.GEO_POINT, "edit 2D point", editGeoPoint);


/// shared item editor func for GeoLine
export function editGeoLine (geoLine, processResult) {
    function procRes (editedPoints) {
        let resultPoints = editedPoints.map( (p)=> main.GeoPoint.fromRoundedLonLatDegrees( p.lon, p.lat) );
        if (geoLine) {
            geoLine.start = resultPoints[0];
            geoLine.end = resultPoints[1];
            processResult( geoLine);
        } else {
            processResult( new main.GeoLine(resultPoints[0], resultPoints[1]));
        }
    }

    let init = geoLine ? [Object.assign({}, geoLine.start), Object.assign({}, geoLine.end)] : [];
    new LineEditor( "Edit GeoLine", init, procRes).open();
}
main.addShareEditor( main.GEO_LINE, "2D line", editGeoLine);

/// shared item editor func for GeoLineString
export function editGeoLineString (geoLineString, processResult) {
    function procRes (editedPoints) {
        let resultPoints = editedPoints.map( (p)=> main.GeoPoint.fromRoundedLonLatDegrees( p.lon, p.lat) );
        if (geoLineString) {
            geoLineString.points = resultPoints;
            processResult( geoLineString);
        } else {
            processResult( new main.GeoLineString(resultPoints));
        }
    }

    let init = geoLineString ? geoLineString.points.map( (p)=>Object.assign({}, p) ) : [];
    new PolyEditor( "Edit GeoLineString", init, procRes).open();
}
main.addShareEditor( main.GEO_LINE_STRING, "2D polyline", editGeoLineString);

export function editPolygon (points, processResult) {
    new PolygonEditor( points, processResult).open();
}

/// shared item editor func for GeoPolygon
export function editGeoPolygonExterior (geoPolygon, processResult) {
    function procRes (editedPoints) {
        let resultPoints = editedPoints.map( (p)=> main.GeoPoint.fromRoundedLonLatDegrees( p.lon, p.lat) );
        if (geoPolygon) {
            geoPolygon.exterior = resultPoints;
            processResult( geoPolygon);
        } else {
            processResult( new main.GeoPolygon(resultPoints));
        }
    }

    let init = geoPolygon ? geoPolygon.exterior.map( (p)=>Object.assign({}, p)) : [];
    new PolygonEditor( "Edit GeoPolygon", init, procRes).open();
}
main.addShareEditor( main.GEO_POLYGON, "2D polygon exterior", editGeoPolygonExterior);


/// shared item editor func for GeoRect
export function editGeoRect (geoRect, processResult) {
    function procRes (rect) {
        let geoRect = new main.GeoRect( 
            util.roundToDecimals( rect.west, 5),
            util.roundToDecimals( rect.south, 5),
            util.roundToDecimals( rect.east, 5),
            util.roundToDecimals( rect.north, 5)
        );
        processResult( geoRect);
    }

    new RectEditor( "Edit GeoRect", geoRect, true, procRes).open();
}
main.addShareEditor( main.GEO_RECT, "2D rectangle", editGeoRect);




/// the base class for PolylineEditor and PolygonEditor
export class PolyEditor {

    constructor (title, points, processResult) {
        this.cp = new Cesium.Cartesian3(); // cache so that we don't need to allocate on each mouseMove
        this.isMetric = cesium.isMetric;
    
        this.title = title;
        this.points = points;
        this.processResult = processResult;
    
        this.minPoints = 0;
        this.maxPoints = 0;

        this.editor = this._createEditor();

        this.polyEntity = undefined;
        this.handles = []; // will hold entities
        this.halfHandles = [];

        this.selHandle = undefined;
        this.restorePos = undefined;

        this.cancelEntry = null;

        //--- bind event handler methods to this (we need to be able to remove them)
        this.onHandleClick = this._onHandleClick.bind(this);
        this.onHandleMove = this._onHandleMove.bind(this);
        this.onHandleKey = this._onHandleKey.bind(this);
        this.onEditorKey = this._onEditorKey.bind(this);
    }

    open () {
        this.editor.openAt( xLeft, yTop);
    
        cesium.setRequestRenderMode(false);

        if (this.points.length) { // no entry mode - create assets and go straight to edit mode
            this._setPointAttributes(); // fill in lon/lat/alt or x/y/z, dist and idx
            this._createAssets();
            cesium.registerMouseClickHandler( this.onHandleClick);
            cesium.registerKeyDownHandler( this.onEditorKey);
            this._setPointList();
      
        } else {
            this._startEntryMode();
        }
    }

    _createEditor () {
        return new PolyEditorWindow( 
            this.title,
            this._enter.bind(this), 
            this._cancel.bind(this), 
            this._setFieldPoint.bind(this), 
            this._insFieldPoint.bind(this), 
            this._delSelectedPoint.bind(this), 
            this._pointSelected.bind(this), 
            this._setMetric.bind(this)
        );
    }

    _startEntryMode () {
        this.cancelEntry = cesium.enterPolyline( this.points, this.maxPoints, {  // @override for polygon
            onEnter: this._onEntryComplete.bind(this),
            onCancel: this._onEntryCancel.bind(this), 
            onAddPoint: this._addHandle.bind(this), 
            onDelPoint: this._delHandle.bind(this),
            onMouseMove: this._updateMovingPoint.bind(this)
        });
    }

    _cancel() {
        this._dispose();
    }
      
    _enter() {
        if (this.points.length >= this.minPoints) {
            if (this.processResult) this.processResult(this.points);
        }
        this._dispose();
    }

    // key handler for the editor window - only active while we are not in entry- or handle-edit mode
    _onEditorKey (event) {
        if (event.code == "Escape") { // cancel/close the window
            this._cancel();
        } else if (event.code == "Enter") { // save/close the window
            this._enter();
        }
    }

    _dispose() {
        let viewportOffset = this.editor.window.getBoundingClientRect();
        xLeft = viewportOffset.left;
        yTop = viewportOffset.top;

        if (this.cancelEntry) {
            this.cancelEntry();
            this.cancelEntry = null;
        }

        this._releaseAssets();
        this.editor.close();

        cesium.setDefaultCursor();
        cesium.setRequestRenderMode(true);
        cesium.requestRender();
    }

    _isInEntryMode() {
        return (this.cancelEntry != null);
    }

    _setFieldPoint (){
        let fp = this.editor.getPointFromFields();
        if (fp) {
            let points = this.points;
            let idx = ui.getSelectedListItemIndex( this.editor.pointList);
            let p;

            if (idx >= 0) { // update selected item
                p = points[idx];
                Object.assign( p, fp);
                p.dist = idx > 0 ? util.gcDistanceBetweenECEF( points[idx-1], p) : 0;
        
                this._pointMoved( idx, p);
        
                if (idx < points.length-1) {
                    points[idx + 1].dist = util.gcDistanceBetweenECEF( p, points[idx+1]);
                }
        
                let e = this.handles.find( (e)=> e.__index == idx); // it has to be there
                e.position = p;

            } else { // no selection - append new point 
                idx = points.length;
                fp.idx = idx;
        
                if (!this.cancelEntry) { // no mover, add new point
                    p = fp;
                    points.push(p);
            
                } else { // in edit mode 
                    if (points.length > 0) {
                        p = points[points.length-1];
                        p.x = fp.x;  p.y = fp.y;  p.z = fp.z; 
                        p.lon = fp.lon; p.lat = fp.lat;
                    } else {
                        p = fp;
                    }
                    
                    let pMover = { ...p};
                    pMover.dist = 0;
                    points.push(pMover);
                }
        
                p.dist = idx > 0 ? util.gcDistanceBetweenECEF( points[idx-1], p) : 0;
                this._addHandle(p);
    
            }
    
            this._setHalfPointHandles();
            this._setLegField(p.dist);
        }
    
        this._setStatsFields();
    }

    _insFieldPoint (){
        // TODO
    }

    _delSelectedPoint (){
        let p = this.editor.getSelectedPoint();
        if (p) {
            //let idx = this.points.indexOf(p);
            let idx = p.idx;

            if (idx == this.points.length-1) {
                this.points.pop();
            } else {
                this.points.splice( idx, 1);
                this._updatePointListFrom( idx);
                this._setHalfPointHandles();
            }
            this._removeHandleIndex( idx);

            this.editor.removePointIndex(idx);
            //this.editor.setPoints( this.points);

        }
    }

    _setMetric (event) {
        let cb = ui.getCheckBox(event.target);
        if (cb) {
            this.isMetric = ui.isCheckBoxSelected(cb);

            ui.updateListItems(this.editor.pointList);
            let p = ui.getSelectedListItem(this.editor.pointList);
            if (p) {
                this._setLegFields(p.dist);
                this._setAltField( p.alt);
            }
            this._setStatsFields();
        }
    }

    /// callback when point is selected from editor.pointList
    _pointSelected (event){
        let editor = this.editor;
        let p = event.detail.curSelection;

        if (p) {
            ui.setField( editor.lonField, util.formatFloat( p.lon, 5));
            ui.setField( editor.latField, util.formatFloat( p.lat, 5));

            this._setAltField(p.alt);
            this._setLegField(p.dist);
    
        } else {    
            ui.setField( editor.lonField, null);
            ui.setField( editor.latField, null);
            ui.setField( editor.altField, null);
            ui.setField( editor.legField, null);
        }
    }

      // mouse click handler for poly handles
    _onHandleClick (event) {
        let p = cesium.getWindowMousePosition(event);
        let points = this.points;

        if (this.selHandle) { // end tracking
            cesium.releaseMouseMoveHandler( this.onHandleMove);
            cesium.releaseKeyDownHandler( this.onHandleKey);
            cesium.registerKeyDownHandler( this.onEditorKey);

            cesium.setDefaultCursor();
            this.selHandle.color = config.handleColor;

            let idx = this.selHandle.__index;
            this._pointMoved( idx, points[idx]);

            this.selHandle = null;
            this._setHalfPointHandles();

        } else { // start tracking mouse
            let picked = viewer.scene.pick(p);

            if (picked && picked.id && picked.id.__index !== undefined) {
                cesium.showSelectionIndicator(false);
                let e = picked.id;
                let idx = e.__index + 1;

                if (e.__isHalf) { // selected a half handle -> promote to full handle and set it selected
                    let pos = e.position.getValue();
                    points.splice(idx, 0, pos);

                    let handles = this.handles;
                    for (let eOther of handles) {
                        if (eOther.__index >= idx) eOther.__index += 1;
                    }
                    let eFull = this._createHandle( pos);
                    handles.push(eFull);
                    viewer.entities.add(eFull);
                    eFull.__index = idx;
                    this.selHandle = eFull;

                    this._pointAdded( idx, points[idx]);
                    this._pointEditing( idx, points[idx]);

                    this.restorePos = {...points[idx]};

                } else { // selected a full handle
                    this.selHandle = e;
                    this.restorePos = { ...points[idx-1] };
                    this._pointEditing( idx, points[idx-1]);
                }

                this._removeHandleEntities(this.halfHandles); // we don't want to move them
                this.selHandle.color = config.selectedHandleColor;
                cesium.setCursor("crosshair");

                cesium.releaseKeyDownHandler( this.onEditorKey);
                cesium.registerMouseMoveHandler( this.onHandleMove);
                cesium.registerKeyDownHandler( this.onHandleKey);
            }
        }
    }

    _createHandle (pos) {
        return new Cesium.Entity( {
            position: pos,
            point: handleOpts(),
            selectable: false
        });
    }

    _createHalfHandle (pos) {
        return new Cesium.Entity( {
            position: pos,
            point: halfHandleOpts(),
            selectable: false
        });
    }

    _removeHandleEntities(handles) {
        for (let e of handles) viewer.entities.remove(e);
        handles.splice(0, handles.length);
    }

    // mouse move handler for poly handles
    _onHandleMove (event) { 
        if (this.selHandle) {
            let cp = this.cp;
            cesium.getCartesian3MousePosition( event, cp);
            this.selHandle.position = cp;

            let idx = this.selHandle.__index;

            let p = this.points[idx];
            p.x = cp.x;  p.y = cp.y;  p.z = cp.z;

            this._updateMovingPoint( this.points, idx);
        }
    }

      // keyDown handler for selected poly handles
    _onHandleKey (event){
        let selHandle = this.selHandle;
        let points = this.points;

        if (selHandle) {
            let idx = selHandle.__index;

            if (event.code == "Delete" || event.code == "Backspace") {
                if (points.length == this._minPoints()) { // cancel the edit - not enough points left
                    this._resetSelHandle();
                    this._releaseAssets();
                    this._cancel();

                } else {
                    let p = points[idx];
                    points.splice(idx, 1);
                    this._removeHandleIndex(idx);
                    this._pointDeleted( idx, p);
                }

            } else if (event.code == "Escape") {
                let rp = this.restorePos;
                if (rp) {
                    let p = points[idx];
                    p.x = rp.x;  p.y = rp.y;  p.z = rp.z;

                    selHandle.position = rp;

                    this.restorePos = undefined;
                    this._resetSelHandle();
                    this._pointMoved( idx, p);
                }
            }
        }
    }

    _removeHandleIndex (idx) {
        let handles = this.handles;
        if (idx >= 0 && idx < handles.length) {
            let h = handles[idx];
            for (let e of handles) { // adjust handle indices
                if (e.__index > idx) { e.__index -= 1 }
            }
            handles.splice(idx, 1);
            viewer.entities.remove(h);
            this._resetSelHandle();
        }
    }

    _resetSelHandle() {
        if (this.selHandle) {
            this.selHandle.point.color = config.handleColor;
            this.selHandle = undefined;
        
            cesium.setDefaultCursor();
            cesium.releaseKeyDownHandler( this.onHandleKey);
            cesium.releaseMouseMoveHandler( this.onHandleMove);
            this._setHalfPointHandles();
        }
    }

    /// notification this point was added
    _pointAdded (idx, p) { 
        setCartographicPos(p);
        p.dist = (idx > 0) ? util.gcDistanceBetweenECEF( this.points[idx-1], p) : 0;
    
        if (idx == this.points.length-1) {
            ui.appendListItem( this.editor.pointList, p);
        } else {
            ui.insertListItem( this.editor.pointList, p, idx);
            this._updatePointListFrom( idx);
        }
    }

    /// notification this point is going to be moved
    _pointEditing (idx, p){ // notification this point is going to be moved
        ui.setSelectedListItem( this.editor.pointList, p);
        this._setLegField(p.dist);
    }

    _setLegField(dist) {
        if (!this.isMetric) {
            dist = util.metersToUsMiles(dist);
            ui.setField( this.editor.legField, util.formatGroupedFloat( dist, 2));
        } else {
            ui.setField( this.editor.legField, util.formatGroupedFloat( dist, 0));
        }
    }

    _setStatsFields() {
        this._setTotalField( this._totalDist());
    }

    _setTotalField(dist) {
        if (!this.isMetric) {
            dist = util.metersToUsMiles(dist);
            ui.setField( this.editor.totalField, util.formatGroupedFloat( dist, 2));
        } else {
            ui.setField( this.editor.totalField, util.formatGroupedFloat( dist, 0));
        }
    }

    _setAltField(alt) {
        if (!this.isMetric) {
            alt = util.metersToFeet(alt);
        }
        ui.setField( this.editor.altField, util.formatGroupedFloat(alt, 0));
    }

    /// notification this point was moved
    _pointMoved (idx, p) { 
        setCartographicPos( p);
    
        p.dist = (idx > 0) ? util.gcDistanceBetweenECEF( this.points[idx-1], p) : 0;
        ui.updateListItem( this.editor.pointList, p);
    
        if (idx < this.points.length-1) {
            let pNext = this.points[idx + 1];
            pNext.dist = util.gcDistanceBetweenECEF( p, pNext);
            ui.updateListItem( this.editor.pointList, pNext);
        }
    
        ui.clearSelectedListItem( this.editor.pointList);
        this._setStatsFields();
    }

    /// notification this point was deleted
    _pointDeleted (idx, p){ 
        if (this.points.length < this._minPoints()) { // not enough points left
            this._cancel();

        } else {
            ui.removeListItem( this.editor.pointList, p);
            this._updatePointListFrom( idx);
            ui.setField( this.editor.legField, null);
            this._setStatsFields();
        }
    }

    /// notification this point is moving with mouse (in flight)
    _updateMovingPoint (points, idx) {
        let p = points[idx];
        setCartographicPos(p);
    
        ui.setField( this.editor.lonField, util.formatFloat( p.lon, 5));
        ui.setField( this.editor.latField, util.formatFloat( p.lat, 5));
    
        if (idx > 0) {
            let prev = this.points[idx-1];
        
            let dist = util.gcDistanceBetweenECEF( prev, p);
            p.dist = dist;
            this._setLegField(dist);
            this._setStatsFields();
        }
    }

    _minPoints() { 
        return 2; 
    }

    _totalDist() {
        let dist = 0;
        let points = this.points;
        for (let i=0; i<points.length; i++) {
            dist += points[i].dist;
        }
        return dist;
    }

    _updatePointListFrom  (idx) {
        let points = this.points;
        let maxIdx = this._maxHandleIndex();
        for (var i=idx; i<=maxIdx; i++) {
            points[i].idx = i;
            ui.updateListItem( this.editor.pointList, points[i]);
        }
    }

    _addHandle (p) {
        let handles = this.handles;
        let idx = handles.length;
        let e = this._createHandle(p);
        handles.push(e);
        viewer.entities.add(e);
        e.__index = idx;
        p.idx = idx;
    
        if (!this.polyEntity) {
            this.polyEntity = this._createPolyEntity();
            viewer.entities.add( this.polyEntity);
        }
    
        this._pointAdded( idx, p);
    }

    _delHandle (idx, p) {
        for (let e of this.handles) {
            if (e.__index == idx) {
                this.handles.splice(idx, 1);
                viewer.entities.remove(e);
                this._pointDeleted( idx, p);
                return;
            }
        }
    }

    /* #region asset management **************************************************************/

    _setPointAttributes () {
        let prev = null;

        let points = this.points;
        for (let i=0; i<points.length; i++) {
            let p = points[i];

            if (p.x && p.lon === undefined) { // cartesian - set lon,lat
                let cp = Cesium.Cartographic.fromCartesian( p, Cesium.Ellipsoid.default);
                p.lon = Math.toDegrees( cp.longitude);
                p.lat = Math.toDegrees( cp.latitude);

            } else if (p.lon) { // geo - set x,y,z
                if (!p.alt) p.alt = 0.0;
                let cp = Cesium.Cartesian3.fromDegrees( p.lon, p.lat, p.alt);
                p.x = cp.x;  p.y = cp.y;  p.z = cp.z;
            }
            p.dist = prev ?  util.gcDistanceBetweenECEF( prev, p) : 0.0;
            p.idx = i;

            prev = p;
        }
    }

    // we already have points - create the entity and the handles from them
    _createAssets() {
        this.polyEntity = this._createPolyEntity();
        viewer.entities.add( this.polyEntity);

        this._setPointHandles();
        this._setHalfPointHandles();
    }

    _createPolyEntity () {
        return new Cesium.Entity( {
            polyline: polylineOpts( this.points), 
            selectable: false
        } );
    }

    _setPointHandles () {
        let points = this.points;
        for (let i=0; i<=this._maxHandleIndex(); i++) {
            let p = points[i];
            let e = this._createHandle( p);
            this.handles.push(e);
            viewer.entities.add(e);
            e.__index = i;
        }
    }

    _setHalfPointHandles () {
        this._removeHandleEntities( this.halfHandles);
    
        if (!this.maxPoints || this.points.length < this.maxPoints) { // otherwise we can't create more points
            let halfPoints = getHalfPoints( this.points);
        
            for (let i = 0; i < halfPoints.length; i++){
                let e = this._createHalfHandle( halfPoints[i]);
                this.halfHandles.push(e);
                viewer.entities.add(e);
                e.__isHalf = true;
                e.__index = i;
            }
        }
    }

    _setPointList () {
        let points = this.points;
        let maxIdx = this._maxHandleIndex();
        for (let i=0; i<= maxIdx; i++) {
            ui.appendListItem( this.editor.pointList, points[i]);
        }
    }

    _maxHandleIndex() {
        return this.points.length-1;
    }

    _releaseAssets() {
        cesium.releaseMouseMoveHandler( this.onHandleMove);
        cesium.releaseKeyDownHandler( this.onHandleKey);
        cesium.releaseMouseClickHandler( this.onHandleClick);

        cesium.releaseKeyDownHandler( this.onEditorKey);
    
        viewer.entities.remove( this.polyEntity);
        this._removeHandleEntities( this.handles);
        this._removeHandleEntities( this.halfHandles);
    }

    /* #endregion asset management */

    /* #region entry mode callbacks **********************************************************/

    _onEntryComplete () { // entry is done, register handler to edit 
        this.cancelEntry = undefined;
    
        this._setHalfPointHandles();
        cesium.registerMouseClickHandler( this.onHandleClick);
        cesium.registerKeyDownHandler( this.onEditorKey);
    
        this._setTotalField( this._totalDist());
    }
    
    _onEntryCancel() {
        this.cancelEntry = undefined;
        this._cancel();
    }

    /* #endregion entry mode callbacks */
}


export class PolygonEditor extends PolyEditor {

    constructor (title, points, processResult) {
        super( title, points, processResult);
    }

    _createEditor () {
        return new PolygonEditorWindow( 
            this.title,
            this._enter.bind(this), 
            this._cancel.bind(this), 
            this._setFieldPoint.bind(this), 
            this._insFieldPoint.bind(this), 
            this._delSelectedPoint.bind(this), 
            this._pointSelected.bind(this), 
            this._setMetric.bind(this)
        );
    }

    _createPolyEntity () {
        return new Cesium.Entity( {
            polyline: polylineOpts( this.points),
            polygon: polygonOpts( this.points),
            selectable: false
        });
    }

    _onEntryComplete () { // entry is done, register handler to edit 
        this.cancelEntry = null;

        this.points.push( this.points[0]); // close the polygon outline (but don't add to pointList)
    
        this._setHalfPointHandles();
        cesium.registerMouseClickHandler( this.onHandleClick);
        cesium.registerKeyDownHandler( this.onEditorKey);

        this._setTotalField( this._totalDist());
        this._setAreaField( this._polygonArea());
    }

    _minPoints() { 
        return 3; 
    }

    _maxHandleIndex() {
        return this.points.length-2;
    }

    _setPointAttributes () {
        super._setPointAttributes();

        let points = this.points;
        points.push( points[0]); // close outline
    }

    _enter() {
        this.points.pop(); // remove the closing point (was only for the outline)
        if (this.processResult) this.processResult(this.points);
        this._dispose();
    }

    _totalDist() {
        // set the closing leg dist
        let iLast = this._isInEntryMode() ? this.points.length-1 : this.points.length-2; // after entry we add a closing point
        this.points[0].dist = util.gcDistanceBetweenECEF( this.points[0], this.points[iLast]);

        let dist = 0;
        let points = this.points;
        for (let i=0; i<=iLast; i++) { // don't count the closing point twice
            dist += points[i].dist;
        }

        return dist;
    }

    _polygonArea() {
        return util.ecefPolygonArea( this.points);
    }

    _setAreaField (area) {
        if (!this.isMetric) {
            area = util.squareMetersToAcres(area);
        } else {
            area = util.squareMetersToHectares(area);
        }
        ui.setField( this.editor.areaField, util.formatGroupedFloat( area, 1));
    }

    _setStatsFields() {
        this._setTotalField( this._totalDist());
        this._setAreaField( this._polygonArea());

        ui.updateListItem( this.editor.pointList, this.points[0]); // update closing distance
    }

    // we have to override this because the first/last points have to be moved together
    _onHandleMove (event) { 
        if (this.selHandle) {
            let cp = this.cp;
            cesium.getCartesian3MousePosition( event, cp);
            this.selHandle.position = cp;

            let idx = this.selHandle.__index;

            let p = this.points[idx];
            p.x = cp.x;  p.y = cp.y;  p.z = cp.z;

            if (idx == this.points.length-1) {
                p = this.points[0];
                p.x = cp.x;  p.y = cp.y;  p.z = cp.z;
            }

            this._updateMovingPoint( this.points, idx);
        }
    }
}

export class LineEditor extends PolyEditor {
    constructor (title, points, processResult) {
        super( title, points, processResult);
        this.maxPoints = 2;
        this.minPoints = 2;
    }
}

export class PointEditor extends PolyEditor {
    constructor (title, points, processResult) {
        super( title, points, processResult);
        this.maxPoints = 1;
        this.minPoints = 1;
    }
}


/* #endregion editor classes */

/* #region utility functions ******************************************************************/

function getHalfPoints (points) {
    let pts = [];
    let p0 = ellipsoid.cartesianToCartographic(points[0]);
  
    for (let i = 1; i < points.length; i++) {
        let p1 = ellipsoid.cartesianToCartographic(points[i]);
        let p = ellipsoid.cartographicToCartesian( new Cesium.EllipsoidGeodesic(p0, p1).interpolateUsingFraction(0.5));
        pts.push(p);
        p0 = p1;
    }
  
    return pts;
}

const geoPoint = new Cesium.Cartographic();

function setCartographicPos (p) {
    Cesium.Cartographic.fromCartesian( p, ellipsoid, geoPoint);
    p.lon = util.toDegrees( geoPoint.longitude);
    p.lat = util.toDegrees( geoPoint.latitude);
    p.alt = 0.0;
}

/* #endregion utility functions */


/* #region Cesium asset options ***************************************************************/

function handleOpts (pos) {
    return {
        //position: pos,
        color: config.handleColor,
        outlineColor: config.color,
        outlineWidth: 1,
        pixelSize: config.pointSize,
        //allowPicking: true, // requires PointPrimitive (which does not support clamp-to-ground)
        heightReference: Cesium.HeightReference.CLAMP_TO_GROUND  // requires Entity point
    };
}

function halfHandleOpts (pos) {
    return {
        //position: pos,
        color: config.color,
        //color: Cesium.Color.TRANSPARENT,
        //outlineColor: Cesium.Color.RED,
        //outlineWidth: 1,
        pixelSize: config.pointSize,
        //allowPicking: true
        heightReference: Cesium.HeightReference.CLAMP_TO_GROUND  // requires Entity point
    };
}

function polylineOpts (points) {
    return {
        positions: new Cesium.CallbackProperty( () => points, false),
        clampToGround: true,
        width: 2,
        material: config.color
    };
}

function polygonOpts (points) {
    return {
        hierarchy: new Cesium.CallbackProperty( () => new Cesium.PolygonHierarchy( points)),
        material: config.fillColor,
        //height: 0,
        //outline: true,
        //outlineColor: Cesium.Color.RED,
        //outlineWidth: 2,
    };
}

/* #endregion asset options */

/* #region RectEditor ***********************************************************************************/


class RectEditorWindow {
    constructor (title, enter, cancel, setPoints, setMetric) {
        let fieldOpts = { alignRight: true, isFixed: true, placeHolder: "0.0" };

        this.window = ui.Window( title, "editor", "./asset/odin_cesium/editor.svg")(
            ui.RowContainer()(
                ui.ColumnContainer()(
                    (this.westField = ui.TextInput("west", "editor.west", "6.5rem", fieldOpts)),
                    (this.southField = ui.TextInput("south", "editor.south", "6.5rem", fieldOpts))
                ),
                ui.ColumnContainer()(
                    (this.eastField = ui.TextInput("east", "editor.east", "6.5rem", fieldOpts)),
                    (this.northField = ui.TextInput("north", "editor.north", "6.5rem", fieldOpts))
                ),
                ui.ColumnContainer()(
                    (this.widthField = ui.TextInput("~width", "editor.width", "6rem", {isDisabled: true, isFixed: true, alignRight: true} )),
                    (this.heightField = ui.TextInput("height", "editor.height", "6rem", {isDisabled: true, isFixed: true, alignRight: true} ))
                )
            ),
            ui.RowContainer("center")(
                ui.Button("set", setPoints),
                ui.CheckBox("metric", setMetric),
                (this.areaField = ui.TextInput("area", "editor.area", "9rem", {isDisabled: true, isFixed: true, alignRight: true} )),
                ui.HorizontalSpacer(0.5),
                ui.Button("cancel", cancel),
                ui.Button("save", enter)
            )
        )
    }

    openAt (x, y) {
        ui.addWindow( this.window);
        ui.placeWindow( this.window, x, y);
        ui.setWindowSpotlight( this.window, true);
        ui.showWindow( this.window);
    }
  
    close () {
        ui.closeWindow( this.window);
    }

    getPointFromFields() {
    }
}

/// specialized editor for rectangles. This differs from PolyEditor in that the source of handles and entity
/// is a (west,south,east,north) rect from which we set the 5 element Cartesian3 coordinate array of the asset.
/// The handles initially start on (west,south) and (east,north) but that might change when moved around, which then
/// changes the rect
export class RectEditor {

    constructor (title, rect, isDegrees, processResult) {
        this.title = title;
        this.isDegrees = isDegrees;
        this.processResult = processResult;

        this.cp = new Cesium.Cartesian3(); // cartesian move point cache
        this.cpGeo = new Cesium.Cartographic(); // geo move point cache

        this.editor = this._createEditor();
        this.points = Cesium.Cartesian3.fromDegreesArray([0,0, 0,0, 0,0, 0,0, 0,0]); // 5 point (closed) rect corners

        this.cancelEntry = null;
        this.isMetric = false;

        //--- bind event handler methods to this (we need to be able to remove them)
        this.onHandleClick = this._onHandleClick.bind(this);
        this.onHandleMove = this._onHandleMove.bind(this);
        this.onKeyDown = this._onKeyDown.bind(this);

        if (rect) {        
            this.rect = isDegrees ? util.toRadiansRect(rect) : {...rect};

            this._setPointsFromRect();
            this.rectEntity = this._createRectEntity();
            viewer.entities.add(this.rectEntity);

            this.hp0 = this._createHandleEntity( this.points[0]);  // initially WS - corner might change when edited
            this.hp0.__geo = Cesium.Cartographic.fromCartesian(this.points[0]);
            viewer.entities.add(this.hp0);

            this.hp1 = this._createHandleEntity( this.points[2]);  // initially NE
            this.hp1.__geo = Cesium.Cartographic.fromCartesian(this.points[2]);
            viewer.entities.add(this.hp1);

            this._updateFields(null);
            this._setStatsFields();
        }
    }

    open () {
        this.editor.openAt( xLeft, yTop);
    
        cesium.setRequestRenderMode(false);
        if (this.rectEntity) { // go straight to edit mode
            cesium.registerMouseClickHandler( this.onHandleClick);

        } else { // start in entry mode
            this.rect = new Cesium.Rectangle();
            this._startEntryMode();
        }
    }

    _createEditor() {
        return new RectEditorWindow( this.title, 
            this._enter.bind(this), 
            this._cancel.bind(this), 
            this._setPoints.bind(this),
            this._setMetric.bind(this)
        );
    }

    _setPointsFromRect () {
        let points = this.points;
        let rect = this.rect;
        points[0] = Cesium.Cartesian3.fromRadians( rect.west, rect.south, 0, ellipsoid, points[0]);        
        points[1] = Cesium.Cartesian3.fromRadians( rect.east, rect.south, 0, ellipsoid, points[1]);        
        points[2] = Cesium.Cartesian3.fromRadians( rect.east, rect.north, 0, ellipsoid, points[2]);        
        points[3] = Cesium.Cartesian3.fromRadians( rect.west, rect.north, 0, ellipsoid, points[3]);        
        points[4] = points[0]; // close
    }

    _createRectEntity () {
        return new Cesium.Entity( {
            polyline: polylineOpts( this.points),
            polygon: polygonOpts( this.points), // we could use rectangle but with polygon we can keep a single source
            selectable: false
        } );
    }

    _createHandleEntity (pos) {
        return new Cesium.Entity( {
            position: pos,
            point: handleOpts(),
            selectable: false
        });
    }

    _startEntryMode () {
        this.cancelEntry = cesium.enterRect( this.rect, this.points, { 
            onEnter: this._onEntryComplete.bind(this),
            onCancel: this._onEntryCancel.bind(this), 
            onAddPoint: this._addHandle.bind(this), 
            onMouseMove: this._updateFields.bind(this)
        });
    }

    _isInEntryMode() {
        return (this.cancelEntry != null);
    }

    _cancel() {
        this._dispose();
    }
      
    _enter() {
        if (this.processResult && this.rect) {
            let rect = this.rect;
            this.processResult(rect);
        }
        this._dispose();
    }

    _dispose() {
        var viewportOffset = this.editor.window.getBoundingClientRect();
        xLeft = viewportOffset.left;
        yTop = viewportOffset.top;

        if (this.cancelEntry) {
            this.cancelEntry();
            this.cancelEntry = null;
        }

        this._releaseAssets();
        this.editor.close();

        cesium.setDefaultCursor();
        cesium.setRequestRenderMode(true);
        cesium.requestRender();
    }

    _releaseAssets() {
        cesium.releaseMouseClickHandler( this.onHandleClick);
        cesium.releaseMouseMoveHandler( this.onHandleMove);
        cesium.releaseKeyDownHandler( this.onKeyDown);
    
        let entities = viewer.entities;
        if (this.rectEntity) entities.remove(this.rectEntity);
        if (this.hp0) entities.remove(this.hp0);
        if (this.hp1) entities.remove(this.hp1);
    }

    // set from window fields
    _setPoints() {
        let west  = Number.parseFloat( ui.getFieldValue( this.editor.westField));
        if (!util.checkLon(west)) { alert("missing of invalid west longitude"); return; }

        let south = Number.parseFloat( ui.getFieldValue( this.editor.southField));
        if (!util.checkLat(south)) { alert("missing of invalid south latitude"); return; }

        let east  = Number.parseFloat( ui.getFieldValue( this.editor.eastField));
        if (!util.checkLon(east)) { alert("missing of invalid east longitude"); return; }

        let north = Number.parseFloat( ui.getFieldValue( this.editor.northField));
        if (!util.checkLat(north)) { alert("missing of invalid north latitude"); return; }

        if (this.cancelEntry) {
            this.cancelEntry();
            this.cancelEntry = undefined;
        }

        let rect = this.rect;
        rect.west = util.toRadians(west);
        rect.south = util.toRadians(south);
        rect.east = util.toRadians(east);
        rect.north = util.toRadians(north);
        this._setPointsFromRect();

        let pGeoWS = new Cesium.Cartographic(rect.west, rect.south);
        let pGeoEN = new Cesium.Cartographic(rect.east, rect.north);
        let pWS = Cesium.Cartesian3.fromDegrees(west,south);
        let pEN = Cesium.Cartesian3.fromDegrees(east,north);

        if (this.hp0 === undefined) {
            this.hp0 = _createHandleEntity( pWS);
            viewer.entities.add(this.hp0);
        } else {
            this.hp0.position = pWS;
        }
        this.hp0.__geo = pGeoWS;

        if (this.hp1 === undefined) {
            this.hp1 =  _createHandleEntity( pEN);
            viewer.entities.add(this.hp1);
        } else {
            this.hp1.position = pEN;
        }
        this.hp1.__geo = pGeoEN;

        if (this.rectEntity === undefined) {
            this.rectEntity = this._createRectEntity();
            viewer.entities.add( this.rectEntity);
        }

        this._setStatsFields();

        cesium.registerMouseClickHandler( this.onHandleClick);
    }

    // set dimensional units
    _setMetric(event) {
        let cb = ui.getCheckBox(event.target);
        if (cb) {
            this.isMetric = ui.isCheckBoxSelected(cb);
            this._setStatsFields();
        }
    }

    _onEntryComplete () { // entry is done, register handler to edit 
        this.cancelEntry = undefined;
        cesium.registerMouseClickHandler( this.onHandleClick);
        cesium.registerKeyDownHandler( this.onKeyDown);
    }
    
    _onEntryCancel() {
        this.cancelEntry = undefined;
        this._cancel();
    }

    // handle selection
    _onHandleClick (event) {
        let selHandle = this.selHandle;
        if (event.detail == 1) { // no double click
            if (selHandle) { // end move handle
                let pGeo = cesium.getCartographicMousePosition(event);
                if (pGeo) {
                    let p = Cesium.Cartographic.toCartesian( pGeo, ellipsoid);
                    selHandle.position = p;
                    selHandle.__geo = pGeo;
                    this._updateFromSelHandle();

                    cesium.releaseMouseMoveHandler( this.onHandleMove);
                    cesium.setDefaultCursor();
                    selHandle.point.color = config.handleColor;

                    this.selHandle = undefined;
                }

            } else { // start move handle
                let p = cesium.getWindowMousePosition(event);
                let picked = viewer.scene.pick(p);
                if (picked && picked.id) {
                    let e = picked.id;
                    if (Object.is( e, this.hp0) || Object.is( e, this.hp1)) {
                        cesium.showSelectionIndicator(false); // get rid of this pesky green marker
                        this.selHandle = e;
                        e.point.color = config.selectedHandleColor;
                        cesium.setCursor("crosshair");
                        cesium.registerMouseMoveHandler( this.onHandleMove);
                    }
                }
            }
        }
    }

    // tracking a moving handle
    _onHandleMove (event) { 
        let selHandle = this.selHandle;
        if (selHandle) {
            let cp = cesium.getCartesian3MousePosition( event, this.cp);
            let pGeo = Cesium.Cartographic.fromCartesian(cp, ellipsoid, selHandle.__geo);
            if (pGeo) {
                selHandle.position = cp;
                this._updateFromSelHandle();
            }
        }
    }

    _onKeyDown(event) {
        if (event.code == "Escape") { // exit edit alltogether
            this._cancel();
        } else if (event.code == "Enter") {
            this._enter();
        }
    }

    _updateFromSelHandle () {
        let selHandle = this.selHandle;
        if (selHandle) {
            let pOther = Object.is(selHandle, this.hp0) ? this.hp1.__geo : this.hp0.__geo;
            cesium.setRectFromCornerPoints( this.rect, selHandle.__geo, pOther);
            this._setPointsFromRect();
            this._updateFields(null);
        }
    }

    // entry mode callback (upon click)
    _addHandle (pGeo) {
        let p = Cesium.Cartographic.toCartesian(pGeo, ellipsoid);

        if (this.hp0 === undefined) { // 1st corner
            this.hp0 = this._createHandleEntity( p);
            viewer.entities.add(this.hp0);
            this.hp0.__geo = pGeo.clone();

            this.rectEntity = this._createRectEntity();
            viewer.entities.add( this.rectEntity);

        } else { // 2nd corner
            this.hp1 = this._createHandleEntity( p);
            viewer.entities.add(this.hp1);
            this.hp1.__geo = pGeo.clone();
        }

        this._updateFields( pGeo);
    }

    // during entry - p is (moving) cartographic point
    _updateFields (pGeo) { 
        let rect = this.rect;
    
        if (this.hp0) { // rect is already updated
            ui.setField( this.editor.westField, util.formatFloat( util.toDegrees(rect.west), 5));
            ui.setField( this.editor.southField, util.formatFloat( util.toDegrees(rect.south), 5));
            ui.setField( this.editor.eastField, util.formatFloat( util.toDegrees(rect.east), 5));
            ui.setField( this.editor.northField, util.formatFloat( util.toDegrees(rect.north), 5));
            this._setStatsFields();

        } else { // we don't have a handle yet - just track cartographic pos as (west,south)
            ui.setField( this.editor.westField, util.formatFloat( util.toDegrees(pGeo.longitude), 5));
            ui.setField( this.editor.southField, util.formatFloat( util.toDegrees(pGeo.latitude), 5));
        }
    }

    _setStatsFields () {
        let points = this.points;
        let rect = this.rect;

        // points are always counter-clockwise starting at WS
        let latDist = util.gcDistanceBetweenECEF( points[0], points[3]);
        let lonDistN = util.gcDistanceBetweenECEF( points[2], points[3]);
        let lonDistS = util.gcDistanceBetweenECEF( points[0], points[1]);
        let lonDist = (lonDistN + lonDistS) / 2.0; 

       //let area1 = util.ecefPolygonArea(points);
       let area = util.rectArea(this.rect); // this is in m^2

        if (!this.isMetric) {
            latDist = util.metersToUsMiles(latDist);
            lonDist = util.metersToUsMiles(lonDist);
            area = util.squareMetersToAcres(area);

            ui.setField( this.editor.widthField, util.formatGroupedFloat( lonDist, 2));
            ui.setField( this.editor.heightField, util.formatGroupedFloat( latDist, 2));
            ui.setField( this.editor.areaField, util.formatGroupedFloat( area, 1));

        } else {
            area = util.squareMetersToHectares(area);

            ui.setField( this.editor.widthField, util.formatGroupedFloat( lonDist, 0));
            ui.setField( this.editor.heightField, util.formatGroupedFloat( latDist, 0));
            ui.setField( this.editor.areaField, util.formatGroupedFloat( area, 1));
        }
    }
}

/* #endregion RectEditor */

/* #region CircleEditor *********************************************************************************/

export function editGeoCircle (geoCircle, processResult) {
    function procRes (circle) {
        let geoCircle = main.GeoCircle.fromRadians( circle.longitude, circle.latitude, circle.radius).toRounded();
        processResult( geoCircle);
    }

    new CircleEditor( "Edit GeoCircle", geoCircle, true, procRes).open();
}
main.addShareEditor( main.GEO_CIRCLE, "circle", editGeoCircle);


class CircleEditorWindow {
    constructor (title, enter, cancel, setPoints, setMetric) {
        let fieldOpts = { alignRight: true, isFixed: true, placeHolder: "0.0" };

        this.window = ui.Window( title, "editor", "./asset/odin_cesium/editor.svg")(
            ui.RowContainer()(
                (this.lonField = ui.TextInput("lon", "editor.clon", "6.5rem", fieldOpts)),
                (this.latField = ui.TextInput("lat", "editor.clat", "6.5rem", fieldOpts)),
                (this.radiusField = ui.TextInput("radius", "editor.radius", "6rem", fieldOpts))
            ),
            ui.RowContainer("center")(
                ui.Button("set", setPoints),
                ui.CheckBox("metric", setMetric),
                (this.areaField = ui.TextInput("area", "editor.area", "9rem", {isDisabled: true, isFixed: true, alignRight: true} )),
                ui.HorizontalSpacer(0.5),
                ui.Button("cancel", cancel),
                ui.Button("save", enter)
            )
        );
    }

    openAt (x, y) {
        ui.addWindow( this.window);
        ui.placeWindow( this.window, x, y);
        ui.setWindowSpotlight( this.window, true);
        ui.showWindow( this.window);
    }
  
    close () {
        ui.closeWindow( this.window);
    }
}

export class CircleEditor {

    /// circle: { lon, lat, radius } 
    constructor (title, circle, isDegrees, processResult) {
        this.title = title;
        this.processResult = processResult;

        // caches so that we don't have to continuously create points
        this.cp = new Cesium.Cartesian3();
        this.cp1 = new Cesium.Cartesian3();
        this.diff = new Cesium.Cartesian3();
        this.cpGeo = new Cesium.Cartographic();

        this.points = [];
        let circleEntity = undefined;

        this.editor = this._createEditor();

        // the edit input handlers
        this.onMouseClick = this._onMouseClick.bind(this);
        this.onMouseMove = this._onMouseMove.bind(this);
        this.onKeyDown = this._onKeyDown.bind(this);

        let pGeo0, pGeo1;
        if (circle) {
            let longitude = circle.lon;
            let latitude = circle.lat;    
            if (isDegrees) {
                longitude = util.toRadians(longitude);
                latitude  = util.toRadians(latitude);
            }

            pGeo0 = { longitude, latitude };
            pGeo1 = util.gcEndPosRadians( longitude, latitude, Math.PI/2, circle.radius);

            this.radius = circle.radius;
            this.points.push( Cesium.Cartesian3.fromRadians( pGeo0.longitude, pGeo0.latitude));
            this.points.push( Cesium.Cartesian3.fromRadians( pGeo1.longitude, pGeo1.latitude));

            this.hp0 = this._createHandleEntity( this.points[0]);
            this.hp0.__geo = pGeo0;
            viewer.entities.add(this.hp0);

            this.hp1 = this._createHandleEntity( this.points[1]);
            this.hp1.__geo = pGeo1;
            viewer.entities.add(this.hp1);

            this.circleEntity = this._createCircleEntity();
            viewer.entities.add( this.circleEntity);

            this._updateCenter(pGeo0);
            this._updateRadius();
        }
    }

    open () {
        this.editor.openAt( xLeft, yTop);
    
        cesium.setRequestRenderMode(false);
        if (this.circleEntity) { // go straight to edit mode
            cesium.registerMouseClickHandler( this.onMouseClick);
            cesium.registerKeyDownHandler( this.onKeyDown);

        } else { // start in entry mode
            this._startEntryMode();
        }
    }

    _createEditor() {
        return new CircleEditorWindow( this.title, 
            this._enter.bind(this), 
            this._cancel.bind(this), 
            this._setPoints.bind(this),
            this._setMetric.bind(this)
        );
    }

    _startEntryMode () {
        this.cancelEntry = cesium.enterPolyline( this.points, 2, { 
            onEnter: this._onEntryComplete.bind(this),
            onCancel: this._onEntryCancel.bind(this), 
            onAddPoint: this._addHandle.bind(this), 
            onMouseMove: this._mouseMoved.bind(this)
        });
    }

    _isInEntryMode() {
        return (this.cancelEntry != null);
    }

    _cancel() {
        this._dispose();
    }
      
    _enter() {
        if (this.processResult ) {
            let pGeo = this.hp0.__geo;
            this.processResult( {longitude: pGeo.longitude, latitude: pGeo.latitude, radius: this.radius});
        }
        this._dispose();
    }

    _dispose() {
        var viewportOffset = this.editor.window.getBoundingClientRect();
        xLeft = viewportOffset.left;
        yTop = viewportOffset.top;

        if (this.cancelEntry) {
            this.cancelEntry();
            this.cancelEntry = null;
        }

        this._releaseAssets();
        this.editor.close();

        cesium.setDefaultCursor();
        cesium.setRequestRenderMode(true);
        cesium.requestRender();
    }

    _releaseAssets() {
        cesium.releaseMouseClickHandler( this.onMouseClick);
        cesium.releaseMouseMoveHandler( this.onMouseMove);
        cesium.releaseKeyDownHandler( this.onKeyDown);
    
        let entities = viewer.entities;
        if (this.circleEntity) entities.remove(this.circleEntity);
        if (this.hp0) entities.remove(this.hp0);
        if (this.hp1) entities.remove(this.hp1);
    }

    _setMetric(event) {
        let cb = ui.getCheckBox(event.target);
        if (cb) {
            this.isMetric = ui.isCheckBoxSelected(cb);
            this._updateRadius();
        }
    }

    _setPoints (event) {
        let lon = Number.parseFloat( ui.getFieldValue( this.editor.lonField));
        if (!util.checkLon(lon)) { alert("missing of invalid longitude degrees"); return null; }

        let lat = Number.parseFloat( ui.getFieldValue( this.editor.latField));
        if (!util.checkLat(lat)) { alert("missing of invalid latitude degrees"); return null; }
    
        let radius = Number.parseFloat( ui.getFieldValue( this.editor.radiusField));
        if (Number.isNaN(radius)) { alert("missing or invalid radius"); return null; }
        if (!this.isMetric) radius = util.usMilesToMeters( radius);
        let area = Math.PI * radius * radius;

        this.radius = radius;

        let p0 = new Cesium.Cartesian3.fromDegrees(lon,lat);

        let rp = util.gcEndPosDegrees(lon,lat,90,radius);
        let p1 = new Cesium.Cartesian3.fromDegrees(rp.lon, rp.lat);

        this.points[0] = p0;
        if (this.hp0) {
            this.hp0.position = p0;
        } else {
            this.hp0 = this._createHandleEntity(p0);
            this.hp0.__geo = new Cesium.Cartographic.fromDegrees(lon,lat);
            viewer.entities.add(this.hp0);
        }

        this.points[1] = p1;
        if (this.hp1) {
            this.hp1.position = p1;
        } else {
            this.hp1 = this._createHandleEntity(p1);
            this.hp1.__geo = new Cesium.Cartographic.fromDegrees(rp.lon, rp.lat);
            viewer.entities.add(this.hp1);
        }

        if (this.circleEntity) {
            this.circleEntity.ellipse.semiMajorAxis = radius;
            this.circleEntity.ellipse.semiMinorAxis = radius;
        } else {
            this.circleEntity = this._createCircleEntity();
            viewer.entities.add(this.circleEntity);
        }

        if (this.isMetric) {
            ui.setField( this.editor.areaField, util.formatGroupedFloat( util.squareMetersToHectares(area), 1))
        } else {
            ui.setField( this.editor.areaField, util.formatGroupedFloat( util.squareMetersToAcres(area), 1))
        }

        if (this.cancelEntry){
            this.cancelEntry();
            this._onEntryComplete();
        }
    }

    _onEntryComplete () { // entry is done, register handler to edit 
        this.cancelEntry = undefined;
        cesium.registerMouseClickHandler( this.onMouseClick);
        cesium.registerKeyDownHandler( this.onKeyDown);
    }
    
    _onEntryCancel() {
        this.cancelEntry = undefined;
        this._cancel();
    }

    // entry mode callback (upon click)
    _addHandle (p) {
        let pGeo = Cesium.Cartographic.fromCartesian(p);

        if (this.hp0 === undefined) { // center point
            this.circleEntity = this._createCircleEntity();
            viewer.entities.add( this.circleEntity);

            this.hp0 = this._createHandleEntity( this.points[0]);
            viewer.entities.add(this.hp0);
            this.hp0.__geo = pGeo;

            this._updateCenter( pGeo);

        } else { // radius point
            cesium.clearSelectedEntity();

            this.hp1 = this._createHandleEntity( this.points[1]);
            viewer.entities.add(this.hp1);
            this.hp1.__geo = pGeo;

            this._updateRadius();
        }
    }

    _mouseMoved (points, idx){
        if (this.hp0) { // radius point moved
            this._updateRadius();
            this.circleEntity.ellipse.semiMajorAxis = this.radius;
            this.circleEntity.ellipse.semiMinorAxis = this.radius;

        } else { // center point moved
            let pGeo = this.cpGeo;
            Cesium.Cartographic.fromCartesian( points[idx], ellipsoid, pGeo);
            this._updateCenter(pGeo)
        }
    }

    _onMouseClick (event) {
        cesium.showSelectionIndicator(false);
        //event.preventDefault();
        if (event.detail == 2) return; // don't process double click

        let selHandle = this.selHandle;
        if (selHandle) { // end move handle
            let pGeo = cesium.getCartographicMousePosition(event);
            if (pGeo) {
                let p = Cesium.Cartographic.toCartesian( pGeo, ellipsoid);
                selHandle.position = p;
                selHandle.__geo = pGeo;
                
                if (selHandle == this.hp0) {
                    this.points[0] = p;
                    this._updateCenter(pGeo);
                } else {
                    this.points[1] = p;
                    this._updateRadius(); 
                }

                cesium.releaseMouseMoveHandler( this.onHandleMove);
                cesium.setDefaultCursor();
                selHandle.point.color = config.handleColor;
            }

            this.selHandle = undefined;

        } else { // start move handle
            let p = cesium.getWindowMousePosition(event);
            let picked = viewer.scene.pick(p);
            if (picked && picked.id) {
                let e = picked.id;
                if (Object.is( e, this.hp0) || Object.is( e, this.hp1)) {
                    cesium.showSelectionIndicator(false); // get rid of this pesky green marker
                    this.selHandle = e;
                    e.point.color = config.selectedHandleColor;
                    cesium.setCursor("crosshair");
                    cesium.registerMouseMoveHandler( this.onMouseMove);
                }
            }
        }
    }

    _onMouseMove (event) { 
        let selHandle = this.selHandle;
        if (selHandle) {
            let newPos = cesium.getCartesian3MousePosition( event);
            let pGeo = Cesium.Cartographic.fromCartesian(newPos, ellipsoid, selHandle.__geo);
            if (pGeo) {
                let p0 = this.points[0];
                selHandle.position = newPos;

                if (Object.is( selHandle, this.hp0)) {
                    this.points[0] = newPos;
                    Cesium.Cartesian3.subtract( newPos, p0, this.diff);
                    let p1 = Cesium.Cartesian3.add( this.points[1], this.diff, this.cp1);

                    this.points[1] = p1; 
                    this.hp1.position = p1;
                    this.hp1.__geo = Cesium.Cartographic.fromCartesian(newPos);
                    this._updateCenter(pGeo);

                } else {
                    this.points[1] = newPos;
                    this._updateRadius();
                    this.circleEntity.ellipse.semiMajorAxis = this.radius;
                    this.circleEntity.ellipse.semiMinorAxis = this.radius;
                }
            }
        }
    }

    _onKeyDown(event) {
        if (event.code == "Escape") { // exit edit alltogether
            this._cancel();
        } else if (event.code == "Enter") {
            this._enter();
        }
    }

    _updateRadius () {
        let radius = util.gcDistanceBetweenECEF (this.points[0], this.points[1]);
        this.radius = radius;
        let area = Math.PI * radius * radius;

        if (this.isMetric) {
            ui.setField( this.editor.radiusField, util.formatGroupedFloat( radius, 0));
            ui.setField( this.editor.areaField, util.formatGroupedFloat( util.squareMetersToHectares(area), 1))

        } else {
            ui.setField( this.editor.radiusField, util.formatGroupedFloat( util.metersToUsMiles(radius), 2));
            ui.setField( this.editor.areaField, util.formatGroupedFloat( util.squareMetersToAcres(area), 1))
        }
    }

    _updateCenter(pGeo) {
        ui.setField( this.editor.lonField, util.formatFloat( util.degrees180( util.toDegrees (pGeo.longitude)), 5));
        ui.setField( this.editor.latField, util.formatFloat( util.degrees90( util.toDegrees( pGeo.latitude)), 5));
    }


    _createCircleEntity() {
        let points = this.points;
        let radius = this.radius;
        let radiusProperty = new Cesium.CallbackProperty( () =>radius, false);

        return new Cesium.Entity( {
            position: new Cesium.CallbackProperty( () => points[0], false),
            ellipse: {
                semiMajorAxis: radiusProperty,
                semiMinorAxis: radiusProperty,
                fill: true,
                material: Cesium.Color.RED.withAlpha(0.2),
                //outline: true,
                //outlineColor: Cesium.Color.RED,
                //outlineWidth: 5,
                //height: 0.0
            },
            polyline: polylineOpts( points),
            selectable: false
        });
    }

    _createHandleEntity (pos) {
        return new Cesium.Entity( {
            position: pos,
            point: handleOpts(),
            selectable: false
        });
    }

    updateRadius (pGeo){
        if (pCenter) {
            this.radius = util.gcDistanceBetweenECEF (points[0], points[1]);

            let circleEntity = this.circleEntity;
            if (circleEntity) {
                circleEntity.ellipse.semiMajorAxis = radius;
                circleEntity.ellipse.semiMinorAxis = radius;
            }
        }
    }
}

/* #endregion CircleEditor */