# ODIN Server Asset Management

uri:  `<crate>/assets/<file>`

## build modes
- local ServeDir("..")  no fallback  : dev
- ODIN_LOCAL ServeDir( ODIN_LOCAL) fallback ServeMem(dict)
- XDG ServeDir( XDG_DATA_HOME) fallback ServeMem(dict)


don't use app-name in either URL or path since this would lead to lots of duplication

make url conforming to partial path -> only 1 ServeDir service (crate root | ODIN_LOCAL | XDG_DATA_HOME ) 
only 1 ServeMem with HashMap<&'static str,&'static[u8]>


## Filesystem

### repository
```
crate-1/
    src/
        assets.rs                    include!(concat!(env!("OUT_DIR"), "/asset_data"))
                                     fn add_assets (&mut app) { app.mem_asset.insert( "crate/asset/file", 
    assets/
        file
        ...
    target/
        asset_data                   <- build.rs
                                     static D1: &'static [u8] = [...]:          

crate-2/
    data/
        ...
```

### external
```
local-odin/                          - or XDG_DATA_HOME -
    crate-1/
        config/                      
        cache/                       
        assets/                       
        data/                        

    crate-2/
        data/
            ...
```