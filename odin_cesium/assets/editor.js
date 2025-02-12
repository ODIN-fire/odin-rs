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

/// cesium_test driver
main.exportFuncToMain( function test() {
    function processResult (points) {
        console.log("edit result:", points);
    }

    let points = [
        { lon: -122.0, lat: 40.0 },
        { lon: -119.0, lat: 38.0 },
        { lon: -120.5, lat: 37.0 }
    ];
    //points = [];

    console.log("start editing");
    //editPolygon( points, processResult);
    /*
    let editor = main.getDefaultShareEditorForItemType( main.GEO_LINE_STRING);
    if (editor) {
        let input = new main.GeoLineString( points);
        editor( input, processResult);
    } else console.log("no editor");
     */

    /*
    let editor = main.getDefaultShareEditorForItemType( main.GEO_POLYGON);
    if (editor) {
        let input = new main.GeoPolygon( points);
        editor( input, processResult);
    } else console.log("no editor");
    */

    /*
    let editor = main.getDefaultShareEditorForItemType( main.GEO_LINE);
    if (editor) {
        editor( null, processResult);
    } else console.log("no editor");
    */

    /*
    let editor = editGeoPoint;
    editor( null, processResult);
     */

    let editor = editGeoRect;
    editor( null, processResult);
});
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
            ui.RowContainer("end")(
                this._statsContainer(),
                ui.CheckBox("metric", setMetric),
                ui.HorizontalSpacer(3),
                ui.Button("cancel", cancel),
                ui.Button("save", enter)
            )
        );
        
        ui.setListItemDisplayColumns( this.pointList, ["fit", "header"], [
            { name: "idx", tip: "point index", width: "2rem", attrs: fieldAttrs, map: p => p.idx },
            { name: "lon", tip: "longitude [deg]", width: "7rem", attrs: fieldAttrs, map: p => util.formatFloat(p.lon,5) },
            { name: "lat", tip: "latitude [deg]", width: "6.5rem", attrs: fieldAttrs, map: p => util.formatFloat(p.lat,5) },
            { name: "alt", tip: "altitude [m]", width: "6.5rem", attrs: fieldAttrs, map: p => Math.round(p.alt) },
            { name: "dist", tip: "distance [km]", width: "6rem", attrs: fieldAttrs, map: p => Math.round(p.dist) }
        ]);    
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
        if (Number.isNaN(lat)) { alert("missing latitude"); return null; }
        if (lat < -90.0 || lat > 90.0) { alert("invalid latitude [-90..90]", lat); return null; }
    
        let lon = Number.parseFloat( ui.getFieldValue( this.lonField));
        if (Number.isNaN(lon)){ alert("missing longitude"); return null; }
        if (lon < -180.0 || lon > 180.0) { alert("invalid longitude [-180..180]", lon); return null; }
    
        let alt = Number.parseFloat( ui.getFieldValue( this.altField));
        if (Number.isNaN(alt)) {
            alt = 0.0;
        } else {
            if (alt < 0) { alert("invalid altitude (>0)", alt); return null; }
        }
        
        let p = new Cesium.Cartesian3.fromDegrees( lon, lat, alt);
        p.lon = lon;  p.lat = lat;  p.alt = alt;
    
        return p;
    }

    showTotalDist (dist) {
        ui.setField( this.totalField, Math.round(dist));
    }
}

class PolygonEditorWindow extends PolyEditorWindow {
    constructor (title, enter, cancel, setPoint, insPoint, delPoint, selPoint, setMetric) {
        super(title, enter, cancel, setPoint, insPoint, delPoint, selPoint, setMetric);
    }

