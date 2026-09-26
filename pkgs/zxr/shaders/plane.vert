#version 450
// A unit quad in the XZ... no: in XY, centred, facing +Z; the push constant carries the full MVP
// for this view and this plane, plus the plane's half extents in metres.
layout(push_constant) uniform PC {
    mat4 mvp;
    vec2 half_size;
    vec2 uv_flip; // (0|1, 0|1): flip u/v for buffer transforms
} pc;
layout(location = 0) out vec2 v_uv;
void main() {
    // two triangles, 6 vertices
    const vec2 corners[6] = vec2[](
        vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
        vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0));
    vec2 c = corners[gl_VertexIndex];
    vec2 uv = c * 0.5 + 0.5;
    uv.y = 1.0 - uv.y; // Wayland buffers are top-down
    v_uv = mix(uv, 1.0 - uv, pc.uv_flip);
    gl_Position = pc.mvp * vec4(c * pc.half_size, 0.0, 1.0);
}
