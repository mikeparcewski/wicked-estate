#version 330 core

uniform mat4 uModelViewProjection;
uniform vec3 uLightDir;

in vec3 aPosition;
in vec3 aNormal;
in vec2 aTexCoord;

out vec3 vNormal;
out vec2 vTexCoord;
out float vDiffuse;

float computeDiffuse(vec3 normal, vec3 lightDir) {
    return max(dot(normalize(normal), normalize(lightDir)), 0.0);
}

vec3 applyGamma(vec3 color, float gamma) {
    return pow(color, vec3(1.0 / gamma));
}

void main() {
    vNormal = aNormal;
    vTexCoord = aTexCoord;
    vDiffuse = computeDiffuse(aNormal, uLightDir);
    gl_Position = uModelViewProjection * vec4(aPosition, 1.0);
}