    _statsContainer() {
        return ui.ColumnContainer("end")(
            (this.legField = ui.TextInput("leg dist", "editor.leg", "6rem", {isDisabled: true, isFixed: true, alignRight: true} )),
            (this.totalField = ui.TextInput("total dist", "editor.total", "6rem", {isDisabled: true, isFixed: true, alignRight: true} )),
            (this.areaField = ui.TextInput("area", "editor.area", "6rem", {isDisabled: true, isFixed: true, alignRight: true} ))
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
            geoPoint.lon = editedPoints[0].lon;
            geoPoint.lat = editedPoints[0].lat;
            processResult( geoPoint);
        } else {
            processResult( new main.GeoPoint(editedPoints[0].lon, editedPoints[0].lat));
        }
    }

    let init = geoPoint ? [Object.assign({}, geoPoint)] : [];
    new PointEditor( "Edit GeoPoint", init, procRes).open();
}
main.addShareEditor( main.GEO_POINT, "2D point", editGeoPoint);

/// shared item editor func for GeoLine
export function editGeoLine (geoLine, processResult) {
    function procRes (editedPoints) {
        let resultPoints = editedPoints.map( (p)=> new main.GeoPoint(p.lon, p.lat) );
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
        let resultPoints = editedPoints.map( (p)=> new main.GeoPoint(p.lon, p.lat) );
        if (geoLineString) {
            geoLineString.points = resultPoints;
            processResult( geoLineString);
        } else {
            processResult( new GeoLineString(resultPoints));
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
        let resultPoints = editedPoints.map( (p)=> new main.GeoPoint(p.lon, p.lat) );
        if (geoPolygon) {
            geoPolygon.exterior = resultPoints;
            processResult( geoPolygon);
        } else {
            processResult( new GeoPolygon(resultPoints));
        }
    }

    let init = geoPolygon ? geoPolygon.exterior.map( (p)=>Object.assign({}, p)) : [];
    new PolygonEditor( "Edit GeoPolygon", init, procRes).open();
}
main.addShareEditor( main.GEO_POLYGON, "2D polygon exterior", editGeoPolygonExterior);


/// shared item editor func for GeoPolygon
export function editGeoRect (geoRect, processResult) {
    function procRes (editedPoints) {
        if (editedPoints.length == 2) {
            if (geoRect) {
                geoRect.setFromPoints( editedPoints[0], editedPoints[1]);
                processResult( geoRect);
            } else {
                processResult( GeoRect.fromPoints( editedPoints[0], editedPoints[1]));
            }
        }
    }

    let init = geoRect ? geoRect.toRadians() : null;
    new RectEditor( "Edit GeoRect", init, procRes).open();
}
main.addShareEditor( main.GEO_POLYGON, "2D polygon exterior", editGeoPolygonExterior)

/// the base class for PolylineEditor and PolygonEditor
export class PolyEditor {

    constructor (title, points, processResult) {
        this.cp = new Cesium.Cartesian3(); // cache so that we don't need to allocate on each mouseMove
        this.isMetric = true;
    
        this.title = title;
        this.points = points;
        this.processResult = processResult;
    
        this.minPoints = 0;
        this.maxPoints = 0;

        this.editor = this._createEditor();
    
        this.handles = new Cesium.PointPrimitiveCollection();
        viewer.scene.primitives.add( this.handles);

        this.halfHandles = new Cesium.PointPrimitiveCollection();
        viewer.scene.primitives.add( this.halfHandles);

        this.polyEntity = undefined;
        this.selHandle = undefined;
        this.restorePos = undefined;

        this.cancelEntry = null;

        //--- bind event handler methods to this (we need to be able to remove them)
        this.onHandleClick = this._onHandleClick.bind(this);
        this.onHandleMove = this._onHandleMove.bind(this);
        this.onHandleKey = this._onHandleKey.bind(this);
    }

    open () {
        this.editor.openAt( xLeft, yTop);
    
        cesium.setRequestRenderMode(false);

        if (this.points.length) { // no enter mode - create assets and go straight to edit mode
            this._setPointAttributes(); // fill in lon/lat/alt or x/y/z, dist and idx
            this._createAssets();
            cesium.registerMouseClickHandler( this.onHandleClick);
            this._setPointList();
      
        } else {
            this._startEntryMode();
        }

        //ui.setListItems( this.editor.pointList, this.points);
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
        this.cancelEntry = enterPolyline( this.points, this.maxPoints, {  // @override for polygon
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
        
                let hp = this._findHandle(idx);
                hp.position = p;

            } else { // no selection - append new point
                idx = points.length;
                fp.idx = idx;
        
                if (!this.cance) { // no mover, add new point
                    p = fp;
                    points.push(p);
            
                } else { // in edit mode 
                    p = points[points.length-1];
                    p.x = fp.x;  p.y = fp.y;  p.z = fp.z;
                    
                    let pMover = { ...p};
                    pMover.dist = 0;
                    points.push(pMover);
                }
        
                p.dist = idx > 0 ? util.gcDistanceBetweenECEF( points[idx-1], p) : 0;
                this._addHandle(p);
    
            }
    
            this._setHalfPointHandles();
            ui.setField( this.editor.legField, Math.round(p.dist));
        }
    
        this.editor.showTotalDist( this._totalDist());
    }

    _isInEntryMode() {
        return (this.cancelEntry != null);
    }

    _insFieldPoint (){
        // TODO
    }

    _delSelectedPoint (){
        // TODO
    }

    _setMetric (event) {
        // TODO
    }

    /// callback when point is selected from editor.pointList
    _pointSelected (event){
        let editor = this.editor;
        let p = event.detail.curSelection;

        if (p) {
            ui.setField( editor.lonField, util.formatFloat( p.lon, 5));
            ui.setField( editor.latField, util.formatFloat( p.lat, 5));
            ui.setField( editor.altField, Math.round(p.alt));
            ui.setField( editor.legField, Math.round(p.dist));
    
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
            cesium.setDefaultCursor();
            this.selHandle.color = config.handleColor;

            let idx = this.selHandle.__index;
            this._pointMoved( idx, points[idx]);

            this.selHandle = null;
            this._setHalfPointHandles();

        } else { // start tracking mouse
            let picked = viewer.scene.pick(p);

            if (picked && picked.primitive instanceof Cesium.PointPrimitive) {
                let prim = picked.primitive;
                let idx = prim.__index + 1;

                if (prim.__isHalf) { // selected a half handle -> insert new handle
                    points.splice(idx, 0, prim.position);

                    let handles = this.handles;
                    for (let i = 0; i < handles.length; i++) {
                        let hp = handles.get(i);
                        if (hp.__index >= idx) { hp.__index += 1 }
                    }
                    let hp = handles.add( handleOpts( prim.position));
                    hp.__index = idx;
                    this.selHandle = hp;

                    this._pointAdded( idx, points[idx]);
                    this._pointEditing( idx, points[idx]);

                    this.restorePos = {...points[idx]};

                } else { // selected a full handle
                    this.selHandle = prim;
                    this.restorePos = { ...points[idx-1] };
                    this._pointEditing( idx, points[idx-1]);
                }

                this.halfHandles.removeAll(); // we don't want to move them
                this.selHandle.color = config.selectedHandleColor;
                cesium.setCursor("crosshair");
                cesium.registerMouseMoveHandler( this.onHandleMove);
                cesium.registerKeyDownHandler( this.onHandleKey);
            }
        }
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

            this._updateMovingPoint( idx, p);
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

                    let handles = this.handles;
                    for (let i = 0; i < handles.length; i++){
                        let hp = handles.get(i);
                        if (hp.__index > idx) { hp.__index -= 1 }
                    }
                    handles.remove(selHandle);
                    this._resetSelHandle();

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

    _resetSelHandle() {
        if (this.selHandle) {
            this.selHandle.color = config.handleColor;
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
        ui.setField( this.editor.legField, Math.round(p.dist));
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
        this.editor.showTotalDist( this._totalDist());
    }

    /// notification this point was deleted
    _pointDeleted (idx, p){ 
        if (this.points.length < this._minPoints()) { // not enough points left
            this._cancel();

        } else {
            ui.removeListItem( this.editor.pointList, p);
            this._updatePointListFrom( idx);
            ui.setField( this.editor.legField, null);
            this.editor.showTotalDist( this._totalDist());
        }
    }

    /// notification this point is moving with mouse (in flight)
    _updateMovingPoint (idx, p) {
        setCartographicPos(p);
    
        ui.setField( this.editor.lonField, util.formatFloat( p.lon, 5));
        ui.setField( this.editor.latField, util.formatFloat( p.lat, 5));
    
        if (idx > 0) {
            let prev = this.points[idx-1];
        
            p.dist = util.gcDistanceBetweenECEF( prev, p);
            ui.setField( this.editor.legField, Math.round(p.dist));
        
            this.editor.showTotalDist( this._totalDist());
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
        let idx = this.handles.length;
        let hp = this.handles.add( handleOpts(p));
        hp.__index = idx;
        p.idx = idx;
    
        if (!this.polyEntity) {
            this.polyEntity = this._createPolyEntity();
            viewer.entities.add( this.polyEntity);

        }
    
        this._pointAdded( idx, p);
    }

    _delHandle (idx, p) {
        let hp = this._findHandle(idx);
        if (hp)  this.handles.remove(hp);
        this._pointDeleted( idx, p);
    }

    _findHandle (idx) {
        let handles = this.handles;

        for (let i=0; i<handles.length; i++) {
            let hp = handles.get(i);
            if (hp && hp.__index == idx) return hp;
        }
        return null;
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
        return new Cesium.Entity( {polyline: polylineOpts( this.points), selectable: false} );
    }

    _setPointHandles () {
        let points = this.points;
        for (let i=0; i<=this._maxHandleIndex(); i++) {
            let p = points[i];
            let hp = this.handles.add( handleOpts(p));
            hp.__index = i;
        }
    }

    _setHalfPointHandles () {
        this.halfHandles.removeAll();
    
        if (this.maxPoints === undefined || this.points.length < this.maxPoints) { // otherwise we can't create more points
            let halfPoints = getHalfPoints( this.points);
        
            for (let i = 0; i < halfPoints.length; i++){
                let hp = this.halfHandles.add( halfHandleOpts(halfPoints[i]));
                hp.__isHalf = true;
                hp.__index = i;
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
        cesium.releaseMouseMoveHandler( this._onHandleMove);
        cesium.releaseKeyDownHandler( this._onHandleKey);
        cesium.releaseMouseClickHandler( this._onHandleClick);
    
        viewer.entities.remove( this.polyEntity);
        viewer.scene.primitives.remove( this.handles);
        viewer.scene.primitives.remove( this.halfHandles);
    }

    /* #endregion asset management */

    /* #region entry mode callbacks **********************************************************/

    _onEntryComplete () { // entry is done, register handler to edit 
        this.cancelEntry = undefined;
    
        this._setHalfPointHandles();
        cesium.registerMouseClickHandler( this.onHandleClick);
    
        this.editor.showTotalDist( this._totalDist());
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
        this.editor.showTotalDist( this._totalDist());
    }

    _minPoints() { 
        return 3; 
    }

    _maxHandleIndex() {
        return this.points.length-2; // account for the closing point
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

/// the low level entry function - no handles, just points. Handles for subsequent editing
/// have to be added through the provided callbacks. This function does not create any
/// Cesium resources. We do pass in onMouseMove so that we don't have to redundantly calculate
/// a Cartesian3 mouse position. There is no onDelPoint since we can't delete points in enter
/// mode - we can't set the pointer position in Javascript
// callbacks: { onEnter, onCancel, onAddPoint, onDelPoint, onMouseMove }
export function enterPolyline (points, maxPoints, callbacks) {
    let cp = new Cesium.Cartesian3(); // cached point to save allocs
  
    points.push( new Cesium.Cartesian3()); // add the mover point
  
    function onMouseMove(event) { // update the last point position (will redraw polyline using points)    
      let idx = points.length-1;
      let p = points[idx];
  
      cesium.getCartesian3MousePosition(event, cp);
      p.x = cp.x;   p.y = cp.y;   p.z = cp.z;
  
      if (callbacks.onMouseMove) { callbacks.onMouseMove( idx, p); }
    }
  
    function onClick(event) {
      if (event.detail == 2) { // double click -> done entering
        event.preventDefault(); // Cesium likes to zoom in on double clicks
        resetEnterPolyline();
  
        points.pop();  // remove the mover
        if (points.length > 1) {
          if (callbacks.onEnter) callbacks.onEnter();
        }
  
      } else if (event.detail == 1) { // single click (but also before double click)
        cesium.getCartesian3MousePosition(event, cp);
  
        let p = points[points.length-1];
        p.x = cp.x;   p.y = cp.y;   p.z = cp.z;
  
        if (callbacks.onAddPoint) { callbacks.onAddPoint(p); }
  
        if (maxPoints && points.length >= maxPoints) {
            resetEnterPolyline();
            if (callbacks.onEnter) callbacks.onEnter();
        } else {
            let pMover = { ...p };
            points.push( pMover); 
        }
      }
    }
  
    function resetEnterPolyline() {
      cesium.setDefaultCursor();
      cesium.releaseMouseClickHandler(onClick);
      cesium.releaseMouseMoveHandler(onMouseMove);
      cesium.releaseKeyDownHandler(onKeyDown);
    }
  
    function onKeyDown(event) {
        if (event.code == "Delete" || event.code == "Backspace") {
            let idx = points.length-2;
            let p = points[idx];
            points.splice( idx, 1);
            if (callbacks.onDelPoint) callbacks.onDelPoint( idx, p);

        } else if (event.code == "Escape") { // exit edit alltogether
            resetEnterPolyline();
            if (callbacks.onCancel) callbacks.onCancel();
        }
    }
  
    cesium.setCursor("copy");
    cesium.registerMouseClickHandler(onClick);
    cesium.registerMouseMoveHandler(onMouseMove);
    cesium.registerKeyDownHandler(onKeyDown);

    return resetEnterPolyline; // so that we can cancel from the outside
}

/* #region Cesium asset options ***************************************************************/

function handleOpts (pos) {
    return {
        position: pos,
        color: config.handleColor,
        outlineColor: config.color,
        outlineWidth: 1,
        pixelSize: config.pointSize,
        allowPicking: true
    };
}

function halfHandleOpts (pos) {
    return {
        position: pos,
        color: config.color,
        //color: Cesium.Color.TRANSPARENT,
        //outlineColor: Cesium.Color.RED,
        //outlineWidth: 1,
        pixelSize: config.pointSize,
        allowPicking: true
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
    constructor (title, enter, cancel, setPoint, delPoint, selPoint, setMetric) {
        let fieldOpts = { alignRight: true, isFixed: true, placeHolder: "0.0" };
        let fieldAttrs = ["fixed","alignRight"];

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
                    (this.widthField = ui.TextInput("width", "editor.width", "6rem", {isDisabled: true, isFixed: true, alignRight: true} )),
                    (this.heightField = ui.TextInput("height", "editor.height", "6rem", {isDisabled: true, isFixed: true, alignRight: true} ))
                )
            ),
            ui.RowContainer("end")(
                (this.areaField = ui.TextInput("area", "editor.area", "6rem", {isDisabled: true, isFixed: true, alignRight: true} )),
                ui.CheckBox("metric", setMetric),
                ui.HorizontalSpacer(5),
                ui.Button("cancel", cancel),
                ui.Button("save", enter)
            )
        )
    }


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
    }
}

/// specialized editor for rectangles. This differs from PolyEditor in that the source of handles and entity
/// is a (west,south,east,north) rect from which we set the 5 element Cartesian3 coordinate array of the asset.
/// The handles initially start on (west,south) and (east,north) but that might change when moved around, which then
/// changes the rect
export class RectEditor {

    constructor (title, rect, processResult) {
        this.rect = rect;

        this.cp = new Cesium.Cartesian3(); // cartesian move point cache
        this.cpGeo = new Cesium.Cartographic(); // geo move point cache

        this.editor = this._createEditor();
        this.points = Cesium.Cartesian3.fromDegreesArray([0,0, 0,0, 0,0, 0,0, 0,0]); // 5 point (closed) rect corners
        this.handles = new Cesium.PointPrimitiveCollection();
        viewer.scene.primitives.add( this.handles);

        this.cancelEntry = null;

        //--- bind event handler methods to this (we need to be able to remove them)
        this.onHandleClick = this._onHandleClick.bind(this);
        this.onHandleMove = this._onHandleMove.bind(this);
        //this.onHandleKey = this._onHandleKey.bind(this);

        if (rect) {
            this._setPointsFromRect();
            this.rectEntity = this._createRectEntity();

            this.hp0 = this.handles.add( handleOpts( this.points[0].clone()));  // initially WS - corner might change when edited
            this.hp1 = this.handles.add( handleOpts( this.points[2].clone()));  // initially NE
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
        return new RectEditorWindow("Edit Rectangle", this._enter, this._cancel, null,null,null,null);
    }

    _setPointsFromRect () {
        let points = this.points;
        let rect = this.rect;
        points[0] = Cesium.Cartesian3.fromRadians( rect.west, rect.south, 0, ellipsoid, points[0]);        
        points[1] = Cesium.Cartesian3.fromRadians( rect.west, rect.north, 0, ellipsoid, points[1]);        
        points[2] = Cesium.Cartesian3.fromRadians( rect.east, rect.north, 0, ellipsoid, points[2]);        
        points[3] = Cesium.Cartesian3.fromRadians( rect.east, rect.south, 0, ellipsoid, points[3]);        
        points[4] = points[0]; // close
    }

    _createRectEntity () {
        return new Cesium.Entity( {
            polyline: polylineOpts( this.points),
            polygon: polygonOpts( this.points), // we could use rectangle but with polygon we can keep a single source
            selectable: false
        } );
    }

    _startEntryMode () {
        this.cancelEntry = enterRect( this.rect, this.points, { 
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
            this.processResult(this.rect);
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
        cesium.releaseMouseMoveHandler( this._onHandleMove);
        cesium.releaseKeyDownHandler( this._onHandleKey);
        cesium.releaseMouseClickHandler( this._onHandleClick);
    
        viewer.entities.remove( this.rectEntity);
        viewer.scene.primitives.remove( this.handles);
    }

    _onEntryComplete () { // entry is done, register handler to edit 
        this.cancelEntry = undefined;
        cesium.registerMouseClickHandler( this.onHandleClick);
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
                    selHandle.geo = pGeo;
                    this._updateFromSelHandle();

                    cesium.releaseMouseMoveHandler( this.onHandleMove);
                    cesium.setDefaultCursor();
                    selHandle.color = config.handleColor;

                    this.selHandle = undefined;
                }

            } else { // start move handle
                let p = cesium.getWindowMousePosition(event);
                let picked = viewer.scene.pick(p);
                if (picked && picked.primitive instanceof Cesium.PointPrimitive) {
                    if (Object.is( picked.primitive, this.hp0) || Object.is( picked.primitive, this.hp1)) {
                        this.selHandle = picked.primitive;

                        this.selHandle.color = config.selectedHandleColor;
                        cesium.setCursor("crosshair");
                        cesium.registerMouseMoveHandler( this.onHandleMove);
                        //cesium.registerKeyDownHandler( this.onHandleKey);
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
            let pGeo = Cesium.Cartographic.fromCartesian(cp, ellipsoid, selHandle.geo);
            if (pGeo) {
                selHandle.position = cp;
                this._updateFromSelHandle();
            }
        }
    }

    _updateFromSelHandle () {
        let selHandle = this.selHandle;
        if (selHandle) {
            let pOther = Object.is(selHandle, this.hp0) ? this.hp1.geo : this.hp0.geo;
            setRectFromCornerPoints( this.rect, selHandle.geo, pOther);
            this._setPointsFromRect();
            this._updateFields(null);
        }
    }

    // entry mode callback (upon click)
    _addHandle (pGeo) {
        let p = Cesium.Cartographic.toCartesian(pGeo, ellipsoid);

        if (this.hp0 === undefined) { // 1st corner
            this.hp0 = this.handles.add( handleOpts( p));
            this.hp0.geo = pGeo.clone();

            this.rectEntity = this._createRectEntity();
            viewer.entities.add( this.rectEntity);

        } else { // 2nd corner
            this.hp1 = this.handles.add( handleOpts( p));
            this.hp1.geo = pGeo.clone();
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

        } else { // we don't have a handle yet - just track cartographic pos as (west,south)
            ui.setField( this.editor.westField, util.formatFloat( util.toDegrees(pGeo.longitude), 5));
            ui.setField( this.editor.southField, util.formatFloat( util.toDegrees(pGeo.latitude), 5));
        }
    }
}


/// low level Rectangle entry - no Cesium assets, just entering two points and updating a cartographic rectangle
/// and the 5 element cartesian point array.
/// callbacks { onEnter, onCancel, onAddPoint, onMouseMove }
export function enterRect (rect, points, callbacks) {
    let cp2 = new Cesium.Cartographic();
    let p0 = undefined;  // 1st corner of rect
    if (points === undefined) points = Cesium.Cartesian3.fromDegreesArray([0,0, 0,0, 0,0, 0,0, 0,0]);
    if (rect == undefined) rect = new Cesium.Rectangle();

    function onMouseMove(event) {
        let p = cesium.getCartographicMousePosition(event, cp2);
        if (p0) {
            setRectFromCornerPoints( rect, p0, p);
            cesium.cartesian3ArrayFromRadiansRect(rect, points);
        }
        
        if (callbacks.onMouseMove) callbacks.onMouseMove( p);
    }

    function onClick(event) {
        let p = cesium.getCartographicMousePosition(event);
        if (p) { 
            if (event.detail == 1) { // ignore double click
                if (p0 === undefined) { // first corner
                    p0 = p;

                    setRectFromCornerPoints( rect, p0, p0);
                    cesium.cartesian3ArrayFromRadiansRect(rect, points);
                    if (callbacks.onAddPoint) callbacks.onAddPoint( p);

                } else { // 2nd corner - this terminates the entry
                    setRectFromCornerPoints( rect, p0, p);
                    cesium.cartesian3ArrayFromRadiansRect(rect, points);
                    if (callbacks.onAddPoint) callbacks.onAddPoint( p);

                    resetEnterRect();
                    if (callbacks.onEnter) callbacks.onEnter();
                }
            }
        }
    }

    function onKeyDown(event) {
        if (event.code == "Escape") { // exit edit alltogether
            resetEnterRect();
            if (callbacks.onCancel) callbacks.onCancel();
        }
    }

    function resetEnterRect() {
        cesium.setDefaultCursor();
        cesium.releaseMouseMoveHandler(onMouseMove);
        cesium.releaseMouseClickHandler(onClick);
        cesium.releaseKeyDownHandler(onKeyDown);
    }

    cesium.setCursor("copy");
    cesium.registerMouseMoveHandler(onMouseMove);
    cesium.registerMouseClickHandler(onClick);
    cesium.registerKeyDownHandler(onKeyDown);

    return resetEnterRect;
}

function setRectFromCornerPoints (rect, p0, p1) {
    rect.west = Math.min( p0.longitude, p1.longitude);
    rect.south = Math.min( p0.latitude, p1.latitude);
    rect.east = Math.max( p0.longitude, p1.longitude);
    rect.north = Math.max( p0.latitude, p1.latitude);
}

/* #endregion RectEditor */