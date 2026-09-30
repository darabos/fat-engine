use bevy::prelude::*;
use parry3d::{
    math::{Isometry, Point, Real},
    query::point::PointQuery,
    shape::{TriMesh, TriMeshFlags},
};
use std::collections::HashMap;
use std::time::Instant;

#[derive(Debug, Default)]
struct TimingSample {
    total_ms: f64,
    samples: u32,
}

#[derive(Resource, Debug)]
pub struct PerformanceTimings {
    samples: HashMap<&'static str, TimingSample>,
    report_elapsed: f32,
}

impl Default for PerformanceTimings {
    fn default() -> Self {
        Self {
            samples: HashMap::new(),
            report_elapsed: 0.0,
        }
    }
}

impl PerformanceTimings {
    pub fn record(&mut self, label: &'static str, started: Instant) {
        let sample = self.samples.entry(label).or_default();
        sample.total_ms += started.elapsed().as_secs_f64() * 1000.0;
        sample.samples += 1;
    }

    pub fn advance_report_clock(&mut self, delta_seconds: f32) -> bool {
        self.report_elapsed += delta_seconds;
        if self.report_elapsed < 1.0 {
            return false;
        }
        self.report_elapsed = 0.0;
        true
    }

    pub fn take_report(&mut self) -> String {
        let mut report = String::new();
        let mut samples = std::mem::take(&mut self.samples)
            .into_iter()
            .collect::<Vec<_>>();
        samples.sort_unstable_by_key(|(label, _)| *label);
        for (label, sample) in samples {
            if sample.samples > 0 {
                report.push_str(&format!(
                    " {}={:.2}ms / {} = {:.2}",
                    label,
                    sample.total_ms,
                    sample.samples,
                    sample.total_ms / f64::from(sample.samples)
                ));
            }
        }
        report
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub position: Vec3,
    pub previous_position: Vec3,
    pub velocity: Vec3,
    pub inverse_mass: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spring {
    pub a: usize,
    pub b: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct PhysicsSettings {
    pub gravity: Vec3,
    pub damping: f32,
    pub spring_strength: f32,
    pub frame_strength: f32,
    pub volume_strength: f32,
    pub collision_slop: f32,
}

#[derive(Component, Debug)]
pub struct SoftBody {
    pub vertices: Vec<Vertex>,
    pub rest_positions: Vec<Vec3>,
    pub triangles: Vec<u32>,
    pub springs: Vec<Spring>,
    pub rest_lengths: Vec<f32>,
    pub rest_volume: f32,
    pub frame_position: Vec3,
    pub frame_rotation: Quat,
}

#[derive(Component, Clone)]
pub struct SoftBodyMesh {
    pub mesh: Handle<Mesh>,
}

impl SoftBody {
    pub fn from_mesh(positions: Vec<Vec3>, indices: Vec<u32>, frame_position: Vec3) -> Self {
        let springs = springs_from_triangles(&indices);
        let rest_lengths = springs
            .iter()
            .map(|spring| positions[spring.a].distance(positions[spring.b]))
            .collect();
        let rest_volume = signed_volume(&positions, &indices);
        let rest_positions = positions.clone();
        let vertices = positions
            .into_iter()
            .map(|position| Vertex {
                position: position + frame_position,
                previous_position: position + frame_position,
                velocity: Vec3::ZERO,
                inverse_mass: 1.0,
            })
            .collect();
        Self {
            vertices,
            rest_positions,
            triangles: indices,
            springs,
            rest_lengths,
            rest_volume,
            frame_position,
            frame_rotation: Quat::IDENTITY,
        }
    }
}

#[derive(Resource, Clone, Copy, Debug)]
pub struct SoftBodySettings {
    pub physics: PhysicsSettings,
    pub fixed_dt: f32,
    pub constraint_iterations: usize,
}

impl Default for SoftBodySettings {
    fn default() -> Self {
        Self {
            physics: PhysicsSettings::default(),
            fixed_dt: 1.0 / 60.0,
            constraint_iterations: 4,
        }
    }
}

pub struct SoftBodyPlugin;

impl Plugin for SoftBodyPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SoftBodySettings::default())
            .insert_resource(PerformanceTimings::default())
            .add_systems(
                FixedUpdate,
                (
                    integrate_particles,
                    apply_frame_constraints,
                    apply_shape_constraints,
                    solve_volume,
                    solve_collisions,
                    update_velocities,
                )
                    .chain(),
            );
    }
}

