#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}

struct FurParams {
    body_from_world: mat4x4<f32>,
    fur_depth: f32,
    strand_spacing: f32,
    strand_radius: f32,
    root_shade: f32,
    messiness: f32,
}

@group(2) @binding(100) var<uniform> fur: FurParams;
@group(2) @binding(101) var back_faces: texture_2d<f32>;

const MAX_STEPS: i32 = 16;

fn hash33(p: vec3<f32>) -> vec3<f32> {
    var q = fract(p * vec3(0.1031, 0.1030, 0.0973));
    q += dot(q, q.yxz + 33.33);
    return fract((q.xxy + q.yxx) * q.zyx);
}

// One hair of length strand_spacing grows from a jittered root in every cell of a body-space grid.
// Returns the distance to the nearest hair and how far along it (0 root, 1 tip) that point is.
fn hairs(q: vec3<f32>, direction: vec3<f32>) -> vec2<f32> {
    let s = fur.strand_spacing;
    // Only the 2x2x2 cells nearest to q; hairs rooted further away are rarely the closest.
    let cell = floor(q / s - 0.5);
    var best = vec2(1e9, 0.0);
    for (var x = 0; x <= 1; x++) {
        for (var y = 0; y <= 1; y++) {
            for (var z = 0; z <= 1; z++) {
                let c = cell + vec3(f32(x), f32(y), f32(z));
                let r = hash33(c);
                let root = (c + r) * s;
                let dir = normalize(direction + (fract(r * 17.31) - 0.5) * fur.messiness);
                let rel = q - root;
                let t = clamp(dot(rel, dir), 0.0, s);
                let along = t / s;
                let dist = length(rel - dir * t) - fur.strand_radius * (1.0 - 0.7 * along);
                if dist < best.x {
                    best = vec2(dist, along);
                }
            }
        }
    }
    return best;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let base = pbr_input.material.base_color.rgb;

    let front = in.world_position.xyz;
    let n_front = normalize(in.world_normal);
    let eye = view.world_position;
    let ray = normalize(front - eye);
    let forward = -view.world_from_view[2].xyz;
    let front_distance = length(front - eye);

    var n_back = -n_front;
    var thickness = fur.fur_depth * 4.0;
    let back_texel = textureLoad(back_faces, vec2<i32>(in.position.xy), 0);
    if back_texel.w > 0.0 {
        let back_distance = back_texel.w / dot(ray, forward);
        if back_distance > front_distance {
            thickness = back_distance - front_distance;
            n_back = normalize(back_texel.xyz);
        }
    }
    let back = front + ray * thickness;

    let pixel = front_distance * 2.0 / (view.clip_from_view[1][1] * view.viewport.w);
    let min_step = fur.fur_depth / 6.0;
    var col = vec4(0.0);
    var hair_normal = vec3(0.0);
    var d = 0.0;
    for (var i = 0; i < MAX_STEPS; i++) {
        if col.a > 0.99 || d > thickness {
            break;
        }
        let p = front + ray * d;
        // Distances below the tangent planes at the entry and exit points; the nearer one wins.
        let depth_front = dot(front - p, n_front);
        let depth_back = dot(back - p, n_back);
        let depth = min(depth_front, depth_back);
        if depth > fur.fur_depth {
            let a = 1.0 - col.a;
            col += vec4(base * fur.root_shade * a, a);
            hair_normal += n_front * a;
            break;
        }
        let w = clamp(depth_front / max(depth_front + depth_back, 1e-4), 0.0, 1.0);
        let h = normalize(mix(n_front, n_back, w) + 1e-3 * n_front);

        let q = (fur.body_from_world * vec4(p, 1.0)).xyz;
        let local_h = (fur.body_from_world * vec4(h, 0.0)).xyz;
        let hair = hairs(q, local_h);
        let coverage = clamp(0.5 - hair.x / pixel, 0.0, 1.0);
        if coverage > 0.0 {
            let a = (1.0 - col.a) * coverage;
            let depth01 = clamp(depth / fur.fur_depth, 0.0, 1.0);
            let shade = mix(1.0, fur.root_shade, depth01) * mix(0.9, 1.1, hair.y);
            col += vec4(base * shade * a, a);
            hair_normal += h * a;
        }
        d += max(hair.x * 0.7, min_step);
    }

    if col.a < 0.004 {
        discard;
    }
    pbr_input.material.base_color = vec4(col.rgb / col.a, col.a);
    pbr_input.N = normalize(pbr_input.N + normalize(hair_normal));

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
