#include <metal_stdlib>
using namespace metal;

struct ViewportUniforms {
    float2 scale;
    float2 offset;
};

struct VertexOut {
    float4 position [[position]];
    float2 texCoord;
};

vertex VertexOut fujihimeVertex(
    uint vertexId [[vertex_id]],
    constant ViewportUniforms &uniforms [[buffer(0)]]
) {
    // Triangle strip: bottom-left, bottom-right, top-left, top-right.
    const float2 quadPositions[4] = {
        float2(-1.0, -1.0),
        float2( 1.0, -1.0),
        float2(-1.0,  1.0),
        float2( 1.0,  1.0)
    };
    const float2 texCoords[4] = {
        float2(0.0, 1.0),
        float2(1.0, 1.0),
        float2(0.0, 0.0),
        float2(1.0, 0.0)
    };

    VertexOut out;
    float2 pos = quadPositions[vertexId] * uniforms.scale + uniforms.offset;
    out.position = float4(pos, 0.0, 1.0);
    out.texCoord = texCoords[vertexId];
    return out;
}

fragment half4 fujihimeFragment(
    VertexOut in [[stage_in]],
    texture2d<half, access::sample> colorTexture [[texture(0)]]
) {
    constexpr sampler texSampler(
        coord::normalized,
        address::clamp_to_edge,
        filter::linear
    );
    half4 color = colorTexture.sample(texSampler, in.texCoord);
    return half4(color.rgb, 1.0h);
}