impl Default for PhysicsSettings {
    fn default() -> Self {
        Self {
            gravity: Vec3::new(0.0, -9.81, 0.0),
            damping: 0.995,
            spring_strength: 0.35,
            frame_strength: 0.7,
            volume_strength: 0.25,
            collision_slop: 0.01,
        }
    }
}

pub fn springs_from_triangles(indices: &[u32]) -> Vec<Spring> {
    let mut springs = Vec::new();
    for triangle in indices.chunks_exact(3) {
        add_unique_spring(&mut springs, triangle[0] as usize, triangle[1] as usize);
        add_unique_spring(&mut springs, triangle[1] as usize, triangle[2] as usize);
        add_unique_spring(&mut springs, triangle[2] as usize, triangle[0] as usize);
    }
    springs
}

fn add_unique_spring(springs: &mut Vec<Spring>, a: usize, b: usize) {
    let spring = Spring {
        a: a.min(b),
        b: a.max(b),
    };
    if spring.a != spring.b && !springs.contains(&spring) {
        springs.push(spring);
    }
}

pub fn signed_volume(positions: &[Vec3], indices: &[u32]) -> f32 {
    indices
        .chunks_exact(3)
        .map(|triangle| {
            let a = positions[triangle[0] as usize];
            let b = positions[triangle[1] as usize];
            let c = positions[triangle[2] as usize];
            a.dot(b.cross(c)) / 6.0
        })
        .sum()
}

pub fn predict(vertices: &mut [Vertex], dt: f32, settings: PhysicsSettings) {
    for vertex in vertices {
        if vertex.inverse_mass == 0.0 {
            continue;
        }
        vertex.previous_position = vertex.position;
        vertex.velocity += settings.gravity * dt;
        vertex.position += vertex.velocity * dt;
    }
}

pub fn solve_springs(
    vertices: &mut [Vertex],
    springs: &[Spring],
    rest_lengths: &[f32],
    strength: f32,
) {
    for (spring, rest_length) in springs.iter().zip(rest_lengths) {
        let delta = vertices[spring.b].position - vertices[spring.a].position;
        let length = delta.length();
        if length <= f32::EPSILON {
            continue;
        }
        let correction = delta / length * (length - rest_length) * strength;
        let inverse_mass_sum = vertices[spring.a].inverse_mass + vertices[spring.b].inverse_mass;
        if inverse_mass_sum == 0.0 {
            continue;
        }
        vertices[spring.a].position +=
            correction * vertices[spring.a].inverse_mass / inverse_mass_sum;
        vertices[spring.b].position -=
            correction * vertices[spring.b].inverse_mass / inverse_mass_sum;
    }
}

pub fn solve_frame_springs(
    vertices: &mut [Vertex],
    rest_positions: &[Vec3],
    frame_position: Vec3,
    frame_rotation: Quat,
    strength: f32,
) {
    for (vertex, rest_position) in vertices.iter_mut().zip(rest_positions) {
        if vertex.inverse_mass == 0.0 {
            continue;
        }
        let target = frame_position + frame_rotation * *rest_position;
        let correction = target - vertex.position;
        vertex.position += correction * strength * vertex.inverse_mass;
    }
}

pub fn solve_ground(vertices: &mut [Vertex], ground_height: f32, slop: f32) {
    for vertex in vertices {
        if vertex.inverse_mass > 0.0 && vertex.position.y < ground_height + slop {
            vertex.position.y = ground_height + slop;
            if vertex.velocity.y < 0.0 {
                vertex.velocity.y = 0.0;
            }
        }
    }
}

pub fn derive_velocities(vertices: &mut [Vertex], dt: f32, damping: f32) {
    for vertex in vertices {
        if vertex.inverse_mass > 0.0 {
            vertex.velocity = (vertex.position - vertex.previous_position) / dt * damping;
        }
    }
}

