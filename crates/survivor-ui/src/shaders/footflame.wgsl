// The flames rising from a quest trail's footstep: the barrier's flame (`barrier.wgsl`) on a
// short ring round the footprint, without the barrier's close-up veil (the camera is always near
// a step). Added onto what is behind (`AlphaMode::Add`).
//
// uv.x runs round the ring (one unit a tongue or so), uv.y from the ground (0) to the top (1);
// The second uv's x is how near the fire's strong end of the print a point is (0 to 1): there
// the tongues reach full height and burn hottest, dying away round to the other end.
// `tint.a` is the step's strength (the glow walking along the trail).

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::globals

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> tint: vec4<f32>;

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
    // Each step's fire starts somewhere else in the noise, so neighbours never burn alike.
    let u = in.uv.x + in.world_position.x * 1.7 + in.world_position.z * 2.3;
    // Round the ring the fire dies away from its strong end: shorter, then gone.
    let strong = smoothstep(0.1, 1.0, in.uv_b.x);
    let v = clamp(in.uv.y / max(0.25 + 0.75 * strong, 0.01), 0.0, 1.0);

    // As on the barrier: a slow warp, then tongues read through it, licking upward.
    let warp = fbm(vec2<f32>(u * 0.9, v * 1.5 - t * 0.5)) - 0.5;
    let rise = fbm(vec2<f32>(u * 2.2 + warp * 1.6, v * 3.0 - t * 1.4));
    let reach = 0.15 + 0.85 * rise * rise;
    let body = 1.0 - smoothstep(reach * 0.55, reach, v + warp * 0.25);
    let fade = pow(1.0 - v, 1.4);
    let flicker = 0.7 + 0.6 * fbm(vec2<f32>(u * 6.0, v * 8.0 - t * 3.0));
    var heat = body * fade * flicker * 0.9;
    heat = heat + 0.6 * (1.0 - smoothstep(0.0, 0.12, v));
    // Brighter than the barrier: the tongues are small.
    heat = heat * strong * tint.a * 1.7;

    let cool = tint.rgb * 0.4;
    let hot = mix(tint.rgb, vec3<f32>(1.0), 0.45);
    let colour = mix(cool, hot, clamp(heat * 0.9, 0.0, 1.0)) * heat;
    return vec4<f32>(colour, 0.0);
}
