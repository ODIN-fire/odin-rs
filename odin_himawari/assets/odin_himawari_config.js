export const config = {
    layer: {
      name: "/fire/detection/Himawari",
      description: "Himawari hotspot data",
      show: true,
    },
    maxDataSets: 60, // 10h (update interval is 10min)
    maxMissingMin: 15,

    followLatest: true,
    pointSize: 5,

    outlineWidth: 1,
    strongOutlineWidth: 2,

    flamingColor: Cesium.Color.fromCssColorString('Red'),
    smolderingColor: Cesium.Color.fromCssColorString('OrangeRed'),
    coldColor:  Cesium.Color.fromCssColorString('Orange'),

    flamingMaterial: new Cesium.ImageMaterialProperty({image: './asset/odin_himawari/radial-red.png', transparent: true}),
    smolderingMaterial: new Cesium.ImageMaterialProperty({image: './asset/odin_himawari/radial-orangered.png', transparent: true}),
    coldMaterial: new Cesium.ImageMaterialProperty({image: './asset/odin_himawari/radial-orange.png', transparent: true}),

    hiReliableColor: Cesium.Color.fromCssColorString('Magenta'),
    normReliableColor: Cesium.Color.fromCssColorString('Yellow'),
    lowReliableColor: Cesium.Color.fromCssColorString('LightGrey'),

    pointDC: new Cesium.DistanceDisplayCondition( 0, Number.MAX_VALUE),
    boundsDC: new Cesium.DistanceDisplayCondition( 0, 80000),
    zoomHeight: 100000,
};
