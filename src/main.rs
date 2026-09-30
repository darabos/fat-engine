use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};
use bevy::gltf::{Gltf, GltfMesh};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use fat_engine::physics::{Physics, SoftBody, SoftBodyPlugin, spawn_soft_body};

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
                asset_name: "animal-cat",
                position: Vec3::new(-2.0, 0.5, 0.0),
                rotation: Quat::IDENTITY,
            },
            LogicBody {
                asset_name: "animal-bunny",
                position: Vec3::new(0.0, 0.5, 0.0),
                rotation: Quat::IDENTITY,
            },
            LogicBody {
                asset_name: "animal-fox",
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
    meshes: Res<Assets<Mesh>>,
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
        if gltf.meshes.is_empty() {
            body_assets.spawned[index] = true;
            warn!("GLB body {} contains no meshes", bodies.0[index].asset_name);
            continue;
        }
        let Some(gltf_mesh_handle) = gltf.meshes.first() else {
            body_assets.spawned[index] = true;
            warn!("GLB body {} contains no mesh", bodies.0[index].asset_name);
            continue;
        };
        let Some(gltf_mesh) = gltf_meshes.get(gltf_mesh_handle) else {
            continue;
        };
        let Some(primitive) = gltf_mesh.primitives.first() else {
            body_assets.spawned[index] = true;
            warn!(
                "GLB body {} contains no primitive",
                bodies.0[index].asset_name
            );
            continue;
        };
        let Some(source_mesh) = meshes.get(&primitive.mesh) else {
            continue;
        };
        if source_mesh.primitive_topology() != PrimitiveTopology::TriangleList {
            body_assets.spawned[index] = true;
            warn!(
                "GLB body {} first primitive is not a triangle list",
                bodies.0[index].asset_name
            );
            continue;
        }
        let Some(positions) = extract_positions(source_mesh) else {
            body_assets.spawned[index] = true;
            warn!(
                "GLB body {} has no Float32x3 positions",
                bodies.0[index].asset_name
            );
            continue;
        };
        let Some(indices) = extract_indices(source_mesh) else {
            body_assets.spawned[index] = true;
            warn!(
                "GLB body {} has no triangle indices",
                bodies.0[index].asset_name
            );
            continue;
        };
        let render_mesh = primitive.mesh.clone();
        let body = &bodies.0[index];
        body_assets.spawned[index] = true;
        let Some(soft_body) = spawn_soft_body(
            &mut physics,
            &positions,
            &indices,
            body.position,
            body.rotation,
            render_mesh.clone(),
        ) else {
            warn!("GLB body {} has an empty mesh", body.asset_name);
            continue;
        };
        info!(
            "loaded mesh {}: vertices={} triangles={}",
            body.asset_name,
            positions.len(),
            indices.len() / 3
        );
        commands.spawn((
            Mesh3d(render_mesh),
            MeshMaterial3d(primitive.material.clone().unwrap_or_default()),
            LogicFrame { index },
            soft_body,
        ));
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
