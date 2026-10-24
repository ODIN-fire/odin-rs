# odin_landfire

## creating ODIN_ROOT/data/odin_landfire

```sh
unzip -j ~/BP/landfire/LF2025/LF2025_CBD_CONUS.zip LF2025_CBD_CONUS/Tif/LF2025_CBD_CONUS.tif LF2025_CBD_CONUS/LF2025_CBD.csv

~/bin/create_csv_cmap --alpha --nd-color 00000000 --format gdal LF2025_CBD.csv LF2025_CBD-gdal.cmap
~/bin/create_csv_cmap --format odin LF2025_CBD.csv LF2025_CBD.json

gdalwarp -t_srs epsg:4326 -co COMPRESS=DEFLATE LF2025_CBD_CONUS.tif LF2025_CBD_CONUS-4326.tif

gdal raster color-map --color-map LF2025_CBD-gdal.cmap --color-selection exact --add-alpha --co ZLEVEL=9 --co WORLDFILE=YES LF2025_CBD_CONUS-4326.tif LF2025_CBD_CONUS.png

gdal raster tile --parallel-method thread --tiling-scheme WebMercatorQuad --convention tms --min-zoom 4 --max-zoom 12 --skip-blank --progress LF2025_CBD_CONUS.png LF2025_CBD_CONUS-tms

rm LF2025_CBD_CONUS-4326.tif
rm LF2025_CBD_CONUS.png
```

## problem with LF topo data 

LF2020_{Elev,Asp,SlpD}_CONUS.tif files have `nodata` metadata value of 32767 for i16 bands but only use that value for a small number of pixels (~4255). Around costal areas it uses Zero values (-1 for aspect) for costal water and -9999.0 out at sea, meaning it has several de facto nodata values that throw off interpolation/regridding, ending in negative values that in turn throw off clients such as NFDRS and WindNinja. Fix:

(1) set consistent, band-type compatible NODATA value:

```sh
gdal_edit -a_nodata -9999 -unsetstat <filename>
```

The `-unsetstat` is important to see the effects since the data sets are so large they store/cache the statistics

(2) convert erroneous -9999.0 nodata values to declared ones

```sh
swap_values --src 32767 --tgt-nodata [--dry-run] <filename>
```

Using the odin_gdal `swap_value` tool

<!-- anecdote
This was/is a bug in official landfire.gov (SRTM) data triggering a deficiency/bug in data processors such as Elmfire or WindNinja (not recognizing nodata or implausible input, esp. on interpolation) which was masked by a bug in odin_gdal::warp (not properly treating/checking set_tgt_nodata in a gdalwarp compatible way).
AI did not detect the input data problem but did find the odin_gdal::SimpleWarpBuilder bug. The devised patch did not differentiate between input bugs (landfire.gov), output processor bugs (specific, needs work around at the data acquisition callsite), but it was good at fixing the odin_gdal problem. The first one was probably due to not having a visual "if you think something is wrong it always is.." clue due to the size of the input data sets (it only sampled input and didn't find the sparse nodata in the samples). The refactoring deficiency comes from not "suffering" from the human need to separate/compartmentalize concerns and resulting implementation complexity.
The odin bug was an example of a bad hack (using nodata mapping gone wrong to "fix" external input & output problems) masking other bugs and hence suggesting reliability that wasn't there. It is typical for human behavior fixated on a desired outcome (we don't want to know if something that seems to "work" does so for the right reasons).
-->
