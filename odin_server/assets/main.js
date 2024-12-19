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
 */

//--- setting up the window.main interface object

var main = {};

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
    if (!main.share) {
        console.log("using default main.share object");
        main.share = new DefaultShare();
    } else {
        console.log("using module provided main.share object");
    }
    console.log("main.js postInitialize complete.");
}

//--- default share interface - these are just forwarded to main.share if it exists and otherwise throw an exception

// note these will throw exceptions if called during normal module init as main.share is only guaranteed to be set
// during module post init. This means shareHandlers/editors have to be set in postInitialize() of respective modules

export function addShareHandler (newHandler) {
    if (main.share) { main.share.addShareHandler(newHandler) } else throw "no main.share set";
}

export function addShareEditor (dataType, label, editorFunc) {
    if (main.share) { main.share.addShareEditor(dataType,label,editorFunc) } else throw "no main.share set";
}

export function getShared (key) {
    if (main.share) { main.share.getShared(key) } else throw "no main.share set";
}

export function getAllMatching (regex) {
    if (main.share) { main.share.getAllMatching(regex) } else throw "no main.share set";
}

export function findAll (pred) {
    if (main.share) { main.share.findAll(pred) } else throw "no main.share set";
}

export function setShared (key, valType, value, isLocal=false, comment=null) {
    if (main.share) { main.share.setShared(key, valType, value, isLocal, comment) } else throw "no main.share set";
}

export function removeShared (key) {
    if (main.share) { main.share.removeShared(key) } else throw "no main.share set";
}

//--- default share object

/// a default share implementation that only shares data between JS modules within the same client.
/// Note this is not backed by an interactive UI and can only be used programmatically through above interface
class DefaultShare {
    constructor() {
        this.sharedItems = new Map();
        this.shareHandlers = [];
        this.shareEditors = new Map();
    }

    addShareHandler (newHandler) {
        this.shareHandlers.push( newHandler);
    }

    addShareEditor (dataType, label, editorFunc) {
        let editorEntry = {label: label, editor: editorFunc};
    
        let editors = this.shareEditors.get(dataType);
        if (editors) {
            editors.push( editorEntry);
        } else {
            this.shareEditors.set( dataType, [editorEntry]);
        }
    }

    getShared (key) {
        return this.sharedItems.get( key);
    }

    getAllMatching (regex) {
        let matching = [];
        for (e of this.sharedItems.entries) {
            if (e[0].match(regex)) matching.push(e);
        }
        matching.sort( (a,b) => a.localeCompare(b)); 
        return matching;
    }

    findAll (pred) {
        let matching = [];
        for (e of this.sharedItems.entries) {
            if (pred(e)) matching.push(e);
        }
        matching.sort( (a,b) => a.localeCompare(b)); 
        return matching;
    }

    setShared (key, type, data, isLocal=false, comment=null) {
        let value = { type, comment, data };
        let sharedItem = { key, value };

        this.sharedItems.set( key, sharedItem);
        this.#notifyShareHandlers( {setShared: sharedItem} );
    }

    removeShared (sharedItem) {
        if (sharedItem) {
            let key = sharedItem.key;
            this.sharedItems.delete( key);
            this.#notifyShareHandlers( {removeShared: key});
        }
    }

    #notifyShareHandlers (msg) {
        for (h of this.shareHandlers) {
            h(msg);
        }
    }
}