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

/** 
 * JS module for global functions and objects - this is an intrinsic module and always included
 * it is not allowed to depend on any other imports/modules
 */

//--- setting up the window.main interface object

var main = {};

export const SHARE_INITIALIZED = { SHARE_INITIALIZED: true };

if (window) {
    if (!window.main) window.main = main; // used as an anchor for global properties available from document
}

/// make a function globally available in the whole document (HTML and all JS modules)
export function exportFuncToMain(func) {
    main[func.name] = func;
}

/// make an object globally available in the whole document (HTML and all JS modules)
export function exportObjToMain(name,obj) {
    main[name] = obj;
}

/// execute f(main[<objName>]) if the main.<objName> property exists and otherwise throw an exception
/// f is a function that takes main.<objName> as an argument (if it exists)
export function withMainObj (objName, f) {
    let o = main[objName];
    if (o) {
        f(o)
    } else {
        throw "unknown main property: " + objName;
    }
}

// postInitPromises are essential promises that have to be resolved before we start post initialization of modules
// note these are dynamic promises that are created during module init, i.e. those are not the same as the
// (static) module init promises themselves (which can be obtained from dynamic import statements for each module)
export var postInitPromises = [];

export function addPostInitPromise (promise) {
    postInitPromises.push( promise);
}

// note this function call has to be awaited to make sure all promises are resolved (this is usually done from the body script module)
export async function resolvePostInitPromises() {
    console.log("resolving", postInitPromises.length, "post init promises");
    await Promise.all( postInitPromises);
    console.log("post init promises resolved.");
}

/// post module init hook
export function postInitialize() {
    if (Object.is(defaultShare, main.share)) {
        console.log("using default main.share object");
        notifyShareHandlers( SHARE_INITIALIZED); // there might be modules processing it
    }
    console.log("main.js postInitialize complete.");
}

//--- default data sharing interface 

// we keep those outside of the share object since we don't want to impose the constraint that
// handlers can only be set from a module's postInitialize()

var shareInitialized = false; // set when sending SHARE_INITIALIZED

var shareHandlers = []; // the list of share message handlers set by other modules
var shareEditors = new Map(); // item_type -> {label,editor} map populated by client modules
// var shareViewers = []; // TODO - item_type -> Entity ctor to display shared value
var syncHandlers = [];

export function addShareHandler (newHandler) {
    shareHandlers.push( newHandler);
}

export function notifyShareHandlers (msg) {
    if (msg.SHARE_INITIALIZED) { 
        shareInitialized = true;
    }

    for (let h of shareHandlers) {
        h(msg);
    }
}

export function isShareInitialized() { return shareInitialized; }

/// shareEditors are functions that take two arguments: `edit( selValue, processEditResult(value))`
/// - `selValue` is the (optional) value that is used to initialize the editor
/// - the `processEditResult(value)` callback function is only called if the edit is not canceled
///   and takes a single edit result value argument. 
export function addShareEditor (dataType, label, editorFunc) {
    let editorEntry = {type: dataType, label, editor: editorFunc};
    
    let editors = shareEditors.get(dataType);
    if (editors) {
        editors.push( editorEntry);
    } else {
        shareEditors.set( dataType, [editorEntry]);
    }
}

export function addSyncHandler (newHandler) {
    syncHandlers.push( newHandler);
}

export function notifySyncHandlers (msg) {
    for (let h of syncHandlers) {
        h(msg);
    }
}

export function getShareEditorEntriesForItemType (itemType) {
    return shareEditors.get(itemType);
}

export function getDefaultShareEditorForItemType (itemType) {
    let editorEntries =  shareEditors.get(itemType);
    return editorEntries ? editorEntries[0].editor : null;
}


//--- basic share value types

const rad2deg = 180.0 / Math.PI;

export function toRadians(deg) { return deg / rad2deg; }
export function toDegrees(rad) { return rad * rad2deg; }
export function ftToMeters (ft) { return (ft * 0.3048); }

const round = Math.round;
export function round5 (x) { return Math.round( x * 100000) / 100000 }

// the basic share-able data value types (note these should *not* depend on any other module)
// it is up to client modules to provide conversion functions (e.g. to/from CesiumJS types)
// The GeoX types have to match the serialization format of odin_common::geo types

