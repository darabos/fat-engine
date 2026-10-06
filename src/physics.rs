use bevy::prelude::*;
use rapier3d::parry::transformation::{MeshEnclosure, VolumeMeshParameters};
use rapier3d::prelude::{
    ColliderBuilder, PhysicsWorld, Pose, Rotation, SoftBodyBuilder, SoftBodyHandle,
    SoftBodyMaterial, SoftPatchConstraints, SpringCoefficients, Vector as RVec,
};

/// The whole-body cluster Rapier creates at insertion.
const ROOT_CLUSTER: u32 = 0;
/// Visual soft-body simulation rate.
const PHYSICS_HZ: f64 = 30.0;
/// Roughly how many simulated cells span the longest side of a body.
const CELLS_ACROSS: f32 = 2.0;
/// Halvings of the cells that straddle the surface. Each one is a few times more particles.
const CAGE_SUBDIVISIONS: u32 = 0;
/// Shrink-wrap passes pulling the cage onto the mesh; they never let the mesh poke out.
const CAGE_SMOOTHING: u32 = 12;
/// How close to the mesh the shrink-wrap may pull, as a fraction of the local cell size.
const CAGE_GUARD: f32 = 0.05;
/// Contact skin around the body, as a fraction of the cell size.
const CONTACT_SKIN: f32 = 0.1;
/// How hard the body is held at the pose the logic side asks for, in Hz.
const POSE_STIFFNESS: f32 = 5.0;
/// Damping of the logic anchor, which must not keep pumping energy into pressed bodies.
const POSE_DAMPING_RATIO: f32 = 3.0;
/// Springs along the cell edges, in Hz. These are what resist squashing.
const EDGE_STIFFNESS: f32 = 10.0;
/// Per-cell volume constraints, in Hz.
const VOLUME_STIFFNESS: f32 = 10.0;
/// Damping ratio of the cell springs; 1.0 is critical damping.
const DAMPING_RATIO: f32 = 0.25;
/// Settles wobble without slowing the body as a whole, per second. 0 leaves it ringing.
const DEFORMATION_DAMPING: f32 = 0.25;
/// Friction of the body's surface.
const FRICTION: f32 = 0.3;
/// Bounciness of the body's surface.
const RESTITUTION: f32 = 0.0;
/// Stiffness of soft body contacts relative to rigid ones. Rapier's default is 4.0.
const CONTACT_STIFFENING: f32 = 0.5;
/// Solver substeps per tick; the default four are unnecessary for these soft visual bodies.
const SOLVER_ITERATIONS: usize = 2;
/// Extra impact substeps, bounded even when the whole scene is kicked.
const MAX_EXTRA_SUBSTEPS: usize = 1;

#[derive(Resource)]
pub struct Physics(pub PhysicsWorld);

impl Default for Physics {
    fn default() -> Self {
        let mut world = PhysicsWorld::default();
        world.integration_parameters.dt = 1.0 / PHYSICS_HZ as f32;
        world.integration_parameters.num_solver_iterations = SOLVER_ITERATIONS;
        world.integration_parameters.soft_bodies.max_extra_substeps = MAX_EXTRA_SUBSTEPS;
        world.integration_parameters.soft_bodies.contact_stiffening = CONTACT_STIFFENING;
        let recovery = &mut world.integration_parameters.soft_bodies.recovery;
        // Let the volume contact own its patch instead of solving competing point contacts too.
        recovery.overlap_skin_volume = true;
        recovery.overlap_patch_constraints = SoftPatchConstraints::StandDown;
        Self(world)
    }
}

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

/// Geometry-derived cage data shared by every instance of the same asset.
#[derive(Clone)]
pub struct SoftBodyShape {
    builder: SoftBodyBuilder,
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
            .insert_resource(Time::<Fixed>::from_hz(PHYSICS_HZ))
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
    Some(builder.particle_radius(cell_size * CONTACT_SKIN))
}

