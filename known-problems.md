# Known Problems

### Safari / VPN / Cesium Ion CORS problem (09/09/2025)
When using Safari while on VPN access to both Bing imagery and Cesium.cesiumWorldTerrainAsync() runs into CORS
rejections by the browser and hence can lead to a blocked JS module initialization (odin_cesium.js awaits a terrainProvider)

#### Workaround:
 
- comment out Bing imagery in ODIN_ROOT/assets/odin_cesium/imglayer_config.js
- set 'fakeTerrain' in ODIN_ROOT/assets/odin_cesium/odin_cesium_config.js
  Note this means height queries will not work and height that is received from the ODIN server might render inconsistently

#### Planned Action:
create own Cesium terrain provider in ODIN


### linker cannot be replaced on macOS

using `lld` on macOS by creating a `~/.cargo/config.toml`:

```toml
[target.aarch64-apple-darwin]
rustflags = ["-C", "link-arg=-fuse-ld=lld"]
```

fails to link tokio_unstable (which is unfortunately still required for tracing and giving tasks names)
This means on macOS the standard Xcode (default) linker has to be used
(mold does not support the Apple -dynamic option)


### odin_book mono image rendering on Safari

The image filter to invert monochrome images in documentation pages does not work on Safari. This will need
a specific `img` CSS definition in `odin_book/src/odin.css`.


### odin_book page template is brittle

Using our own `odin_book/theme/index.hbs` is prone to break [`mdbook`](https://rust-lang.github.io/mdBook/index.html)
compatibility. As of this writing (01/2026) `odin_book` is compatible with `mdbook` v0.5.2
