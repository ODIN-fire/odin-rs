//#version 300 es

in vec4 v_color; // single or wind speed color (computed in segmentDraw.vert)
in float v_side; // -1..1 across the line width
in float v_edge; // edge darkening for this line [0..1]
out vec4 fragColor;

void main() {
    // darken the edges of (wide) lines so that they stand out against any background
    float edge = smoothstep( 0.5, 1.0, abs(v_side));
    fragColor = vec4( v_color.rgb * (1.0 - v_edge * edge), v_color.a);
}
