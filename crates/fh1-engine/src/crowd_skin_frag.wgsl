// GPU crowd figures, main-pass fragment (MODLOG 125). Bevy's StandardMaterial forward fragment, plus one multiplier: the figure's
// slot (its MeshTag) holds, in the w of the third vec4, the tint of that spectator over the map's mean tint (the material's base
// colour already carries the mean); 0 = none. See crowd/figures_gpu.rs `slot_bytes` and crowd.rs `tint_ratio`.

#import bevy_pbr::{
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
    forward_io::{VertexOutput, FragmentOutput},
    mesh_functions,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<storage, read> crowd_figs: array<vec4<f32>>;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let slot = mesh_functions::get_tag(in.instance_index);
    let ratio = crowd_figs[slot * 3u + 2u].w;
    if (ratio > 0.0) {
        pbr_input.material.base_color = vec4<f32>(pbr_input.material.base_color.rgb * ratio, pbr_input.material.base_color.a);
    }
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
