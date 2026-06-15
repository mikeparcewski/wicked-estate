cbuffer PerFrame : register(b0)
{
    float4x4 gViewProj;
    float3   gLightDir;
    float    gTime;
};

struct VertexIn
{
    float3 Pos    : POSITION;
    float3 Normal : NORMAL;
    float2 Tex    : TEXCOORD;
};

struct VertexOut
{
    float4 PosH   : SV_POSITION;
    float3 Normal : NORMAL;
    float2 Tex    : TEXCOORD;
};

float3 ComputeDiffuse(float3 normal, float3 lightDir)
{
    return max(dot(normalize(normal), normalize(lightDir)), 0.0f) * float3(1, 1, 1);
}

float4 ToClipSpace(float3 pos, float4x4 vp)
{
    return mul(float4(pos, 1.0f), vp);
}

VertexOut VS(VertexIn vin)
{
    VertexOut vout;
    vout.PosH   = ToClipSpace(vin.Pos, gViewProj);
    vout.Normal = vin.Normal;
    vout.Tex    = vin.Tex;
    return vout;
}
