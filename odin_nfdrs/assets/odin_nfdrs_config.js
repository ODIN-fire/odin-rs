export const config = {
    layer: {
        name: "/overlay/nfdrs",
        description: "NFDRS fuel moisture and fire danger index grids",
        show: true,
    },

    //--- request parameter choices
    hours: [6, 12, 24, 48, 72], // forecast hours (datasets have hours+1 hourly products)
    defaultHours: 0,

    //--- display
    alpha: 0.8,                 // imagery layer alpha (PNG pixels are already semi-transparent)
    areaColor: Cesium.Color.YELLOW,
    areaWidth: 1,
};