fn vertex_bounds(vertices: &[Vertex]) -> Option<(Vec3, Vec3)> {
    let first = vertices.first()?.position;
    let mut minimum = first;
    let mut maximum = first;
    for vertex in &vertices[1..] {
        minimum = minimum.min(vertex.position);
        maximum = maximum.max(vertex.position);
    }
    Some((minimum, maximum))
}

fn bounds_overlap(first: (Vec3, Vec3), second: (Vec3, Vec3)) -> bool {
    first.0.x <= second.1.x
        && first.1.x >= second.0.x
        && first.0.y <= second.1.y
        && first.1.y >= second.0.y
        && first.0.z <= second.1.z
        && first.1.z >= second.0.z
}

fn bounds_contain(bounds: (Vec3, Vec3), point: Vec3) -> bool {
    point.x >= bounds.0.x
        && point.x <= bounds.1.x
        && point.y >= bounds.0.y
        && point.y <= bounds.1.y
        && point.z >= bounds.0.z
        && point.z <= bounds.1.z
}

pub fn integrate_particles(
    settings: Res<SoftBodySettings>,
    mut bodies: Query<&mut SoftBody>,
    mut timings: ResMut<PerformanceTimings>,
) {
    let started = Instant::now();
    for mut body in &mut bodies {
        predict(&mut body.vertices, settings.fixed_dt, settings.physics);
    }
    timings.record("integrate", started);
}

pub fn apply_frame_constraints(
    settings: Res<SoftBodySettings>,
    mut bodies: Query<&mut SoftBody>,
    mut timings: ResMut<PerformanceTimings>,
) {
    let started = Instant::now();
    for mut body in &mut bodies {
        let rest_positions = body.rest_positions.clone();
        let frame_position = body.frame_position;
        let frame_rotation = body.frame_rotation;
        solve_frame_springs(
            &mut body.vertices,
            &rest_positions,
            frame_position,
            frame_rotation,
            settings.physics.frame_strength,
        );
    }
    timings.record("frame_constraints", started);
}

pub fn apply_shape_constraints(
    settings: Res<SoftBodySettings>,
    mut bodies: Query<&mut SoftBody>,
    mut timings: ResMut<PerformanceTimings>,
) {
    let started = Instant::now();
    for mut body in &mut bodies {
        let springs = body.springs.clone();
        let rest_lengths = body.rest_lengths.clone();
        solve_springs(
            &mut body.vertices,
            &springs,
            &rest_lengths,
            settings.physics.spring_strength,
        );
    }
    timings.record("shape_constraints", started);
}

pub fn solve_volume(
    settings: Res<SoftBodySettings>,
    mut bodies: Query<&mut SoftBody>,
    mut timings: ResMut<PerformanceTimings>,
) {
    let started = Instant::now();
    for mut body in &mut bodies {
        let positions: Vec<Vec3> = body.vertices.iter().map(|vertex| vertex.position).collect();
        let current_volume = signed_volume(&positions, &body.triangles);
        if body.rest_volume.abs() <= f32::EPSILON || current_volume.abs() <= f32::EPSILON {
            continue;
        }
        let scale = (body.rest_volume / current_volume).cbrt();
        let center = positions.iter().copied().sum::<Vec3>() / positions.len() as f32;
        let correction = (scale - 1.0) * settings.physics.volume_strength;
        for vertex in &mut body.vertices {
            vertex.position = center + (vertex.position - center) * (1.0 + correction);
        }
    }
    timings.record("volume", started);
}

