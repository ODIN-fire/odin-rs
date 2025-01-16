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

var shareHandlers = []; // the list of share message handlers set by other modules
var shareEditors = new Map(); // item_type -> {label,editor} map populated by client modules
// var shareViewers = []; // TODO - item_type -> Entity ctor to display shared value
var syncHandlers = [];

export function addShareHandler (newHandler) {
    shareHandlers.push( newHandler);
}

export function notifyShareHandlers (msg) {
    for (let h of shareHandlers) {
        h(msg);
    }
}

// shareEditors are functions that take a single callback function as argument, which is
// called with the entered value when the editor is finished. This needs to use a callback
// since most editors work async (e.g. for interactively entering of picking data)
export function addShareEditor (dataType, label, editorFunc) {
    let editorEntry = {label: label, editor: editorFunc};
    
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

export function getShareEditorForItemType (itemType) {
    return shareEditors.get(itemType);
}


//--- basic share value types

export function radToDeg (rad) { return (rad * 180.0)/Math.PI; }
export function ftToMeters (ft) { return (ft * 0.3048); }

// the basic share-able data value types (note these should *not* depend on any other module)
// it is up to client modules to provide conversion functions (e.g. to/from CesiumJS types)
// The GeoX types have to match the serialization format of odin_common::geo types

export class GeoPoint {
    constructor (lon,lat) {
        this.lon = lon;
        this.lat = lat;
    }
    static fromLonLatDegrees (lon, lat) { return new GeoPoint(lon,lat); }
    static fromLonLatRadians (lonRad, latRad) { return new GeoPoint(radToDeg(lonRad),radToDeg(latRad)); }
}

// this is not in GeoJSON but required to represent camera positions etc.
export class GeoPoint3 {
    constructor (lon,lat, alt) {
        this.lon = lon;
        this.lat = lat;
        this.alt = alt;
    }
    static fromLonLatDegreesMeters (lon, lat, alt) { return new GeoPoint3(lon,lat, alt); }
    static fromLonLatRadiansMeters (lonRad, latRad, altMeters) { return new GeoPoint3(radToDeg(lonRad),radToDeg(latRad), altMeters); }
    static fromGeoPoint (point) { return new GeoPoint3( point.lon, point.lat, 0.0); }
}

export class GeoLine {
    constructor (start,end) {
        this.start = start;
        this.end = end;
    }
}

export class GeoLineString {
    constructor (points) {
        this.points = points;
    }
}

export class GeoPolygon {
    constructor (vertices) {
        this.vertices = vertices;
    }
}

export class GeoRect {
    constructor (west,south,east,north) {
        this.west = west;
        this.south = south;
        this.east = east;
        this.north = north;
    }
    static fromWSENdeg (west,south,east,north) { return GeoRect(west,south,east,north); }
}

//... and eventually all of GeoJSON


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
