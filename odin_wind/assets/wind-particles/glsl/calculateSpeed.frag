//#version 300 es

// the size of UV textures: width = lon, height = lat*lev
uniform sampler2D U; // eastward wind 
uniform sampler2D V; // northward wind
uniform sampler2D W; // up/down wind
uniform sampler2D currentParticlesPosition; // (lon, lat, lev)

uniform vec3 dimension; // (lon, lat, lev)
uniform vec3 minimum; // minimum of each dimension
uniform vec3 maximum; // maximum of each dimension
uniform vec3 interval; // interval of each dimension

// used to calculate the wind norm
uniform vec2 uSpeedRange; // (min, max);
uniform vec2 vSpeedRange;
uniform vec2 wSpeedRange;
uniform float pixelSize;
uniform float speedFactor;

in vec2 v_textureCoordinates;
out vec4 fragColor;

// NOTE - grid lookup is done in index space with texelFetch since textures use NEAREST sampling. Longitudes are
// measured from the western grid boundary modulo 360 so that grids crossing the prime meridian (where minimum.x is
// normalized to [0,360) and maximum.x can exceed 360) and particle positions in any longitude representation work.
// Grid rows are stored north to south (as in the grid CSV), i.e. texture row 0 is the northern most row

// fractional (col, row-from-south, lev) index of a position
vec3 gridIndex(vec3 lonLatLev) {
    float width = maximum.x - minimum.x;
    float dLon = mod(lonLatLev.x - minimum.x, 360.0);
    if (dLon > 180.0 + width / 2.0) dLon -= 360.0; // west of grid

    return vec3(
        dLon / interval.x,
        (lonLatLev.y - minimum.y) / interval.y,
        (interval.z != 0.0) ? (lonLatLev.z - minimum.z) / interval.z : 0.0  // WATCH OUT - interval.z might be 0
    );
}

float gridValue(sampler2D componentTexture, int col, int iy, int iz) {
    int nx = int(dimension.x);
    int ny = int(dimension.y);
    int nz = int(dimension.z);
    col = clamp(col, 0, nx - 1);
    iy = clamp(iy, 0, ny - 1);
    iz = clamp(iz, 0, nz - 1);
    int row = iz * ny + (ny - 1 - iy);
    return texelFetch(componentTexture, ivec2(col, row), 0).r;
}

float interpolateTexture(sampler2D componentTexture, vec3 lonLatLev) {
    vec3 idx = gridIndex(lonLatLev);
    float x0 = floor(idx.x);
    float y0 = floor(idx.y);
    float fx = idx.x - x0;
    float fy = idx.y - y0;
    int i = int(x0);
    int j = int(y0);
    int k = int(round(idx.z));

    float v00 = gridValue(componentTexture, i,   j,   k);
    float v10 = gridValue(componentTexture, i+1, j,   k);
    float v01 = gridValue(componentTexture, i,   j+1, k);
    float v11 = gridValue(componentTexture, i+1, j+1, k);

    return mix( mix(v00, v10, fx), mix(v01, v11, fx), fy);
}

vec3 linearInterpolation(vec3 lonLatLev) {
    // https://en.wikipedia.org/wiki/Bilinear_interpolation
    float u = interpolateTexture(U, lonLatLev);
    float v = interpolateTexture(V, lonLatLev);
    //float w = interpolateTexture(W, lonLatLev);
    float w = 0.1; // already causes error -> subsequent w==0 assumption ? 
    //float w = 0.0;

    return vec3(u, v, w);
}

vec2 lengthOfLonLat(vec3 lonLatLev) {
    // unit conversion: meters -> longitude latitude degrees
    // see https://en.wikipedia.org/wiki/Geographic_coordinate_system#Length_of_a_degree for detail

    // Calculate the length of a degree of latitude and longitude in meters
    float latitude = radians(lonLatLev.y);

    float term1 = 111132.92;
    float term2 = 559.82 * cos(2.0 * latitude);
    float term3 = 1.175 * cos(4.0 * latitude);
    float term4 = 0.0023 * cos(6.0 * latitude);
    float latLength = term1 - term2 + term3 - term4;

    float term5 = 111412.84 * cos(latitude);
    float term6 = 93.5 * cos(3.0 * latitude);
    float term7 = 0.118 * cos(5.0 * latitude);
    float longLength = term5 - term6 + term7;

    return vec2(longLength, latLength);
}

vec3 convertSpeedUnitToLonLat(vec3 lonLatLev, vec3 speed) {
    vec2 lonLatLength = lengthOfLonLat(lonLatLev);
    float u = speed.x / lonLatLength.x;
    float v = speed.y / lonLatLength.y;
    float w = speed.z / lonLatLength.x; // ?? we just need to scale proportionally (lon is less dependent than lat)
    //float w = 0.0;
    vec3 windVectorInLonLatLev = vec3(u, v, w);

    return windVectorInLonLatLev;
}

vec3 calculateSpeedByRungeKutta2(vec3 lonLatLev) {
    // see https://en.wikipedia.org/wiki/Runge%E2%80%93Kutta_methods#Second-order_methods_with_two_stages for detail
    const float h = 0.5;
    float speedScaleFactor = speedFactor * pixelSize;       //<<<<<<<<<<<<<<<<<<<<

    vec3 y_n = lonLatLev;
    vec3 f_n = linearInterpolation(lonLatLev);
    vec3 midpoint = y_n + 0.5 * h * convertSpeedUnitToLonLat(y_n, f_n) * speedScaleFactor;
    vec3 speed = h * linearInterpolation(midpoint) * speedScaleFactor;

    return speed;
}

vec3 calcSpd (vec3 lonLatLev) {
    float speedScaleFactor = speedFactor * pixelSize;     //<<<<<<<<<<<<<<<<<<<<
    return linearInterpolation(lonLatLev) * speedScaleFactor;
}

float calculateWindNorm(vec3 speed) {
    vec3 percent = vec3(0.0);
    percent.x = (speed.x - uSpeedRange.x) / (uSpeedRange.y - uSpeedRange.x);
    percent.y = (speed.y - vSpeedRange.x) / (vSpeedRange.y - vSpeedRange.x);
    percent.z = (speed.z - wSpeedRange.x) / (wSpeedRange.y - wSpeedRange.x);

    float norm = length(percent);

    return norm;
}

void main() {
    // texture coordinate must be normalized
    vec3 lonLatLev = texture(currentParticlesPosition, v_textureCoordinates).rgb;
    //vec3 speed = calculateSpeedByRungeKutta2(lonLatLev);
    vec3 speed = calcSpd(lonLatLev);
    vec3 speedInLonLat = convertSpeedUnitToLonLat(lonLatLev, speed);

    float speedScaleFactor = speedFactor * pixelSize;  //<<<<<<<<<<<<<<<<<<<<
    vec4 particleSpeed = vec4(speedInLonLat, calculateWindNorm(speed / speedScaleFactor));
    fragColor = particleSpeed;
}