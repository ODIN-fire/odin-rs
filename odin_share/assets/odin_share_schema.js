/// example schema for shared data

// the application-specific schema for shared data. The underlying storage type is a general key/value store.
// Keys are path-like strings composed of static prefix/suffix elements (e.g. "view") and one or more 
// variable elements (names, e.g. "CZU" in "incident/CZU/origin"). Variable elements can occur in both infix
// and suffix positions.
// The schema does not use a traditional language such as XML since it is mostly used as hints for the client UI, i.e.
// we still want to allow free form entry
export const schema = {
    
    // the structural node levels we present (even if they don't have child nodes)
    // if expand property is not set the item will be shown collapsed
    keyCategories: [
        { key: "geometry", expand: true },
        { key: "geometry/point" },
        { key: "geometry/line" },
        { key: "geometry/rect" },
        { key: "geometry/polygon" },
        { key: "geometry/circle" },

        { key: "view" },
        { key: "view/globe" },
        { key: "view/region" },
        { key: "view/local" },

        { key: "incident" }
    ],

    // known suffixes for key patterns - used to show known key name patterns (informal - just a hint)
    // each completion is just for the next key path level
    keyCompletions: [
        { pattern: "geometry", completion: ["/point", "/line", "/polyline", "/rect", "/polygon", "/circle"]},
        { pattern: "geometry/{point,line,polyline,rect,polygon,circle}", completion: ["/◻︎"] },

        // example of keys where the variable name is a suffix path element
        { pattern: "view", completion: ["/globe", "/region", "/local"] },
        { pattern: "view/*", completion: ["/◻︎"] },

        // example of keys where the variable name is an infix path element
        { pattern: "incident", completion: ["/◻"] },
        { pattern: "incident/*", completion: ["/view", "/org", "/cause", "/rect", "/origin", "/perimeter", "/line", "/fta"] },
    ],

    // shared item types associated with key patterns
    keyTypes: [
        //--- view patterns
        { pattern: "{**/view/**,**/view}",         types: ["GeoPoint3"] },  // anythig with a 'view' element in the key path is a GeoPoint3

        //--- geometry patterns
        { pattern: "{**/point/**,**/point}",       types: ["GeoPoint"] },
        { pattern: "{**/line/**,**/line}",         types: ["GeoLine", "GeoLineString"] },
        { pattern: "{**/polyline/**,**/polyline}", types: ["GeoLineString"] },
        { pattern: "{**/rect/**,**/rect}",         types: ["GeoRect"] },
        { pattern: "{**/bbox/**,**/bbox}",         types: ["GeoRect"] },
        { pattern: "{**/polygon/**,**/polygon}",   types: ["GeoPolygon"] },
        { pattern: "{**/circle/**,**/circle}",     types: ["GeoCircle"] },
        { pattern: "{**/area/**,**/area}",         types: ["GeoPolygon", "GeoRect", "GeoCircle"] },


        //--- incident patterns
        { pattern: "incident/*/fta",               types: ["GeoCircle"] },
        { pattern: "incident/*/cause",             types: ["String"] },
        { pattern: "incident/*/origin",            types: ["GeoPoint"] },
        { pattern: "incident/*/perimeter",         types: ["GeoPolygon"] }
    ],

    // JSON templates for known key patterns (overrides default type templates)
    // this is mostly for free-form JSON structures as we otherwise use configured editors for known types to enter/edit
    keyTemplates: [
        { pattern: "incident/*/org",  template: '{\n  "type": 3,\n  "divisions": [\n    { "name": "A", "left": "", "right": "" }\n  ]\n}' }
    ]
}