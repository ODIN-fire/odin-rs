export const config = {
  layer: {
    name: "/overlay/fires",
    description: "operational fire data",
    show: true,
  },
  zoomHeight: 80000,
  perimeterRender: {
    strokeColor: Cesium.Color.fromCssColorString('#ffff00'),
    strokeWidth: 1.5,
    fillColor: Cesium.Color.fromCssColorString('#f00000'),
    fillOpacity: 0.5,
    dimFactor: 0.9
  }
}