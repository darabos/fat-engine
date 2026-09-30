#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, globals},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
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

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let m = slime.body_from_world;
    let q = (m * in.world_position).xyz;
    // Transposing the rotation takes body-space vectors back to world space.
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
