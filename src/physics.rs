use bevy::prelude::*;
use rapier3d::parry::transformation::{MeshEnclosure, VolumeMeshParameters};
use rapier3d::prelude::{
    ColliderBuilder, PhysicsWorld, Pose, Rotation, SoftBodyBuilder, SoftBodyHandle, Vector as RVec,
};

/// The whole-body cluster Rapier creates at insertion.
const ROOT_CLUSTER: u32 = 0;
/// Roughly how many simulated cells span the longest side of a body.
const CELLS_ACROSS: f32 = 4.0;

#[derive(Resource, Default)]
pub struct Physics(pub PhysicsWorld);

/// One render mesh of a body, with the geometry it contributes to the shared skin.
pub struct SkinPart {
    pub mesh: Handle<Mesh>,
    pub positions: Vec<Vec3>,
    pub indices: Vec<u32>,
}

struct SkinSlice {
    mesh: Handle<Mesh>,
    start: usize,
    len: usize,
}

#[derive(Component)]
pub struct SoftBody {
    pub handle: SoftBodyHandle,
    pub position: Vec3,
    pub rotation: Quat,
    /// Rapier centers the rest shape on the cage's center of mass, not on the skin.
    local_center: Vec3,
    slices: Vec<SkinSlice>,
}

pub struct SoftBodyPlugin;

impl Plugin for SoftBodyPlugin {
    fn build(&self, app: &mut App) {
        let mut physics = Physics::default();
        physics.0.insert_collider(
            ColliderBuilder::cuboid(10.0, 0.1, 10.0).translation(RVec::new(0.0, -0.1, 0.0)),
            None,
        );
        app.insert_resource(physics)
            .add_systems(FixedUpdate, step_physics)
            .add_systems(PostUpdate, sync_meshes);
    }
}

fn to_rapier(v: Vec3) -> RVec {
    RVec::new(v.x, v.y, v.z)
}

fn to_bevy(v: RVec) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// Fills the mesh with cells and keeps the mesh itself as the skin riding them.
fn build_cage(
    vertices: &[RVec],
    triangles: &[[u32; 3]],
    cell_size: f32,
) -> Option<SoftBodyBuilder> {
    if let Some(builder) = SoftBodyBuilder::volumetric_skinned(vertices, triangles, cell_size) {
        return Some(builder);
    }
    // A mesh that isn't closed has no inside to fill, so it gets a shell of cells instead.
    let params = VolumeMeshParameters {
        enclosure: MeshEnclosure::Crust,
        ..VolumeMeshParameters::new(cell_size)
    };
    let builder = SoftBodyBuilder::volumetric_with(vertices, triangles, &params)?;
    Some(builder.skin(vertices.to_vec(), triangles.to_vec()))
}

pub fn spawn_soft_body(
    physics: &mut Physics,
    parts: Vec<SkinPart>,
    position: Vec3,
    rotation: Quat,
) -> Option<SoftBody> {
    let mut vertices: Vec<RVec> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut slices = Vec::with_capacity(parts.len());
    let mut minimum = Vec3::splat(f32::MAX);
    let mut maximum = Vec3::splat(f32::MIN);
    for part in parts {
        let start = vertices.len();
        for &p in &part.positions {
            minimum = minimum.min(p);
            maximum = maximum.max(p);
            vertices.push(to_rapier(position + p));
        }
        let offset = start as u32;
        for t in part.indices.chunks_exact(3) {
            triangles.push([t[0] + offset, t[1] + offset, t[2] + offset]);
        }
        slices.push(SkinSlice {
            mesh: part.mesh,
            start,
            len: part.positions.len(),
        });
    }
    if vertices.is_empty() || triangles.is_empty() {
        return None;
    }

    let cell_size = (maximum - minimum).max_element() / CELLS_ACROSS;
    let handle = physics
        .0
        .insert_soft_body(build_cage(&vertices, &triangles, cell_size)?);

    let body = &physics.0.soft_bodies[handle];
    let cage_center =
        body.particle_positions().map(to_bevy).sum::<Vec3>() / body.num_particles() as f32;
    Some(SoftBody {
        handle,
        position,
        rotation,
        local_center: cage_center - position,
        slices,
    })
}

fn step_physics(time: Res<Time<Fixed>>, mut physics: ResMut<Physics>, bodies: Query<&SoftBody>) {
    let world = &mut physics.0;
    world.integration_parameters.dt = time.delta_secs();
    for body in &bodies {
        let r = body.rotation;
        let target = Pose::from_parts(
            to_rapier(body.position + r * body.local_center),
            Rotation::from_xyzw(r.x, r.y, r.z, r.w),
        );
        if let Some(cluster) = world.soft_bodies[body.handle].cluster_mut(ROOT_CLUSTER) {
            cluster.set_shape_matching_target(Some(target));
        }
    }
    world.step();
}

fn sync_meshes(physics: Res<Physics>, bodies: Query<&SoftBody>, mut meshes: ResMut<Assets<Mesh>>) {
    for body in &bodies {
        let soft_body = &physics.0.soft_bodies[body.handle];
        let Some(skin) = soft_body.meshes().find(|mesh| mesh.is_skinned()) else {
            continue;
        };
        let positions: Vec<[f32; 3]> = skin
            .vertex_positions(soft_body)
            .map(|p| to_bevy(p).to_array())
            .collect();
        for slice in &body.slices {
            let Some(mesh) = meshes.get_mut(&slice.mesh) else {
                continue;
            };
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_POSITION,
                positions[slice.start..slice.start + slice.len].to_vec(),
            );
            mesh.compute_normals();
        }
    }
}