export class GeoPoint {
    constructor (lon,lat) {
        this.lon = lon;
        this.lat = lat;
    }
    static fromLonLatDegrees (lon, lat) { return new GeoPoint(lon,lat); }
    static fromLonLatRadians (lonRad, latRad) { return new GeoPoint(toDegrees(lonRad),toDegrees(latRad)); }

    static fromRoundedLonLatDegrees (lon, lat) {
        return new GeoPoint( round5(lon), round5(lat));
    }

    toRounded() {
        return new GeoPoint( round5(this.lon), round5(this.lat));
    }

    static checkType (o) {
        return (
            (o.lon != undefined && typeof o.lon == "number") && 
            (o.lat != undefined && typeof o.lat == "number")
        )
    }

    static template = '{ "lon": 0.0, "lat": 0.0 }'
}

// this is not in GeoJSON but required to represent camera positions etc.
export class GeoPoint3 {
    constructor (lon,lat, alt) {
        this.lon = lon;
        this.lat = lat;
        this.alt = alt;
    }
    static fromLonLatDegreesMeters (lon, lat, alt) { return new GeoPoint3(lon,lat, alt); }
    static fromLonLatRadiansMeters (lonRad, latRad, altMeters) { return new GeoPoint3(toDegrees(lonRad),toDegrees(latRad), altMeters); }
    static fromGeoPoint (point) { return new GeoPoint3( point.lon, point.lat, 0.0); }

    toRounded() {
        return new GeoPoint3( round5(this.lon), round5(this.lat), round(this.alt));
    }

    static checkType (o) {
        return (
            (o.lon != undefined && typeof o.lon == "number") && 
            (o.lat != undefined && typeof o.lat == "number") &&
            (o.alt != undefined && typeof o.alt == "number")
        )
    }

    static template = '{ "lon": 0.0, "lat": 0.0, "alt": 0 }'
}

export class GeoLine {
    constructor (start,end) {
        this.start = start;
        this.end = end;
    }

    toRounded() {
        return new GeoLine( this.start.toRounded(), this.end.toRounded());
    }

    static checkType (o) {
        return (
            (o.start != undefined && GeoPoint.checkType(o.start)) && 
            (o.end != undefined && GeoPoint.checkType(o.end))
        )
    }

    static template = '{\n  "start": {"lon": 0.0, "lat": 0.0},\n  "end": {"lon": 0.0, "lat": 0.0}\n}'
}

export class GeoLineString {
    constructor (points) {
        this.points = points;
    }

    toRounded() {
        let roundedPoints = this.points.map( (p) => p.toRounded() );
        return new GeoLineString( roundedPoints);
    }

    static checkType (o) {
        return (
            (Array.isArray(o.points) && o.points.every( p=> GeoPoint.checkType(p)))
        )
    }

    static template = '{\n  "points": [\n    {"lon": 0.0, "lat": 0.0},\n    {"lon": 0.0, "lat": 0.0},\n    {"lon": 0.0, "lat": 0.0}\n  ]\n}'
}

export class GeoPolyline3 {
    constructor (points) {
        this.points = points;
    }

    toRounded() {
        let roundedPoints = this.points.map( (p) => p.toRounded() );
        return new GeoPolyline3( roundedPoints);
    }

    static checkType (o) {
        return (
            (Array.isArray(o.points) && o.points.every( p=> GeoPoint3.checkType(p)))
        )
    }

    static template = '{\n  "points": [\n    {"lon": 0.0, "lat": 0.0, "alt": 0.0},\n    {"lon": 0.0, "lat": 0.0, "alt": 0.0},\n    {"lon": 0.0, "lat": 0.0, "alt": 0.0}\n  ]\n}'
}

export class GeoPolygon {
    constructor (exterior,interiors=[]) {
        this.exterior = exterior;
        this.interiors = interiors;
    }

    toRounded() {
        let roundedExterior = this.exterior.map( (p)=> p.toRounded());
        let roundedInteriors = this.interiors; // TODO

        return new GeoPolygon( roundedExterior, roundedInteriors);
    }

