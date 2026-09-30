#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::view,
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let depth = -(view.view_from_world * in.world_position).z;
    return vec4(normalize(in.world_normal), depth);
}
