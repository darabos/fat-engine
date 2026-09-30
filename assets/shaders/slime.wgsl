#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_functions,
    mesh_view_bindings::{view, globals},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    view_transformations::position_world_to_clip,
}

// forward_io's Vertex and VertexOutput, plus the rest position.
struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
#ifdef VERTEX_UVS_A
    @location(2) uv: vec2<f32>,
#endif
#ifdef VERTEX_UVS_B
    @location(3) uv_b: vec2<f32>,
#endif
#ifdef VERTEX_TANGENTS
    @location(4) tangent: vec4<f32>,
#endif
#ifdef VERTEX_COLORS
    @location(5) color: vec4<f32>,
#endif
    @location(8) rest_position: vec3<f32>,
}

struct SlimeVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
#ifdef VERTEX_UVS_A
    @location(2) uv: vec2<f32>,
#endif
#ifdef VERTEX_UVS_B
    @location(3) uv_b: vec2<f32>,
#endif
#ifdef VERTEX_TANGENTS
    @location(4) world_tangent: vec4<f32>,
#endif
#ifdef VERTEX_COLORS
    @location(5) color: vec4<f32>,
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    @location(6) @interpolate(flat) instance_index: u32,
#endif
#ifdef VISIBILITY_RANGE_DITHER
    @location(7) @interpolate(flat) visibility_range_dither: i32,
#endif
    @location(8) rest_position: vec3<f32>,
}

struct SlimeParams {
    body_from_world: mat4x4<f32>,
    wart_spacing: f32,
    wart_height: f32,
    blotch_scale: f32,
    blotch_darkness: f32,
    ripple_scale: f32,
    ripple_strength: f32,
    flow_speed: f32,
    translucency_depth: f32,
    reflection: f32,
}

@group(2) @binding(100) var<uniform> slime: SlimeParams;
@group(2) @binding(101) var back_faces: texture_2d<f32>;

fn hash33(p: vec3<f32>) -> vec3<f32> {
    var q = fract(p * vec3(0.1031, 0.1030, 0.0973));
    q += dot(q, q.yxz + 33.33);
    return fract((q.xxy + q.yxx) * q.zyx);
}

fn value_noise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(hash33(i).x, hash33(i + vec3(1.0, 0.0, 0.0)).x, u.x);
    let b = mix(hash33(i + vec3(0.0, 1.0, 0.0)).x, hash33(i + vec3(1.0, 1.0, 0.0)).x, u.x);
    let c = mix(hash33(i + vec3(0.0, 0.0, 1.0)).x, hash33(i + vec3(1.0, 0.0, 1.0)).x, u.x);
    let d = mix(hash33(i + vec3(0.0, 1.0, 1.0)).x, hash33(i + vec3(1.0, 1.0, 1.0)).x, u.x);
    return mix(mix(a, b, u.y), mix(c, d, u.y), u.z);
}

fn fbm(p: vec3<f32>) -> f32 {
    return 0.5 * value_noise(p) + 0.3 * value_noise(p * 2.03 + 11.7) + 0.2 * value_noise(p * 4.01 + 23.1);
}

// Distance to the nearest feature point (w) and the unit direction from it to q (xyz).
fn worley(q: vec3<f32>) -> vec4<f32> {
    let cell = floor(q);
    var best = 1e9;
    var nearest = vec3(0.0);
    for (var x = -1; x <= 1; x++) {
        for (var y = -1; y <= 1; y++) {
            for (var z = -1; z <= 1; z++) {
                let c = cell + vec3(f32(x), f32(y), f32(z));
                let p = c + 0.15 + 0.7 * hash33(c);
                let d = length(q - p);
                if d < best {
                    best = d;
                    nearest = p;
                }
            }
        }
    }
    return vec4(normalize(q - nearest + 1e-5), best);
}

// Tilts n against a surface gradient given in world space.
fn bend(n: vec3<f32>, gradient: vec3<f32>) -> vec3<f32> {
    return normalize(n - (gradient - n * dot(gradient, n)));
}

@vertex
fn vertex(vertex: Vertex) -> SlimeVertexOutput {
    var out: SlimeVertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4(vertex.position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, vertex.tangent, vertex.instance_index);
#endif
#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(vertex.instance_index, world_from_local[3]);
#endif
    out.rest_position = vertex.rest_position;
    return out;
}