fn combine_parts(parts: &[SkinPart]) -> (Vec<RVec>, Vec<[u32; 3]>, Vec<SkinSlice>) {
    let mut vertices = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut slices = Vec::with_capacity(parts.len());
    for part in parts {
        let start = vertices.len();
        for &p in &part.positions {
            vertices.push(to_rapier(p));
        }
        let offset = start as u32;
        for t in part.indices.chunks_exact(3) {
            triangles.push([t[0] + offset, t[1] + offset, t[2] + offset]);
        }
        slices.push(SkinSlice {
            mesh: part.mesh.clone(),
            start,
            len: part.positions.len(),
        });
    }
    (vertices, triangles, slices)
}

pub fn prepare_soft_body_shape(parts: &[SkinPart]) -> Option<SoftBodyShape> {
    let (vertices, triangles, _) = combine_parts(parts);
    if vertices.is_empty() || triangles.is_empty() {
        return None;
    }

    let mut minimum = Vec3::splat(f32::MAX);
    let mut maximum = Vec3::splat(f32::MIN);
    for part in parts {
        for &position in &part.positions {
            minimum = minimum.min(position);
            maximum = maximum.max(position);
        }
    }
    let cell_size = (maximum - minimum).max_element() / CELLS_ACROSS;
    let builder = build_cage(&vertices, &triangles, cell_size)?
        .shape_matching(true)
        .material(SoftBodyMaterial {
            edge_softness: SpringCoefficients::new(EDGE_STIFFNESS, DAMPING_RATIO),
            volume_softness: SpringCoefficients::new(VOLUME_STIFFNESS, DAMPING_RATIO),
            shape_matching_softness: SpringCoefficients::new(POSE_STIFFNESS, POSE_DAMPING_RATIO),
            deformation_damping: DEFORMATION_DAMPING,
            ..Default::default()
        })
        // Only the surface properties are kept; the shape is replaced by the body's own.
        .surface_collider(
            ColliderBuilder::ball(cell_size)
                .friction(FRICTION)
                .restitution(RESTITUTION),
        );
    Some(SoftBodyShape { builder })
}

