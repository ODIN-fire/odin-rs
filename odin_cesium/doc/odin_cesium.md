# odin_cesium

This is a system crate that depends on [`odin_server`], providing `odin_server::SpaService` implementations that are related to using
[CesiumJS](https://cesium.com/platform/cesiumjs/) for user interfaces (UI) on top of a virtual globe display. The crate provides two main
services:

- `CesiumService` is responsible for providing the 3rd party CesiumJS code for the virtual globe rendering, and the
  `odin-rs` specific user interface code that controls it. It also includes some general UI elements that are always
  present (e.g. clock)
- `ImgLayerService` is a configurable service to provide and control background imagery from own and external (proxied) sources.
  It is especially the layer that controls the map display

Since CesiumJS is a large library we do support options for how it is accessed by the client. This can be either by

- directly downloading it from https://cesium.com/downloads/cesiumjs/releases/<version>/Build/Cesium,
- proxying this URL with the `odin-rs` server
- providing it as a static asset from <ODIN-ROOT>/assets/odin_cesium/cesiumjs

Those alternatives are controlled by the Cargo features `cesium_external`, `cesium_proxy` and the default `cesium_asset`.

To obtain, strip and store the requires cesium assets in `<ODIN-ROOT>/odin_cesium/cesiumjs/` we provide the
`install_cesium` binary tool, which you can run like so: `cargo run --bin install_cesium -- --version=<cesium-version>`

To see the current CesiumJS version please visit https://cesium.com/downloads/