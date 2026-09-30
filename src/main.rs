use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};
use bevy::gltf::{Gltf, GltfMesh};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use fat_engine::physics::{Physics, SkinPart, SoftBody, SoftBodyPlugin, spawn_soft_body};

#[derive(Clone, Debug)]
struct LogicBody {
    asset_name: &'static str,
    position: Vec3,
    rotation: Quat,
}

#[derive(Resource)]
struct DemoBodies(Vec<LogicBody>);

#[derive(Resource)]
struct BodyAssets {
    handles: Vec<Handle<Gltf>>,
    spawned: Vec<bool>,
}

#[derive(Component)]
struct LogicFrame {
    index: usize,
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            file_path: "assets".into(),
            ..default()
        }))
        .add_plugins((
            FrameTimeDiagnosticsPlugin::default(),
            LogDiagnosticsPlugin::default(),
        ))
        .add_plugins(SoftBodyPlugin)
        .insert_resource(DemoBodies(vec![
            LogicBody {
                asset_name: "test",
                position: Vec3::new(-2.0, 0.5, 0.0),
                rotation: Quat::IDENTITY,
            },
            LogicBody {
                asset_name: "test",
                position: Vec3::new(0.0, 0.5, 0.0),
                rotation: Quat::IDENTITY,
            },
            LogicBody {
                asset_name: "test",
                position: Vec3::new(2.0, 0.5, 0.0),
                rotation: Quat::IDENTITY,
            },
        ]))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (spawn_loaded_bodies, move_selected_frame, sync_logic_frames).chain(),
        )
        .run();
}

fn setup(
    mut commands: Commands,
    bodies: Res<DemoBodies>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(7.0, 5.0, 9.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        PointLight {
            intensity: 1800.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));

    commands.insert_resource(BodyAssets {
        handles: bodies
            .0
            .iter()
            .map(|body| asset_server.load(format!("animals/{}.glb", body.asset_name)))
            .collect(),
        spawned: vec![false; bodies.0.len()],
    });

    commands.spawn((
        DirectionalLight {
            illuminance: 5000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, -0.5, 0.0)),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(20.0, 20.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.18, 0.22, 0.2),
            ..default()
        })),
    ));
}

fn spawn_loaded_bodies(
    mut commands: Commands,
    bodies: Res<DemoBodies>,
    mut body_assets: ResMut<BodyAssets>,
    gltfs: Res<Assets<Gltf>>,
    gltf_meshes: Res<Assets<GltfMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut physics: ResMut<Physics>,
) {
    let handles = body_assets.handles.clone();
    for (index, handle) in handles.iter().enumerate() {
        if body_assets.spawned[index] {
            continue;
        }
        let Some(gltf) = gltfs.get(handle) else {
            continue;
        };
        let body = &bodies.0[index];
        // Every primitive must be loaded before any of them is turned into a render mesh.
        if gltf
            .meshes
            .iter()
            .filter_map(|handle| gltf_meshes.get(handle))
            .flat_map(|gltf_mesh| &gltf_mesh.primitives)
            .any(|primitive| meshes.get(&primitive.mesh).is_none())
        {
            continue;
        }
        body_assets.spawned[index] = true;

        let mut parts = Vec::new();
        let mut materials = Vec::new();
        for gltf_mesh_handle in &gltf.meshes {
            let Some(gltf_mesh) = gltf_meshes.get(gltf_mesh_handle) else {
                continue;
            };
            for primitive in &gltf_mesh.primitives {
                let Some(source_mesh) = meshes.get(&primitive.mesh) else {
                    continue;
                };
                if source_mesh.primitive_topology() != PrimitiveTopology::TriangleList {
                    warn!("{}: skipping a non-triangle primitive", body.asset_name);
                    continue;
                }
                let (Some(positions), Some(indices)) =
                    (extract_positions(source_mesh), extract_indices(source_mesh))
                else {
                    warn!(
                        "{}: skipping a primitive without positions or indices",
                        body.asset_name
                    );
                    continue;
                };
                // A private copy, so two bodies of the same asset deform independently.
                let copy = source_mesh.clone();
                let render_mesh = meshes.add(copy);
                materials.push(primitive.material.clone().unwrap_or_default());
                parts.push(SkinPart {
                    mesh: render_mesh,
                    positions,
                    indices,
                });
            }
        }
        if parts.is_empty() {
            warn!("GLB body {} has no usable primitive", body.asset_name);
            continue;
        }

        let vertices: usize = parts.iter().map(|part| part.positions.len()).sum();
        let triangles: usize = parts.iter().map(|part| part.indices.len() / 3).sum();
        let render_meshes: Vec<Handle<Mesh>> = parts.iter().map(|part| part.mesh.clone()).collect();
        let Some(soft_body) = spawn_soft_body(&mut physics, parts, body.position, body.rotation)
        else {
            warn!(
                "GLB body {} could not be filled with cells",
                body.asset_name
            );
            continue;
        };
        info!(
            "loaded {}: primitives={} vertices={} triangles={}",
            body.asset_name,
            render_meshes.len(),
            vertices,
            triangles
        );
        commands
            .spawn((
                LogicFrame { index },
                soft_body,
                Transform::default(),
                Visibility::default(),
            ))
            .with_children(|parent| {
                for (mesh, material) in render_meshes.into_iter().zip(materials) {
                    parent.spawn((Mesh3d(mesh), MeshMaterial3d(material)));
                }
            });
    }
}

fn extract_positions(mesh: &Mesh) -> Option<Vec<Vec3>> {
    match mesh.attribute(Mesh::ATTRIBUTE_POSITION)? {
        VertexAttributeValues::Float32x3(values) => Some(
            values
                .iter()
                .map(|value| Vec3::from_array(*value))
                .collect(),
        ),
        _ => None,
    }
}

fn extract_indices(mesh: &Mesh) -> Option<Vec<u32>> {
    match mesh.indices()? {
        Indices::U16(values) => Some(values.iter().map(|value| u32::from(*value)).collect()),
        Indices::U32(values) => Some(values.clone()),
    }
}

fn move_selected_frame(
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut bodies: ResMut<DemoBodies>,
) {
    let mut direction = Vec3::ZERO;
    if keyboard.pressed(KeyCode::ArrowLeft) {
        direction.x -= 1.0;
    }
    if keyboard.pressed(KeyCode::ArrowRight) {
        direction.x += 1.0;
    }
    if keyboard.pressed(KeyCode::ArrowUp) {
        direction.z -= 1.0;
    }
    if keyboard.pressed(KeyCode::ArrowDown) {
        direction.z += 1.0;
    }
    if direction != Vec3::ZERO {
        bodies.0[0].position += direction.normalize() * 3.0 * time.delta_secs();
    }
}

fn sync_logic_frames(bodies: Res<DemoBodies>, mut query: Query<(&LogicFrame, &mut SoftBody)>) {
    if !bodies.is_changed() {
        return;
    }
    for (frame, mut soft_body) in &mut query {
        let body = &bodies.0[frame.index];
        soft_body.position = body.position;
        soft_body.rotation = body.rotation;
    }
}
