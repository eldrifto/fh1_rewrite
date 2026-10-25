// FM2 projected car shadow (the game's `RenderProjShadow` pass, effect @82103d90 in FM2's default.xex; STATUS.md 2026-10-26):
// the ground near the car is multiplied towards the track's ShadowColor where the car blocks the light, ON TOP of the scenery
// shadow (the car is also a caster there; the game darkens its shadow twice). The game tests a variance shadow map of the car;
// here the car's top-down body silhouette (drop_shadow.rs) is sampled at a few heights along the ray towards the light.

#import bevy_pbr::mesh_view_bindings::view

struct ProjShadowParams {
    // World -> car-local (the car entity's space, +Y up).
    car_from_world: mat4x4<f32>,
    // xyz = direction towards the light, car-local; w = n.l weight of a flat ground (max(light.y, 0) in world).
    to_light: vec4<f32>,
    // Silhouette bounds in car-local x / z: (min.x, min.z, max.x, max.z).
    bounds: vec4<f32>,
    // x = body bottom, y = body top (car-local y), z = sample count, w = -.
    heights: vec4<f32>,
    // rgb = ShadowColor, w = strength (1 = the game's full multiply).
    colour: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var mask: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var samp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> g: ProjShadowParams;

struct In {
    @location(0) pos: vec3<f32>,
}

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
}

@vertex
fn vertex(v: In) -> Out {
    var o: Out;
    o.clip = view.clip_from_world * vec4<f32>(v.pos, 1.0);
    o.world = v.pos;
    return o;
}

@fragment
fn fragment(i: Out) -> @location(0) vec4<f32> {
    let p = (g.car_from_world * vec4<f32>(i.world, 1.0)).xyz;
    let l = g.to_light.xyz;
    var occ = 0.0;
    if (l.y > 0.02) {
        let n = max(g.heights.z, 1.0);
        for (var k = 0.0; k < n; k = k + 1.0) {
            let h = mix(g.heights.x, g.heights.y, (k + 0.5) / n);
            // The point on the ray from the ground towards the light at body height h.
            let q = p + l * ((h - p.y) / l.y);
            let uv = (q.xz - g.bounds.xy) / max(g.bounds.zw - g.bounds.xy, vec2<f32>(1e-3));
            if (all(uv >= vec2<f32>(0.0)) && all(uv <= vec2<f32>(1.0))) {
                occ = max(occ, textureSampleLevel(mask, samp, uv, 0.0).r);
            }
        }
    }
    let k = clamp(occ * g.colour.w * g.to_light.w, 0.0, 1.0);
    // Multiply blend (dst * src): white where the light passes.
    return vec4<f32>(mix(vec3<f32>(1.0), g.colour.rgb, k), 1.0);
}
