// example odin_share.js configuration asset

export const config = {
    // the top level shared item categories
    // keys are path-like strings composed of static prefix/suffix elements (e.g. "view") and one variable element (e.g. "CZU")
    // var elements can have both static prefixes and suffixes (e.g. "incidents/CZU/view")
    categories: [
        { key: "bbox" ,    type: "/ ⟨GeoRect⟩" }, // the Rust variant name of data under this category 
        { key: "incident", type: "/ *"},
        { key: "point",    type: "/ ⟨GeoPoint⟩" },
        { key: "view",     type: "/ ⟨GeoPoint3⟩" }
    ],

    completions: [
        { pattern: "incident",
            completion: ["/◻︎/view", "/◻︎/origin", "/◻︎/bbox"]
        },
        { pattern: "incident/*",
            completion: ["/view", "/origin", "/bbox"]
        },
        { pattern: "{bbox,point,view}",
            completion: ["/◻︎"]
        }
    ],

    // associates key glob patterns with (server) types tags and Javascript template objects
    // type tags can be empty (or omitted) in which case the server side just stores the data as JSON strings
    // template objects are used to generate JSON templates and check user input 
    typeInfos: [
        { pattern: "{point/**,**/point/**,**/point}", 
            type: "GeoPoint", 
            template: {lon: 0.0, lat: 0.0} 
        },
        { pattern: "{view/**,**/view/**,**/view}",    
            type: "GeoPoint3", 
            template: {lon: 0.0, lat: 0.0, alt: 0.0} 
        },
        { pattern: "{bbox/**,**/bbox/**,**/bbox}",    
            type: "GeoRect", 
            template: {west: 0.0, south: 0.0, east: 0.0, north: 0.0} 
        }
    ],

    maxMessages: 50
}
