#version 450

layout(push_constant) uniform Data {
    mat2x4 transform;
    vec4 color_tl;
    vec4 color_tr;
    vec4 color_bl;
    vec4 color_br;
} data;

layout(location = 0) in vec2 position;
layout(location = 1) in vec2 texcoord;

layout(location = 0) out vec4 color;
layout(location = 1) out vec2 outTexcoord;

void main() {
    vec4 left = mix(data.color_tl, data.color_bl, texcoord.y);
    vec4 right = mix(data.color_tr, data.color_br, texcoord.y);
    color = mix(left, right, texcoord.x);

    vec2 pos = vec4(position, 1, 0) * data.transform;
    gl_Position = vec4(vec2(2, 2) * pos.xy + vec2(-1, -1), 0, 1);
    outTexcoord = texcoord;
}
