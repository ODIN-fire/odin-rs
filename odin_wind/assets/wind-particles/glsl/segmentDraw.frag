//#version 300 es

in vec4 v_color; // single or wind speed color (computed in segmentDraw.vert)
out vec4 fragColor;

void main() {
    fragColor = v_color;
}