pub fn spawn_soft_body_from_shape(
    physics: &mut Physics,
    shape: &SoftBodyShape,
    parts: Vec<SkinPart>,
    position: Vec3,
    rotation: Quat,
) -> Option<SoftBody> {
    let (mut vertices, triangles, slices) = combine_parts(&parts);
    if vertices.is_empty() || triangles.is_empty() {
        return None;
    }
    for vertex in &mut vertices {
        *vertex += to_rapier(position);
    }

    let mut builder = shape.builder.clone();
    for cage_position in &mut builder.positions {
        *cage_position += to_rapier(position);
    }
    builder = builder.skin(vertices, triangles);
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

pub fn spawn_soft_body(
    physics: &mut Physics,
    parts: Vec<SkinPart>,
    position: Vec3,
    rotation: Quat,
) -> Option<SoftBody> {
    let shape = prepare_soft_body_shape(&parts)?;
    spawn_soft_body_from_shape(physics, &shape, parts, position, rotation)
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

    /// Mean particle speed once the bodies have had time to settle.
    fn residual_speed(physics: &mut Physics, bodies: &[SoftBody], steps: usize) -> f32 {
        physics.0.integration_parameters.dt = 1.0 / 60.0;
        let measure_from = steps * 4 / 5;
        let mut speed = 0.0;
        let mut samples = 0;
        for step in 0..steps {
            for body in bodies {
                apply_target(&mut physics.0, body);
            }
            physics.0.step();
            if step >= measure_from {
                for body in bodies {
                    let sb = &physics.0.soft_bodies[body.handle];
                    speed += sb
                        .particle_velocities()
                        .map(|v| to_bevy(v).length())
                        .sum::<f32>()
                        / sb.num_particles() as f32;
                    samples += 1;
                }
            }
        }
        speed / samples as f32
    }

    fn pressed_cubes(half_separation: f32) -> (Physics, Vec<SoftBody>) {
        let mut physics = Physics::default();
        physics.0.insert_collider(
            ColliderBuilder::cuboid(10.0, 0.1, 10.0).translation(RVec::new(0.0, -0.1, 0.0)),
            None,
        );
        let bodies: Vec<SoftBody> = [-half_separation, half_separation]
            .iter()
            .map(|&x| {
                spawn_soft_body(
                    &mut physics,
                    vec![cube()],
                    Vec3::new(x, 0.5, 0.0),
                    Quat::IDENTITY,
                )
                .expect("the cube fills with cells")
            })
            .collect();
        (physics, bodies)
    }

    #[test]
    fn bodies_pressed_together_settle_instead_of_vibrating() {
        // Two cubes overlapping by a fifth of their width, both held by the logic side.
        let (mut physics, bodies) = pressed_cubes(0.4);
        let speed = residual_speed(&mut physics, &bodies, 300);
        assert!(speed < 0.05, "the bodies keep moving at {speed}");
    }

    #[test]
    fn deep_overlaps_stay_finite_and_still_deform_the_bodies() {
        for half_separation in [0.05, 0.0] {
            let (mut physics, bodies) = pressed_cubes(half_separation);
            let (mut isolated_physics, mut isolated_bodies) = pressed_cubes(half_separation);
            let removed = isolated_bodies.pop().expect("there are two cubes");
            isolated_physics.0.remove_soft_body(removed.handle);
            residual_speed(&mut isolated_physics, &isolated_bodies, 180);
            let isolated: Vec<_> = isolated_physics.0.soft_bodies[isolated_bodies[0].handle]
                .particle_positions()
                .collect();
            residual_speed(&mut physics, &bodies, 180);

            for body in &bodies {
                let sb = &physics.0.soft_bodies[body.handle];
                let collision_mesh = sb.collision_mesh().expect("the cage still collides");
                assert!(
                    !collision_mesh.is_skinned(),
                    "the detailed skin must not collide"
                );
                for p in sb.particle_positions().map(to_bevy) {
                    assert!(p.is_finite(), "overlap produced a non-finite particle");
                    assert!(
                        (p - body.position).length() < 2.0,
                        "overlap exploded the cage"
                    );
                }
                for v in sb.particle_velocities().map(to_bevy) {
                    assert!(v.is_finite(), "overlap produced a non-finite velocity");
                }
            }
            let deformation = physics.0.soft_bodies[bodies[0].handle]
                .particle_positions()
                .zip(isolated)
                .map(|(p, alone)| (p - alone).length())
                .fold(0.0, f32::max);
            assert!(
                deformation > 0.05,
                "inter-body contacts did not deform the cage"
            );
        }
    }

    #[test]
    #[ignore = "wall-clock benchmark; run separately with --ignored --nocapture"]
    fn overlapping_cages_fit_the_physics_frame_budget() {
        for half_separation in [0.4, 0.05, 0.0] {
            let (mut physics, bodies) = pressed_cubes(half_separation);
            residual_speed(&mut physics, &bodies, 60);
            let start = std::time::Instant::now();
            residual_speed(&mut physics, &bodies, 300);
            let per_step = start.elapsed().as_secs_f64() / 300.0;
            eprintln!(
                "half-separation={half_separation}: {:.2} ms/step",
                per_step * 1000.0
            );
            assert!(
                per_step < 1.0 / 60.0,
                "overlapping cages exceeded the 60 Hz physics budget: {per_step:.4} s/step"
            );
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

    #[test]
    fn cached_shape_spawns_independent_bodies_at_distinct_positions() {
        let part = cube();
        let shape = prepare_soft_body_shape(std::slice::from_ref(&part))
            .expect("the cube fills with cells");
        let mut physics = Physics::default();

        let first = spawn_soft_body_from_shape(
            &mut physics,
            &shape,
            vec![cube()],
            Vec3::new(-2.0, 3.0, 0.0),
            Quat::IDENTITY,
        )
        .expect("the first body spawns from the cached shape");
        let second = spawn_soft_body_from_shape(
            &mut physics,
            &shape,
            vec![cube()],
            Vec3::new(2.0, 3.0, 0.0),
            Quat::IDENTITY,
        )
        .expect("the second body spawns from the cached shape");

        let first_center = center(&physics, &first);
        let second_center = center(&physics, &second);
        assert!(
            (second_center - first_center - Vec3::X * 4.0).length() < 0.05,
            "cached bodies were not offset independently: {first_center} and {second_center}"
        );
        assert_ne!(first.handle, second.handle);
    }
}