    static checkType (o) {
        return (
            (Array.isArray(o.exterior) && o.exterior.every( p=> GeoPoint.checkType(p))) &&
            (o.interiors == undefined || 
                (Array.isArray(o.interiors) && o.interiors.every( a=> {
                    Array.isArray(a) && a.every( p=> GeoPoint.checkType(p))
                }))
            )
        )
    }

    static template = '{\n  "exterior": [\n    {"lon": 0.0, "lat": 0.0},\n    {"lon": 0.0, "lat": 0.0},\n    {"lon": 0.0, "lat": 0.0}\n  ]\n}'
}

export class GeoRect {
    constructor (west,south,east,north) {
        this.west = west;
        this.south = south;
        this.east = east;
        this.north = north;
    }
    static fromWSENdeg (west,south,east,north) { return new GeoRect(west,south,east,north); }

    static fromPoints (p1, p2) {
        let rect = new GeoRect(0,0,0,0);
        rect.setFromPoints( p1, p2);
        return rect; 
    }

    toRounded() {
        return new GeoRect(
            Math.round( this.west * 100000) / 100000,
            Math.round( this.south * 100000) / 100000,
            Math.round( this.east * 100000) / 100000,
            Math.round( this.north * 100000) / 100000
        );
    }

    toRectangle() {
        return { west: Math.toRadians(this.west), south: Math.toRadians(this.south), east: Math.toRadians(this.east), north: Math.toRadians(this.north) };
    }

    setFromPoints(p1,p2) {
        if (p1.lon < p2.lon) {
            this.west = p1.lon;
            this.east = p2.lon;
        } else {
            this.west = p2.lon;
            this.east = p1.lon;      
        }

        if (p1.lat < p2.lat) {
            this.south = p1.lat;
            this.north = p2.lat;
        } else {
            this.south = p2.lat;
            this.north = p1.lat;
        }
    }

    toPoints () {
        return [ new GeoPoint( this.west, this.south), new GeoPoint( this.east, this.north) ];
    }

    static checkType (o) {
        return (
            (o.west != undefined && typeof o.west == 'number') && 
            (o.south != undefined && typeof o.south == 'number') && 
            (o.east != undefined && typeof o.east == 'number') && 
            (o.north != undefined && typeof o.north == 'number')
        )
    }

    static template = '{\n  "west": 0.0,\n  "south": 0.0,\n  "east": 0.0,\n  "north": 0.0\n}'
}


export class GeoCircle {
    constructor (lon,lat,radius){
        this.lon = lon; // degrees
        this.lat = lat; // degrees
        this.radius = radius; // meters
    }

    static fromRadians( lon, lat, radius) {
        return new GeoCircle( toDegrees(lon), toDegrees(lat), radius);
    }

    toRounded() {
        return new GeoCircle(
            Math.round( this.lon * 100000) / 100000,
            Math.round( this.lat * 100000) / 100000,
            Math.round( this.radius)
        );
    }

    toCircle () {
        return { 
            lon: toRadians(this.lon), 
            lat: toRadians(this.lat), 
            radius: this.radius 
        };
    }

    static checkType (o) {
        return (
            (o.lon != undefined && typeof o.lon == "number") && 
            (o.lat != undefined && typeof o.lat == "number") &&
            (o.radius != undefined && typeof o.radius == "number")
        )
    }

    static template = '{\n  "lon": 0.0,\n  "lat": 0.0,\n  "radius": 0.0\n}'
}


export const GEO_POINT = GeoPoint.name; // the type name
export const GEO_POINT3 = GeoPoint3.name;
export const GEO_LINE = GeoLine.name;
export const GEO_LINE_STRING = GeoLineString.name;
export const GEO_POLYLINE3 = GeoPolyline3.name;
export const GEO_POLYGON = GeoPolygon.name;
export const GEO_RECT = GeoRect.name;
export const GEO_CIRCLE = GeoCircle.name;

export const F64 = "F64";
export const I64 = "I64";
export const STRING = "String";
export const JSON = "Json";

export const ALL_TYPES = [JSON, STRING, F64, I64, GEO_POINT, GEO_POINT3, GEO_LINE, GEO_LINE_STRING, GEO_POLYLINE3, GEO_POLYGON, GEO_RECT, GEO_CIRCLE];