pub fn solve_collisions(
    settings: Res<SoftBodySettings>,
    mut bodies: ParamSet<(Query<(Entity, &mut SoftBody)>, Query<(Entity, &SoftBody)>)>,
    mut timings: ResMut<PerformanceTimings>,
) {
    let started = Instant::now();
    let snapshot_started = Instant::now();
    let body_data: Vec<(
        Entity,
        Vec<Point<Real>>,
        Vec<[u32; 3]>,
        Option<(Vec3, Vec3)>,
    )> = bodies
        .p1()
        .iter()
        .map(|(entity, body)| {
            let positions: Vec<Point<Real>> = body
                .vertices
                .iter()
                .map(|vertex| Point::new(vertex.position.x, vertex.position.y, vertex.position.z))
                .collect();
            let triangles: Vec<[u32; 3]> = body
                .triangles
                .chunks_exact(3)
                .map(|triangle| [triangle[0], triangle[1], triangle[2]])
                .collect();
            (entity, positions, triangles, vertex_bounds(&body.vertices))
        })
        .collect();
    timings.record("collision_snapshot", snapshot_started);

    let mesh_build_started = Instant::now();
    let other_bodies: Vec<(Entity, TriMesh, (Vec3, Vec3))> = body_data
        .into_iter()
        .filter_map(|(entity, positions, triangles, bounds)| {
            if positions.is_empty() || triangles.is_empty() || bounds.is_none() {
                None
            } else {
                Some((
                    entity,
                    TriMesh::with_flags(positions, triangles, TriMeshFlags::ORIENTED),
                    bounds.expect("checked above"),
                ))
            }
        })
        .collect();
    timings.record("collision_mesh_build", mesh_build_started);

    let ground_started = Instant::now();
    for (_, mut body) in &mut bodies.p0() {
        solve_ground(&mut body.vertices, 0.0, settings.physics.collision_slop);
    }
    timings.record("collision_ground", ground_started);

    let query_started = Instant::now();
    for (entity, mut body) in &mut bodies.p0() {
        let Some(body_bounds) = vertex_bounds(&body.vertices) else {
            continue;
        };
        for (other_entity, mesh, other_bounds) in &other_bodies {
            if entity == *other_entity {
                continue;
            }
            let overlap_started = Instant::now();
            if !bounds_overlap(body_bounds, *other_bounds) {
                timings.record("collision_bound_overlap", overlap_started);
                continue;
            }
            timings.record("collision_bound_overlap", overlap_started);
            for vertex in &mut body.vertices {
                let position = vertex.position;
                let bound_contained_started = Instant::now();
                if !bounds_contain(*other_bounds, position) {
                    timings.record("collision_bound_contained", bound_contained_started);
                    continue;
                }
                timings.record("collision_bound_contained", bound_contained_started);
                let point = Point::new(position.x, position.y, position.z);
                let identity = Isometry::identity();
                let contains_started = Instant::now();
                let contains = mesh.contains_point(&identity, &point);
                timings.record("collision_contains", contains_started);
                if !contains {
                    continue;
                }
                let project_started = Instant::now();
                let projection = mesh.project_point(&identity, &point, true);
                timings.record("collision_project", project_started);
                vertex.position =
                    Vec3::new(projection.point.x, projection.point.y, projection.point.z);
            }
        }
    }
    timings.record("collision_queries", query_started);
    timings.record("collisions", started);
}

pub fn update_velocities(
    settings: Res<SoftBodySettings>,
    mut bodies: Query<&mut SoftBody>,
    mut timings: ResMut<PerformanceTimings>,
) {
    let started = Instant::now();
    for mut body in &mut bodies {
        derive_velocities(
            &mut body.vertices,
            settings.fixed_dt,
            settings.physics.damping,
        );
    }
    timings.record("velocities", started);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_unique_edges_from_triangles() {
        let springs = springs_from_triangles(&[0, 1, 2, 2, 1, 3]);
        assert_eq!(springs.len(), 5);
        assert!(springs.contains(&Spring { a: 1, b: 2 }));
    }

    #[test]
    fn calculates_signed_tetrahedron_volume() {
        let positions = [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z];
        let indices = [0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3];
        assert!((signed_volume(&positions, &indices) - 1.0 / 6.0).abs() < 1e-6);
    }

    #[test]
    fn ground_constraint_removes_downward_penetration() {
        let mut vertices = [Vertex {
            position: Vec3::new(0.0, -1.0, 0.0),
            previous_position: Vec3::ZERO,
            velocity: Vec3::new(0.0, -2.0, 0.0),
            inverse_mass: 1.0,
        }];
        solve_ground(&mut vertices, 0.0, 0.01);
        assert_eq!(vertices[0].position.y, 0.01);
        assert_eq!(vertices[0].velocity.y, 0.0);
    }
}
