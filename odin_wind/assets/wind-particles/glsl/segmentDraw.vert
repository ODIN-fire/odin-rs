//#version 300 es

in vec2 st;
// it is not normal itself, but used to control lines drawing
in vec3 normal; // (point to use, offset sign, not used component)



uniform sampler2D H; // grid cell height
uniform sampler2D U; // eastward wind (for wind speed colors)
uniform sampler2D V; // northward wind
uniform sampler2D W; // up/down wind
uniform float particleHeight; //  extra height we can interactively set

// particle colors - either a single color or by wind speed (in 5mph steps, same as vector and contour displays)
#define MAX_SPEED_COLORS 16
const float SPEED_COLOR_STEP = 2.2352; // 5mph in m/s
uniform vec4 color;
uniform vec4 speedColors[MAX_SPEED_COLORS];
uniform float nSpeedColors; // number of valid speedColors entries
uniform float colorBySpeed; // > 0.5 means use speedColors

// wind speed dependent line width: the width grows linearly from lineWidth (no wind) to maxWidthFactor * lineWidth
// (at widthSpeedMax and above) to make higher wind speeds more pronounced
uniform float widthBySpeed;   // > 0.5 means scale line width by wind speed
uniform float maxWidthFactor; // max line width scale factor
uniform float widthSpeedMax;  // wind speed [m/s] at which we reach maxWidthFactor
uniform float edgeDarkening;  // [0..1] darkening of line edges (only for lines wide enough to show edges)

out vec4 v_color;
out float v_side; // -1..1 across the line width (for edge darkening in segmentDraw.frag)
out float v_edge; // edge darkening for this line

float widthFactor = 1.0; // set in main() for the current particle (has to be the same for all segment vertices)

uniform sampler2D previousParticlesPosition;
uniform sampler2D currentParticlesPosition;
uniform sampler2D postProcessingPosition;

uniform float aspect;
uniform float pixelSize;
uniform float lineWidth;

uniform vec3 minimum; // minimum of each dimension
uniform vec3 maximum; // maximum of each dimension

// required to interpolate height
uniform vec3 dimension; // (lon, lat, lev)
uniform vec3 interval; // interval of each dimension

struct adjacentPoints {
    vec4 previous;
    vec4 current;
    vec4 next;
};

//--- height interpolation (unfortunately somewhat redundant with calculateSpeed.frag)

// NOTE - this has to be consistent with the grid lookup in calculateSpeed.frag (see comments there)
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