export function checkType (typeName, data, template=null) {
    switch (typeName) {
        case GEO_POINT: return GeoPoint.checkType(data);
        case GEO_POINT3: return GeoPoint3.checkType(data);
        case GEO_LINE: return GeoLine.checkType(data);
        case GEO_LINE_STRING: return GeoLineString.checkType(data);
        case GEO_POLYGON: return GeoPolygon.checkType(data);
        case GEO_RECT: return GeoRect.checkType(data);
        case GEO_CIRCLE: return GeoCircle.checkType(data);

        case STRING: return typeof data == "string";
        case F64: return typeof data == "number";
        case I64: return typeof data == "number";

        default: 
            if (template) {
                let keys = Object.keys(template);
                return keys.every( k=> (data[k] != undefined) && typeof data[k] == typeof template[k]);
            } else { 
                return true;
            }        
    }
}

export function typeTemplate (typeName) {
    switch (typeName) {
        case GEO_POINT: return GeoPoint.template;
        case GEO_POINT3: return GeoPoint3.template;
        case GEO_LINE: return GeoLine.template;
        case GEO_LINE_STRING: return GeoLineString.template;
        case GEO_POLYGON: return GeoPolygon.template;
        case GEO_RECT: return GeoRect.template;
        case GEO_CIRCLE: return GeoCircle.template;

        case STRING: return '""';
        case F64: return "0.0";
        case I64: return "0";

        default: return "{\n}"
    }
}

// a shared value (of the above types) with addition instance meta data
export class SharedValue {
    constructor (type, comment, data) {
        this.type = type;
        this.comment = comment;
        this.data = data;
    }
}

// a named SharedValue that is either local (on client browser - to share between modules) or global (shared with all other users)
export class SharedItem {
    constructor (key, isLocal, value) {
        this.key = key;
        this.isLocal = isLocal;
        this.value = value;
    }

    name () {
        let idx = this.key.lastIndexOf('/');
        return (idx >= 0) ? this.key.substring(idx+1) : this.key; 
    }
}


//--- default share object

/// a default share implementation that only shares data between JS modules within the same client.
/// Note this is not backed by an interactive UI and can only be used programmatically through above interface
export class Share {
    //--- shared data
    _sharedItems = new Map(); // key -> SharedItem

    //--- ownership / sync
    _ownRoles = new Map(); // roles of this user: Map ( role -> { role, isPublishing, nSubscribers } )
    _extRoles = new Map();  // external roles: Map ( role -> { role, isPublishing, nSubscribers } )

    constructor() {}

    //--- protected members not supposed to be overridden by share modules

    _set (key,sharedItem) {
        this._sharedItems.set(key,sharedItem);
    }

    _get (key) {
        return this._sharedItems.get(key);
    }

    _delete (key) {
        this._sharedItems.delete(key);
    }

    _initExtRoles (roleEntries) {
        let map = new Map();
        for (let e of roleEntries) {
            map.set( e.role, e);
        }
        this._extRoles = map;
    }

    _roleAccepted (roleEntry) {
        this._ownRoles.set( roleEntry.role, roleEntry);
    }

    _extRoleAdded (roleEntry) {
        this._extRoles.set( roleEntry.role, roleEntry);
    }

    _updateRole (roleEntry) {
        let ownRolesChanged = false;
        let extRolesChanged = false;

        let e = this._ownRoles.get(roleEntry.role);
        if (e) {
            e.isPublishing = roleEntry.isPublishing;
            e.nSubscribers = roleEntry.nSubscribers;
            ownRolesChanged = true;
        } else {
            let e = this._extRoles.get(roleEntry.role);
            if (e) {
                e.isPublishing = roleEntry.isPublishing;
                e.nSubscribers = roleEntry.nSubscribers;
                extRolesChanged = true;
            }
        }

        return { ownRolesChanged,extRolesChanged };
    }

    _dropRoles (roles) {
        let ownRolesChanged = false;
        let extRolesChanged = false;

        // note that a role can be either own or ext but not both
        for (let r of roles) {
            let e = this._ownRoles.get(r);
            if (e) {
                this._ownRoles.delete(r);
                ownRolesChanged = true;
            } else {
                e = this._extRoles.get(r);
                if (e) {
                    this._extRoles.delete( r);
                    extRolesChanged = true;
                }
            }
        }
        return { ownRolesChanged,extRolesChanged };
    }

