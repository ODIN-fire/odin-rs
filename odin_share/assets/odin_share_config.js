// example odin_share.js configuration asset

export const config = {
    layer: {
        name: "/system/sharedItems",
        description: "local/global shared items and sync",
        show: true,
    },

    // the top level shared item categories
    // keys are path-like strings composed of static prefix/suffix elements (e.g. "view") and one variable element (e.g. "CZU")
    // var elements can have both static prefixes and suffixes (e.g. "incidents/CZU/view")

    // the toplevel nodes we always show even if they don't have child nodes
    keyCategories: [
        { key: "bbox" ,    type: "" }, 
        { key: "incident", type: "" }, 
        { key: "point",    type: "" },
        { key: "view",     type: "" },
        { key: "area",     type: "" }
    ],

    // known suffixes for key patterns
    keyCompletions: [
        { pattern: "incident", completion: ["/◻/view", "/◻︎/cause", "/◻︎/bbox", "/◻︎/perimeter"] },
        { pattern: "incident/*", completion: ["/view", "/cause", "/bbox", "/perimeter"] },
        { pattern: "{bbox,point}", completion: ["/◻︎"] },
        { pattern: "view", completion: ["/globe/◻︎", "/region/◻︎", "/state/◻︎/◻︎"] },
        { pattern: "view/*", completion: ["/◻︎"] },
        { pattern: "area", completion: ["/◻︎"] },
    ],

    // associates key glob patterns with (server) types tags and Javascript template objects
    // type tags can be empty (or omitted) in which case the server side just stores the data as JSON strings
    // template objects are used to generate JSON templates and check user input 
    keyTypes: [
        { pattern: "{point/**,**/point/**,**/point}", type: "GeoPoint" },
        { pattern: "{view/**,**/view/**,**/view}",    type: "GeoPoint3" },
        { pattern: "{bbox/**,**/bbox/**,**/bbox}",    type: "GeoRect" },
        { pattern: "{area/**,**/area/**,**/area}",    type: "GeoPolygon" },
        { pattern: "**/cause",  type: "String"},
        { pattern: "{perimeter/**,**/perimeter/**,**/perimeter}", type: "GeoPolygon" }
    ],

    // JSON templates for known types
    typeTemplates: new Map([
        ["GeoPoint",        {lon: 0.0, lat: 0.0 }],
        ["GeoPoint3",       {lon: 0.0, lat: 0.0, alt: 0.0 }],
        ["GeoLine",         {start: {lon: 0.0, lat: 0.0, alt: 0.0 }, end: {lon: 0.0, lat: 0.0, alt: 0.0 }}],
        ["GeoLineString",   {points: [{lon: 0.0, lat: 0.0, alt: 0.0 }, {lon: 0.0, lat: 0.0, alt: 0.0 }]}],
        ["GeoRect",         {west: 0.0, south: 0.0, east: 0.0, north: 0.0}],
        ["GeoPolygon",      {exterior: [{lon: 0.0, lat: 0.0, alt: 0.0 }, {lon: 0.0, lat: 0.0, alt: 0.0 }]}],
        ["String",          ""],
        ["F64",             0.0],
        ["U64",             0],
        ["Json",            {}]
    ]),

    maxMessages: 50,

    // rendering options
    color: Cesium.Color.AQUA,
    labelFont: '16px sans-serif',
    labelBackground: Cesium.Color.BLACK,
    labelOffset: new Cesium.Cartesian2( 8, 0),
    labelDC: new Cesium.DistanceDisplayCondition( 0, Number.MAX_VALUE),
    pointSize: 5,
    lineWidth: 2,
    outlineColor: Cesium.Color.BLUE,
    outlineWidth: 1,
    fillColor: Cesium.Color.AQUA.withAlpha(0.3),
    pointDC: new Cesium.DistanceDisplayCondition( 0, Number.MAX_VALUE),
}
