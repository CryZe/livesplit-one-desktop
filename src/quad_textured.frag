#version 450

layout(location = 0) in vec4 color;
layout(location = 1) in vec2 texcoord;
layout(location = 0) out vec4 outColor;

layout(binding = 1) uniform texture2D u_texture;
layout(binding = 2) uniform sampler u_sampler;

void main() {
    outColor = color * texture(sampler2D(u_texture, u_sampler), texcoord);
}