float interpolateGrid(sampler2D componentTexture, vec3 lonLatLev) {
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

float interpolateHeight(vec3 lonLatLev) {
    return interpolateGrid(H, lonLatLev);
}

float windSpeed(vec3 lonLatLev) {
    vec3 wind = vec3( interpolateGrid(U, lonLatLev), interpolateGrid(V, lonLatLev), interpolateGrid(W, lonLatLev));
    return length(wind);
}

vec4 particleColor(float speed) {
    if (colorBySpeed > 0.5 && nSpeedColors > 0.5) {
        int idx = int( min( floor( speed / SPEED_COLOR_STEP), nSpeedColors - 1.0));
        return speedColors[idx];
    } else {
        return color;
    }
}

float particleWidthFactor(float speed) {
    if (widthBySpeed > 0.5 && widthSpeedMax > 0.0) {
        return 1.0 + (max(maxWidthFactor, 1.0) - 1.0) * clamp( speed / widthSpeedMax, 0.0, 1.0);
    } else {
        return 1.0;
    }
}

float getHeight (vec3 lonLatLev) {
    return interpolateHeight(lonLatLev) + particleHeight; // particleHeight is just extra, on top of wind/terrain height
}


vec3 convertCoordinate(vec3 lonLatLev) {
    // WGS84 (lon, lat, lev) -> ECEF (x, y, z)
    // read https://en.wikipedia.org/wiki/Geographic_coordinate_conversion#From_geodetic_to_ECEF_coordinates for detail

    // WGS 84 geometric constants 
    float a = 6378137.0; // Semi-major axis 
    float b = 6356752.3142; // Semi-minor axis 
    float e2 = 6.69437999014e-3; // First eccentricity squared

    float latitude = radians(lonLatLev.y);
    float longitude = radians(lonLatLev.x);

    float cosLat = cos(latitude);
    float sinLat = sin(latitude);
    float cosLon = cos(longitude);
    float sinLon = sin(longitude);

    float N_Phi = a / sqrt(1.0 - e2 * sinLat * sinLat);
    float h = getHeight(lonLatLev); 
    float c = (N_Phi + h) * cosLat;

    vec3 cartesian = vec3(0.0);
    cartesian.x = c * cosLon;
    cartesian.y = c * sinLon;
    cartesian.z = ((N_Phi * (b * b) / (a * a)) + h) * sinLat;
    return cartesian;
}

vec4 calculateProjectedCoordinate(vec3 lonLatLev) {
    // the range of longitude in Cesium is [-180, 180] but the range of longitude in the NetCDF file is [0, 360]
    // [0, 180] is corresponding to [0, 180] and [180, 360] is corresponding to [-180, 0]
    lonLatLev.x = mod(lonLatLev.x + 180.0, 360.0) - 180.0;
    vec3 particlePosition = convertCoordinate(lonLatLev);

    // czm_modelViewProjection: automatic GLSL uniform representing a 4x4 model-view-projection transformation matrix 
    // that transforms model coordinates to clip coordinates
    vec4 projectedCoordinate = czm_modelViewProjection * vec4(particlePosition, 1.0);
    return projectedCoordinate;
}

vec4 calculateOffsetOnNormalDirection(vec4 pointA, vec4 pointB, float offsetSign) {
    vec2 aspectVec2 = vec2(aspect, 1.0);
    vec2 pointA_XY = (pointA.xy / pointA.w) * aspectVec2;
    vec2 pointB_XY = (pointB.xy / pointB.w) * aspectVec2;

    float offsetLength = lineWidth * widthFactor / 2.0;
    vec2 direction = normalize(pointB_XY - pointA_XY);
    vec2 normalVector = vec2(-direction.y, direction.x);
    normalVector.x = normalVector.x / aspect;
    normalVector = offsetLength * normalVector;

    vec4 offset = vec4(offsetSign * normalVector, 0.0, 0.0);
    return offset;
}

vec4 calculateOffsetOnMiterDirection(adjacentPoints projectedCoordinates, float offsetSign) {
    vec2 aspectVec2 = vec2(aspect, 1.0);

    vec4 PointA = projectedCoordinates.previous;
    vec4 PointB = projectedCoordinates.current;
    vec4 PointC = projectedCoordinates.next;

    vec2 pointA_XY = (PointA.xy / PointA.w) * aspectVec2;
    vec2 pointB_XY = (PointB.xy / PointB.w) * aspectVec2;
    vec2 pointC_XY = (PointC.xy / PointC.w) * aspectVec2;

    vec2 AB = normalize(pointB_XY - pointA_XY);
    vec2 BC = normalize(pointC_XY - pointB_XY);

    vec2 normalA = vec2(-AB.y, AB.x);
    vec2 tangent = normalize(AB + BC);
    vec2 miter = vec2(-tangent.y, tangent.x);

    float offsetLength = lineWidth * widthFactor / 2.0;
    float projection = dot(miter, normalA);
    vec4 offset = vec4(0.0);
    // avoid to use values that are too small
    if (projection > 0.1) {
        float miterLength = offsetLength / projection;
        offset = vec4(offsetSign * miter * miterLength, 0.0, 0.0);
        offset.x = offset.x / aspect;
    } else {
        offset = calculateOffsetOnNormalDirection(PointB, PointC, offsetSign);
    }

    return offset;
}

void main() {
    vec2 particleIndex = st;

    vec3 previousPosition = texture(previousParticlesPosition, particleIndex).rgb;
    vec3 currentPosition = texture(currentParticlesPosition, particleIndex).rgb;
    vec3 nextPosition = texture(postProcessingPosition, particleIndex).rgb;

    // speed dependent color and width - computed for the current position so that all segment vertices use the same values
    float speed = windSpeed(currentPosition);
    v_color = particleColor(speed);
    widthFactor = particleWidthFactor(speed);

    // edges are only visible (and hence only darkened) for lines that are at least a few pixels wide
    v_edge = edgeDarkening * smoothstep( 3.0, 6.0, lineWidth * widthFactor);

    float isAnyRandomPointUsed = texture(postProcessingPosition, particleIndex).a +
        texture(currentParticlesPosition, particleIndex).a +
        texture(previousParticlesPosition, particleIndex).a;

    adjacentPoints projectedCoordinates;
    if (isAnyRandomPointUsed > 0.0) {
        projectedCoordinates.previous = calculateProjectedCoordinate(previousPosition);
        projectedCoordinates.current = projectedCoordinates.previous;
        projectedCoordinates.next = projectedCoordinates.previous;
    } else {
        projectedCoordinates.previous = calculateProjectedCoordinate(previousPosition);
        projectedCoordinates.current = calculateProjectedCoordinate(currentPosition);
        projectedCoordinates.next = calculateProjectedCoordinate(nextPosition);
    }

    int pointToUse = int(normal.x);
    float offsetSign = normal.y;
    v_side = offsetSign;
    vec4 offset = vec4(0.0);
    // render lines with triangles and miter joint
    // read https://blog.scottlogic.com/2019/11/18/drawing-lines-with-webgl.html for detail
    if (pointToUse == -1) {
        offset = pixelSize * calculateOffsetOnNormalDirection(projectedCoordinates.previous, projectedCoordinates.current, offsetSign);
        gl_Position = projectedCoordinates.previous + offset;
    } else {
        if (pointToUse == 0) {
            offset = pixelSize * calculateOffsetOnMiterDirection(projectedCoordinates, offsetSign);
            gl_Position = projectedCoordinates.current + offset;
        } else {
            if (pointToUse == 1) {
                offset = pixelSize * calculateOffsetOnNormalDirection(projectedCoordinates.current, projectedCoordinates.next, offsetSign);
                gl_Position = projectedCoordinates.next + offset;
            } else {

            }
        }
    }

    // simplistic clipping to (geographic) wind field area
    float x = currentPosition.x;
    float y = currentPosition.y;
    if (x < minimum.x || x > maximum.x || y < minimum.y || y > maximum.y) {
        gl_Position.w = 0.0;
    }
}