@fragment
fn fragment(slime_in: SlimeVertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var in: VertexOutput;
    in.position = slime_in.position;
    in.world_position = slime_in.world_position;
    in.world_normal = slime_in.world_normal;
#ifdef VERTEX_UVS_A
    in.uv = slime_in.uv;
#endif
#ifdef VERTEX_UVS_B
    in.uv_b = slime_in.uv_b;
#endif
#ifdef VERTEX_TANGENTS
    in.world_tangent = slime_in.world_tangent;
#endif
#ifdef VERTEX_COLORS
    in.color = slime_in.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    in.instance_index = slime_in.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    in.visibility_range_dither = slime_in.visibility_range_dither;
#endif
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let m = slime.body_from_world;
    let q = slime_in.rest_position;
    // The rest pose shares the body's orientation, so its rotation takes pattern gradients to world space.
    let world_from_body = transpose(mat3x3(m[0].xyz, m[1].xyz, m[2].xyz));

    // Warts: a smooth dome around each Worley feature point.
    let cell = worley(q / slime.wart_spacing);
    let t = clamp(cell.w / 0.5, 0.0, 1.0);
    let wart = 1.0 - t * t * (3.0 - 2.0 * t);
    let wart_slope = -6.0 * t * (1.0 - t) / 0.5;
    let wart_gradient = world_from_body * (cell.xyz * wart_slope * slime.wart_height);

    // Slime ripples creep downward over time; only the clear coat sees them.
    let e = 0.05;
    let flow = q / slime.ripple_scale + vec3(0.0, globals.time * slime.flow_speed / slime.ripple_scale, 0.0);
    let r0 = fbm(flow);
    let ripple = vec3(
        fbm(flow + vec3(e, 0.0, 0.0)) - r0,
        fbm(flow + vec3(0.0, e, 0.0)) - r0,
        fbm(flow + vec3(0.0, 0.0, e)) - r0,
    ) / e;
    let ripple_gradient = world_from_body * (ripple * slime.ripple_strength);

    let skin_n = bend(pbr_input.N, wart_gradient);
    pbr_input.N = skin_n;
    pbr_input.clearcoat_N = bend(skin_n, ripple_gradient * 0.5 + wart_gradient * -0.3);

    let blotch = smoothstep(0.5, 0.62, fbm(q / slime.blotch_scale + 7.1));
    let tint = (1.0 - slime.blotch_darkness * blotch) * (1.0 + 0.15 * wart);
    pbr_input.material.base_color = vec4(pbr_input.material.base_color.rgb * tint, pbr_input.material.base_color.a);
    // Where the slime layer thins out the skin underneath looks drier.
    pbr_input.material.perceptual_roughness *= mix(0.6, 1.3, r0);

    // Light shines through where the back face is close behind the front face.
    let eye = view.world_position;
    let ray = normalize(in.world_position.xyz - eye);
    let forward = -view.world_from_view[2].xyz;
    let front_distance = length(in.world_position.xyz - eye);
    var thickness = 1e3;
    let back_texel = textureLoad(back_faces, vec2<i32>(in.position.xy), 0);
    if back_texel.w > 0.0 {
        let back_distance = back_texel.w / dot(ray, forward);
        if back_distance > front_distance {
            thickness = back_distance - front_distance;
        }
    }
    pbr_input.material.thickness = min(thickness, 1.0);
    pbr_input.material.diffuse_transmission *= exp(-thickness / slime.translucency_depth);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);

    // There is no environment map, so fake a sky and a bright window for the wet coat to reflect.
    let cn = pbr_input.clearcoat_N;
    let r = reflect(ray, cn);
    let sky = mix(vec3(0.2, 0.18, 0.15), vec3(0.6, 0.75, 1.0), smoothstep(-0.2, 0.6, r.y));
    let window = pow(max(dot(r, normalize(vec3(0.3, 1.0, 0.2))), 0.0), 80.0) * 6.0;
    let fresnel = 0.04 + 0.96 * pow(1.0 - max(dot(cn, -ray), 0.0), 5.0);
    out.color += vec4((sky + window) * fresnel * slime.reflection * view.exposure, 0.0);

    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