    _setExtRolePublished (role, isPublishing) {
        let e = this._extRoles.get(role);
        if (e && e.isPublishing != isPublishing) {
            e.isPublishing = isPublishing;
            return e;
        }
        return null;
    }

    _ownRolesList() {
        return Array.from( this._ownRoles.values()).sort( (a,b)=> a.role.localeCompare(b.role));
    }

    _extRolesList() {
        return Array.from( this._extRoles.values()).sort( (a,b)=> a.role.localeCompare(b.role));
    }

    _getOwnRole(role) {
        return this._ownRoles.get(role);
    }

    _getExtRole(role) {
        return this._extRoles.get(role);
    }

    _isSubscribedToExtRole (role) {
        let e = this._extRoles.get(role);
        return (e && e.isSubscribed); 
    }

    //--- the public getters (can be called from any module using shared items)

    getSharedItem (key) {
        return this._sharedItems.get( key);
    }

    hasSharedItem (key) {
        return this._sharedItems.get( key) != undefined;
    }
    
    // this returns a list of SharedItem objects
    getAllMatchingSharedItems (regex) {
        let matching = [];
        for (let sharedItem of this._sharedItems.values()) {
            if (sharedItem.key.match(regex)) matching.push(sharedItem);
        }
        matching.sort( (a,b) => a.key.localeCompare(b.key)); 
        return matching;
    }
    
    findAllSharedItems (pred) {
        let matching = [];
        for (let sharedItem of this._sharedItems.values()) {
            if (pred(sharedItem)) matching.push(sharedItem);
        }
        matching.sort( (a,b) => a.key.localeCompare(b.key)); 
        return matching;
    }

    requestRole (role) {
        if (!this._ownRoles.get(role)) {
            this._roleAccepted( {role, isPublishing: false, nSubscribers: 0});
        }
    }

    releaseRole (role) {
        if (this._ownRoles.get(role)) {
            this._dropRoles( [role]);
        }
    }

    publishRole (role, isPublishing) {
        let e = this._ownRoles.get(role);
        if (e && e.isPublishing != isPublishing) {
            e.isPublishing = isPublishing;
            return true;
        }
        return false;
    }

    subscribeToExtRole (role, isSubscribed) {
        let e = this._extRoles.get(role);
        if (e) {
            e.isSubscribed = isSubscribed;
        }
    }

    publishCmd (cmd) {
        // nothing to do locally - we don't have to sync with ourselves
    }

    setSharedItem (key, type, data, isLocal=false, comment=null) {
        let value = new SharedValue( type, comment, data);
        let sharedItem = new SharedItem( key, isLocal, value);

        this._sharedItems.set( key, sharedItem);
        notifyShareHandlers( {setShared: sharedItem} );
    }

    removeSharedItem (key) {
        let sharedItem = this._sharedItems.get(key);
        if (sharedItem) {
            this._sharedItems.delete( key);
            notifyShareHandlers( {removeShared: key});
        }
    }
}


var defaultShare = new Share();
main.share = defaultShare;

//--- the share API used by other modules - forwarding to the main.share object so that we don't have to hand out its reference

export function setShareObj (newShare) {
    console.log("setting share object");

    let oldItems = main.share._sharedItems;
    main.share = newShare;

    if (oldItems.size > 0) {
        // carry over existing shared items
        oldItems.forEach( (sharedItem,key) => {
            if (sharedItem.isLocal && !newShare._sharedItems.has(key)) {
                // we can't do server updates through the websocket yet so this can only copy over local shared items
                newShare._set( sharedItem.key, sharedItem);
            }
        })
    }
}

export function getSharedItem (key) {
    return main.share.getSharedItem(key);
}

export function getAllMatchingSharedItems (regex) {
    return main.share.getAllMatchingSharedItems(regex);
}

export function findAllSharedItems (pred) {
    return main.share.findAllSharedItems(pred);
}

export function setSharedItem (key, valType, data, isLocal=false, comment=null) {
    main.share.setSharedItem(key, valType, data, isLocal, comment);
}

export function removeSharedItem (key) {
    main.share.removeSharedItem(key);
}

export function publishCmd (cmd) {
    main.share.publishCmd(cmd);
}
