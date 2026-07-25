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
