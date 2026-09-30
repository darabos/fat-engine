use bevy::prelude::*;
use rapier3d::parry::transformation::{MeshEnclosure, VolumeMeshParameters};
use rapier3d::prelude::{
    ColliderBuilder, PhysicsWorld, Pose, Rotation, SoftBodyBuilder, SoftBodyHandle,
    SoftBodyMaterial, SpringCoefficients, Vector as RVec,
};

/// The whole-body cluster Rapier creates at insertion.
const ROOT_CLUSTER: u32 = 0;
/// Roughly how many simulated cells span the longest side of a body.
const CELLS_ACROSS: f32 = 5.0;
/// Halvings of the cells that straddle the surface. Each one is a few times more particles.
const CAGE_SUBDIVISIONS: u32 = 0;
/// Shrink-wrap passes pulling the cage onto the mesh; they never let the mesh poke out.
const CAGE_SMOOTHING: u32 = 12;
/// How close to the mesh the shrink-wrap may pull, as a fraction of the local cell size.
const CAGE_GUARD: f32 = 0.05;
/// Contact skin around the body, as a fraction of the cell size.
const CONTACT_SKIN: f32 = 0.01;
/// How hard the body is held at the pose the logic side asks for, in Hz.
const POSE_STIFFNESS: f32 = 5.0;

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
    // Without smoothing and subdivision the cage is a raw lattice standing up to a whole cell
    // outside the mesh.
    let params = |enclosure| VolumeMeshParameters {
        enclosure,
        cover_subdivisions: CAGE_SUBDIVISIONS,
        cover_smoothing: CAGE_SMOOTHING,
        cover_guard: CAGE_GUARD,
        ..VolumeMeshParameters::new(cell_size)
    };
    let builder =
        SoftBodyBuilder::volumetric_with(vertices, triangles, &params(MeshEnclosure::Cover))
            // A mesh that isn't closed has no inside to fill, so it gets a shell of cells.
            .or_else(|| {
                SoftBodyBuilder::volumetric_with(vertices, triangles, &params(MeshEnclosure::Crust))
            })?;
    Some(
        builder
            .skin(vertices.to_vec(), triangles.to_vec())
            .particle_radius(cell_size * CONTACT_SKIN),
    )
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
    // Shape matching is what ties the particles to the pose the logic side sets; the volumetric
    // constructors leave it off.
    let builder = build_cage(&vertices, &triangles, cell_size)?
        .shape_matching(true)
        .material(SoftBodyMaterial {
            shape_matching_softness: SpringCoefficients::new(POSE_STIFFNESS, 1.0),
            ..Default::default()
        });
    let handle = physics.0.insert_soft_body(builder);

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

/// Pulls the body toward the pose the logic side asks for, leaving the cells free to deform.
fn apply_target(world: &mut PhysicsWorld, body: &SoftBody) {
    let r = body.rotation;
    let target = Pose::from_parts(
        to_rapier(body.position + r * body.local_center),
        Rotation::from_xyzw(r.x, r.y, r.z, r.w),
    );
    if let Some(cluster) = world.soft_bodies[body.handle].cluster_mut(ROOT_CLUSTER) {
        cluster.set_shape_matching_target(Some(target));
    }
}

fn step_physics(time: Res<Time<Fixed>>, mut physics: ResMut<Physics>, bodies: Query<&SoftBody>) {
    let world = &mut physics.0;
    world.integration_parameters.dt = time.delta_secs();
    for body in &bodies {
        apply_target(world, body);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cube() -> SkinPart {
        let positions = [
            Vec3::new(-0.5, -0.5, -0.5),
            Vec3::new(0.5, -0.5, -0.5),
            Vec3::new(0.5, 0.5, -0.5),
            Vec3::new(-0.5, 0.5, -0.5),
            Vec3::new(-0.5, -0.5, 0.5),
            Vec3::new(0.5, -0.5, 0.5),
            Vec3::new(0.5, 0.5, 0.5),
            Vec3::new(-0.5, 0.5, 0.5),
        ];
        let indices = [
            0, 3, 2, 0, 2, 1, // -Z
            4, 5, 6, 4, 6, 7, // +Z
            0, 1, 5, 0, 5, 4, // -Y
            3, 7, 6, 3, 6, 2, // +Y
            0, 4, 7, 0, 7, 3, // -X
            1, 2, 6, 1, 6, 5, // +X
        ];
        SkinPart {
            mesh: Handle::default(),
            positions: positions.to_vec(),
            indices: indices.to_vec(),
        }
    }

    fn center(physics: &Physics, body: &SoftBody) -> Vec3 {
        let soft_body = &physics.0.soft_bodies[body.handle];
        soft_body.particle_positions().map(to_bevy).sum::<Vec3>() / soft_body.num_particles() as f32
    }

    /// How far a cage stands outside the mesh it was built for, in cell sizes.
    fn overshoot(positions: &[RVec], half_extent: f32, cell_size: f32) -> f32 {
        positions
            .iter()
            .map(|p| (to_bevy(*p).abs().max_element() - half_extent).max(0.0))
            .fold(0.0, f32::max)
            / cell_size
    }

    fn simulate(physics: &mut Physics, body: &SoftBody, steps: usize) {
        physics.0.integration_parameters.dt = 1.0 / 60.0;
        for _ in 0..steps {
            apply_target(&mut physics.0, body);
            physics.0.step();
        }
    }

    #[test]
    fn the_cage_hugs_the_mesh_tighter_than_a_raw_lattice() {
        let part = cube();
        let vertices: Vec<RVec> = part.positions.iter().map(|&p| to_rapier(p)).collect();
        let triangles: Vec<[u32; 3]> = part
            .indices
            .chunks_exact(3)
            .map(|t| [t[0], t[1], t[2]])
            .collect();
        let cell_size = 1.0 / CELLS_ACROSS;

        let raw = SoftBodyBuilder::volumetric_with(
            &vertices,
            &triangles,
            &VolumeMeshParameters::new(cell_size),
        )
        .expect("the cube fills with cells");
        let tuned =
            build_cage(&vertices, &triangles, cell_size).expect("the cube fills with cells");

        let raw = overshoot(raw.particle_positions(), 0.5, cell_size);
        let tuned = overshoot(tuned.particle_positions(), 0.5, cell_size);
        assert!(
            tuned < raw * 0.75,
            "the cage stands {tuned} cell sizes outside the mesh, the raw lattice {raw}"
        );
    }

    #[test]
    fn gravity_does_not_move_the_body_off_its_logic_position() {
        let mut physics = Physics::default();
        let position = Vec3::new(0.0, 3.0, 0.0);
        let body = spawn_soft_body(&mut physics, vec![cube()], position, Quat::IDENTITY)
            .expect("the cube fills with cells");
        let start = center(&physics, &body);

        simulate(&mut physics, &body, 180);

        let drift = center(&physics, &body) - start;
        assert!(drift.length() < 0.05, "the body drifted by {drift}");
    }

    #[test]
    fn the_body_follows_its_logic_position() {
        let mut physics = Physics::default();
        let mut body = spawn_soft_body(
            &mut physics,
            vec![cube()],
            Vec3::new(0.0, 3.0, 0.0),
            Quat::IDENTITY,
        )
        .expect("the cube fills with cells");
        let start = center(&physics, &body);

        body.position += Vec3::X;
        simulate(&mut physics, &body, 120);

        let moved = center(&physics, &body) - start;
        assert!(
            (moved - Vec3::X).length() < 0.05,
            "the body moved by {moved}"
        );
    }
}
