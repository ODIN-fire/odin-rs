# odin_cesium

This is a system crate that depends on [`odin_server`], providing `odin_server::SpaService` implementations that are related to using
[CesiumJS](https://cesium.com/platform/cesiumjs/) for user interfaces (UI) on top of a virtual globe display that is rendered by means
of [WebGL](https://developer.mozilla.org/en-US/docs/Web/API/WebGL_API) inside of a standard web browser. 

This crate provides two main services:

- `CesiumService` is responsible for providing the 3rd party CesiumJS code for the virtual globe rendering, and the
  `odin-rs` specific user interface code that controls it. It also includes some general UI elements that are always
  present (e.g. clock)
- `ImgLayerService` is a configurable service to provide and control background imagery from own and external (proxied) sources.
  It is especially the layer that controls the map display


## How to access CesiumJS

Since CesiumJS is a large 3rd party library we do support options for how it is accessed by the client. This can be either by

- directly downloading it from https://cesium.com/downloads/cesiumjs/releases/<version>/Build/Cesium,
- proxying this URL with the `odin-rs` server - this is the default
- providing it as a static asset from <ODIN-ROOT>/assets/odin_cesium/cesiumjs

Those alternatives are controlled by the Cargo features `cesium_external`, `cesium_proxy` and `cesium_asset`.
Please note that crates depending on `odin_cesium` should support "pass-through" features in their respective `Cargo.toml` like so:

```toml
...
[dependencies]
...
odin_cesium = { workspace = true }

[features]
...
cesium_asset = ["odin_cesium/cesium_asset"]
cesium_external = ["odin_cesium/cesium_external"]
```

There is no need to pass through `cesium_proxy` as it is the default. While this is convenient for setting up development
environments with minimal configuration this might cause reloading and should be avoided in a production environment.

To obtain, strip and store the requires cesium assets in `<ODIN-ROOT>/odin_cesium/cesiumjs/` we provide the
`install_cesium` binary tool, which you can run like so: `cargo run --bin install_cesium -- --version=<cesium-version>`

In all cases the Cesium version to use is specified as a `CESIUM_VERSION` constant in `odin_orbital/src/lib.rs`. If you use
the `cesium_asset` option this has to correspond with the downloaded/installed version.

To see the current CesiumJS version please visit https://cesium.com/downloads/.


## Testing Data and UI Rendering

Since [client side](../odin_server/client.md) `odin-rs` code is written in Javascript there is - depending on the development
environment in use - only limited type checking available. It is therefore good practice to test often and early, especially
since this applies to both the [`odin-rs` user interface](../odin_server/ui_library.md) and to CesiumJS / WebGL based service-specific
data rendering.

To simplify testing of UI and rendering without a fully functional `SpaService` we provide the `cesium_test` tool. This is a 
test harness for `odin-rs` Javascript modules that can make full use of the `odin-rs` [UI library](../odin_server/ui_library.md) and
the CesiumJS library (including its `odin-rs` specific extensions in `odin_cesium.js`). The test module is provided as an explicit
file pathname when launching `cesium_test` like so:

```shell
cargo run --bin cesium_test  --  <JS-module-pathname>
```

Test modules have to use the `exportFuncToMain(..)` function of `main.js` to set a global parameterless `start()` function that
can be triggered by a respective "start" button which is automatically added to the right of the icon box at the top of the page.
This function should in turn execute the behavior that is supposed to be tested. Look at the minimal `odin_cesium/test/test.js`
example for details:

```javascript
import * as main from "../odin_server/main.js";

/* import on demand */
// import * as util from "../odin_server/ui_util.js";
// import * as data from "../odin_server/ui_data.js";
// import * as ui from "../odin_server/ui.js";
// import * as odinCesium from "./odin_cesium.js";

function start() {
    console.log("get started.");
    // trigger your real test here
}
main.exportFuncToMain(start);

// your test functions/data goes here...
```

Since `cesium_test` is a test and debug tool it automatically sets the `ODIN_RELOAD_ASSETS` environment variable used by
[`odin_build`](../odin_build/odin_build.md) and therefore it does not have to be relaunched if any of the involved Javascript
modules is changed - just refresh the page in the browser and hit "start" again to see the change effect.
