// The town barrier: a wall of golden flame. Strongest at its foot, fading as it rises, its
// tongues licking upward and warping slowly over time. Added onto what is behind it (the
// material's `AlphaMode::Add`), so the world shows through.
//
// uv.x runs around the wall (one unit per few yards), uv.y from the foot (0) to the top (1).
// The tint's alpha is how many times finer the flame's pattern runs up the wall than a town
// barrier's: a taller wall with narrower tongues keeps their shape instead of stretching them.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::{globals, view}

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
    let u = in.uv.x;
    let v = clamp(in.uv.y, 0.0, 1.0);
    // The pattern's height, in the same units as a town barrier's.
    let p = v * max(tint.a, 1.0);

    // A slow warp first, then the flame read through it: the tongues sway and bend as they rise.
    let warp = fbm(vec2<f32>(u * 0.9, p * 1.5 - t * 0.35)) - 0.5;
    let rise = fbm(vec2<f32>(u * 2.2 + warp * 1.6, p * 3.0 - t * 0.9));

    // The flame's edge: high where the noise is high, so tongues reach up unevenly.
    let reach = 0.1 + 0.9 * rise * rise;
    let body = 1.0 - smoothstep(reach * 0.55, reach, v + warp * 0.25);
    // Stronger at the foot, fading as it rises.
    let fade = pow(1.0 - v, 1.6);
    // Flicker: fine noise racing upward.
    let flicker = 0.7 + 0.6 * fbm(vec2<f32>(u * 6.0, p * 8.0 - t * 2.4));
    var heat = body * fade * flicker * 0.6;
    // The seam where the wall meets the ground burns brightest.
    heat = heat + 0.7 * (1.0 - smoothstep(0.0, 0.06, v)) * (0.8 + 0.2 * sin(t * 2.0 + u * 3.0));

    // The tint deep at the edges, paling at the hottest.
    let cool = tint.rgb * 0.4;
    let hot = mix(tint.rgb, vec3<f32>(1.0), 0.45);
    var colour = mix(cool, hot, clamp(heat * 0.9, 0.0, 1.0)) * heat;
    // Up close the wall thins to a veil, so a camera at the barrier still sees the world.
    let near = distance(in.world_position.xyz, view.world_position);
    colour = colour * mix(0.15, 1.0, smoothstep(4.0, 22.0, near));
    // See-through: the town behind the wall must stay readable. Bevy draws `AlphaMode::Add` as
    // a premultiplied blend, so alpha 0 adds the colour and keeps all of what is behind.
    return vec4<f32>(colour * 0.55, 0.0);
}
