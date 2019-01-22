#version 450

layout(location = 0) in vec4 color;
layout(location = 1) in vec2 texcoord;
layout(location = 0) out vec4 outColor;

void main() {
    outColor = color;
}
