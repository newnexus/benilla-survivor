// A quest trail's footstep: a footprint of the barrier's golden flame burning on the ground. The
// same noise as `barrier.wgsl`, read across the ground instead of up a wall, so neighbouring
// steps burn as one fire; the footprint's edge burns brightest, like the barrier's foot. Added
// onto the ground (the material's `AlphaMode::Add`).
//
// The print's shape is the mask texture's alpha; `tint.a` is the step's strength (the glow
// walking along the trail).

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::globals

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> tint: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var mask_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var mask_sampler: sampler;

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let s = f * f * (3.0 - 2.0 * f);
    let a = hash(i);
    let b = hash(i + vec2<f32>(1.0, 0.0));
    let c = hash(i + vec2<f32>(0.0, 1.0));
    let d = hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, s.x), mix(c, d, s.x), s.y);
}

fn fbm(p: vec2<f32>) -> f32 {
    var v = 0.0;
    var amp = 0.5;
    var q = p;
    for (var k = 0; k < 4; k = k + 1) {
        v = v + amp * noise(q);
        q = q * 2.03 + vec2<f32>(1.7, 9.2);
        amp = amp * 0.5;
    }
    return v;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let t = globals.time;
    let mask = textureSample(mask_texture, mask_sampler, in.uv).a;
    if (mask <= 0.01) {
        discard;
    }
    // The ground under the step, in yards: the fire is laid on the world, not on the print.
    let p = in.world_position.xz * 3.0;

    // A slow warp, then the flame read through it, drifting and licking as on the barrier.
    let warp = fbm(p * 0.8 + vec2<f32>(0.0, -t * 0.35)) - 0.5;
    let rise = fbm(p * 2.2 + warp * 1.6 + vec2<f32>(t * 0.3, -t * 0.9));
    let flicker = 0.7 + 0.6 * fbm(p * 6.0 + vec2<f32>(0.0, -t * 2.4));
    // The body burns unevenly; the edge of the print is the seam that burns brightest.
    // A solid golden print, streaked by the fire.
    var heat = mask * (1.0 + 0.9 * rise * rise) * flicker;
    let edge = 1.0 - abs(mask * 2.0 - 1.0);
    heat = heat + 0.9 * edge * (0.8 + 0.2 * sin(t * 2.0 + p.x * 3.0));
    // Brighter than the barrier: a footprint is small and lies on dark grass.
    heat = heat * tint.a * 1.6;

    // The tint deep at the edges, paling at the hottest, as on the barrier.
    let cool = tint.rgb * 0.4;
    // A warm tint (gold) pales toward white at its hottest; a cool one (the trainer's blue) keeps
    // its colour, else it bleaches white.
    let pale = select(0.08, 0.45, tint.r > tint.b);
    let hot = mix(tint.rgb, vec3<f32>(1.0), pale);
    let colour = mix(cool, hot, clamp(heat * 0.9, 0.0, 1.0)) * heat;
    return vec4<f32>(colour, 0.0);
}
