use bevy::prelude::*;
use rapier3d::prelude::{
    ColliderBuilder, PhysicsWorld, Pose, Rotation, SoftBodyBuilder, SoftBodyHandle, Vector as RVec,
};

/// The whole-body cluster Rapier creates at insertion.
const ROOT_CLUSTER: u32 = 0;

#[derive(Resource, Default)]
pub struct Physics(pub PhysicsWorld);

#[derive(Component)]
pub struct SoftBody {
    pub handle: SoftBodyHandle,
    pub mesh: Handle<Mesh>,
    pub position: Vec3,
    pub rotation: Quat,
    /// Rapier's rest shape is centered on the particles' center of mass.
    local_center: Vec3,
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

pub fn spawn_soft_body(
    physics: &mut Physics,
    positions: &[Vec3],
    indices: &[u32],
    position: Vec3,
    rotation: Quat,
    mesh: Handle<Mesh>,
) -> Option<SoftBody> {
    let vertices = positions.iter().map(|&p| to_rapier(position + p)).collect();
    let triangles = indices
        .chunks_exact(3)
        .map(|t| [t[0], t[1], t[2]])
        .collect();
    let handle = physics
        .0
        .insert_soft_body(SoftBodyBuilder::trimesh(vertices, triangles)?);
    let local_center = positions.iter().copied().sum::<Vec3>() / positions.len() as f32;
    Some(SoftBody {
        handle,
        mesh,
        position,
        rotation,
        local_center,
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
        let Some(mesh) = meshes.get_mut(&body.mesh) else {
            continue;
        };
        let positions: Vec<[f32; 3]> = physics.0.soft_bodies[body.handle]
            .particle_positions()
            .map(|p| to_bevy(p).to_array())
            .collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.compute_normals();
    }
}
