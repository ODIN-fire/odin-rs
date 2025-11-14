export const config = {
  layer: {
    name: "/overlay/firehistory",
    description: "static map overlays with historic fire data",
    show: true,
  },
  zoomHeight: 80000,
  perimeterRender: {
    strokeColor: Cesium.Color.fromCssColorString('orange'),
    strokeWidth: 1.5,
    fillColor: Cesium.Color.fromCssColorString('#f00000'),
    fillOpacity: 0.5,
    dimFactor: 0.8
  }
}