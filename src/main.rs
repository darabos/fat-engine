use bevy::asset::LoadState;
use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};
use bevy::gltf::{Gltf, GltfMesh};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::render::view::RenderLayers;
use bevy::window::PrimaryWindow;
use fat_engine::physics::{
    Physics, SkinPart, SoftBody, SoftBodyPlugin, SoftBodyShape, prepare_soft_body_shape,
    spawn_soft_body_from_shape,
};
use fat_engine::scripting::{GameCommand, ScriptRuntime};
use fat_engine::slime::{
    ATTRIBUTE_REST_POSITION, BACK_FACE_LAYER, SlimeAssets, SlimeCamera, SlimeMaterial,
    SlimeMaterials, SlimePlugin,
};
use std::collections::HashMap;
use std::fs;

const ENABLE_SLIME_SHADER: bool = false;

#[derive(Clone, Debug)]
struct LogicBody {
    handle: u64,
    asset_name: String,
    position: Vec3,
    rotation: Quat,
    active: bool,
}

#[derive(Resource, Default)]
struct GameBodies(Vec<LogicBody>);

#[derive(Resource, Default)]
struct BodyAssets {
    handles: Vec<Handle<Gltf>>,
    spawned: Vec<bool>,
    by_path: HashMap<String, Handle<Gltf>>,
    shapes: HashMap<String, SoftBodyShape>,
}

#[derive(Component)]
struct LogicFrame {
    handle: u64,
}

fn main() {
    let script = fs::read_to_string("assets/scripts/logic.lua")
        .expect("failed to read assets/scripts/logic.lua");
    let script_runtime =
        ScriptRuntime::new(&script).expect("failed to initialize assets/scripts/logic.lua");

    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            file_path: "assets".into(),
            ..default()
        }))
        .add_plugins((
            FrameTimeDiagnosticsPlugin::default(),
            LogDiagnosticsPlugin::default(),
        ))
        .add_plugins((SoftBodyPlugin, SlimePlugin))
        .insert_resource(GameBodies::default())
        .insert_resource(BodyAssets::default())
        .insert_resource(CameraView::default())
        .insert_non_send_resource(script_runtime)
        .add_systems(Startup, (setup, run_script_init).chain())
        .add_systems(
            Update,
            (
                run_script_update,
                apply_script_commands,
                spawn_loaded_bodies,
                sync_logic_frames,
                update_camera_view,
            )
                .chain(),
        )
        .run();
}

#[derive(Resource, Default)]
struct CameraView(Option<(f32, f32, f32, f32)>);

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let camera = commands
        .spawn((
            Camera3d::default(),
            Transform::from_xyz(7.0, 8.0, 9.0).looking_at(Vec3::ZERO, Vec3::Y),
        ))
        .id();
    if ENABLE_SLIME_SHADER {
        commands.entity(camera).insert(SlimeCamera);
    }
    commands.spawn((
        PointLight {
            intensity: 1800.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));

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
        Transform::from_xyz(4.0, 0.0, 4.0),
    ));
}

fn run_script_init(runtime: NonSendMut<ScriptRuntime>) {
    runtime.initialize().expect("Lua _init hook failed");
}

fn run_script_update(keyboard: Res<ButtonInput<KeyCode>>, mut runtime: NonSendMut<ScriptRuntime>) {
    let mut just_pressed = Vec::new();
    if keyboard.just_pressed(KeyCode::ArrowUp) {
        just_pressed.push("up");
    }
    if keyboard.just_pressed(KeyCode::ArrowDown) {
        just_pressed.push("down");
    }
    if keyboard.just_pressed(KeyCode::ArrowLeft) {
        just_pressed.push("left");
    }
    if keyboard.just_pressed(KeyCode::ArrowRight) {
        just_pressed.push("right");
    }
    runtime
        .update(&just_pressed)
        .expect("Lua _update hook failed");
}

fn apply_script_commands(
    mut commands: Commands,
    mut runtime: NonSendMut<ScriptRuntime>,
    mut bodies: ResMut<GameBodies>,
    mut body_assets: ResMut<BodyAssets>,
    asset_server: Res<AssetServer>,
    mut camera_view: ResMut<CameraView>,
    mut physics: ResMut<Physics>,
    entities: Query<(Entity, &LogicFrame, &SoftBody)>,
) {
    for command in runtime.drain_commands() {
        match command {
            GameCommand::AddEntity {
                handle,
                asset_name,
                x,
                y,
            } => {
                if bodies.0.iter().any(|body| body.handle == handle) {
                    warn!("Lua attempted to reuse entity handle {handle}");
                    continue;
                }
                let asset_path = if asset_name.ends_with(".glb") {
                    asset_name.clone()
                } else {
                    format!("{asset_name}.glb")
                };
                let asset_handle = body_assets
                    .by_path
                    .entry(asset_path.clone())
                    .or_insert_with(|| asset_server.load(asset_path))
                    .clone();
                bodies.0.push(LogicBody {
                    handle,
                    asset_name,
                    position: Vec3::new(x, 0.0, y),
                    rotation: Quat::IDENTITY,
                    active: true,
                });
                body_assets.handles.push(asset_handle);
                body_assets.spawned.push(false);
            }
            GameCommand::MoveTo { handle, x, y } => {
                if let Some(body) = bodies.0.iter_mut().find(|body| body.handle == handle) {
                    body.position.x = x;
                    body.position.z = y;
                } else {
                    warn!("Lua attempted to move unknown entity handle {handle}");
                }
            }
            GameCommand::SetRotation { handle, degrees } => {
                if let Some(body) = bodies.0.iter_mut().find(|body| body.handle == handle) {
                    body.rotation = Quat::from_rotation_y(degrees.to_radians());
                } else {
                    warn!("Lua attempted to rotate unknown entity handle {handle}");
                }
            }
            GameCommand::RemoveEntity { handle } => {
                let Some(body) = bodies.0.iter_mut().find(|body| body.handle == handle) else {
                    warn!("Lua attempted to remove unknown entity handle {handle}");
                    continue;
                };
                body.active = false;
                for (entity, frame, soft_body) in &entities {
                    if frame.handle == handle {
                        physics.0.remove_soft_body(soft_body.handle);
                        commands.entity(entity).despawn();
                        break;
                    }
                }
            }
            GameCommand::SetCameraView {
                x,
                y,
                width,
                height,
            } => camera_view.0 = Some((x, y, width, height)),
        }
    }
}

fn spawn_loaded_bodies(
    mut commands: Commands,
    bodies: Res<GameBodies>,
    mut body_assets: ResMut<BodyAssets>,
    asset_server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    gltf_meshes: Res<Assets<GltfMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut physics: ResMut<Physics>,
    standard_materials: Res<Assets<StandardMaterial>>,
    mut slime_materials: ResMut<Assets<SlimeMaterial>>,
    slime: Res<SlimeAssets>,
) {
    let handles = body_assets.handles.clone();
    for (index, handle) in handles.iter().enumerate() {
        if body_assets.spawned[index] {
            continue;
        }
        if !bodies.0[index].active {
            body_assets.spawned[index] = true;
            continue;
        }
        let Some(gltf) = gltfs.get(handle) else {
            if let Some(LoadState::Failed(error)) = asset_server.get_load_state(handle.id()) {
                warn!(
                    "failed to load script entity asset {}: {error}",
                    bodies.0[index].asset_name
                );
                body_assets.spawned[index] = true;
            }
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
                let mut copy = source_mesh.clone();
                if ENABLE_SLIME_SHADER {
                    copy.insert_attribute(
                        ATTRIBUTE_REST_POSITION,
                        positions.iter().map(|p| p.to_array()).collect::<Vec<_>>(),
                    );
                }
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

        let asset_path = if body.asset_name.ends_with(".glb") {
            body.asset_name.clone()
        } else {
            format!("{}.glb", body.asset_name)
        };
        if !body_assets.shapes.contains_key(&asset_path) {
            let Some(shape) = prepare_soft_body_shape(&parts) else {
                warn!(
                    "GLB body {} could not be filled with cells",
                    body.asset_name
                );
                continue;
            };
            info!(
                "prepared soft-body shape for {}: primitives={}",
                body.asset_name,
                parts.len()
            );
            body_assets.shapes.insert(asset_path.clone(), shape);
        }

        let vertices: usize = parts.iter().map(|part| part.positions.len()).sum();
        let triangles: usize = parts.iter().map(|part| part.indices.len() / 3).sum();
        let render_meshes: Vec<Handle<Mesh>> = parts.iter().map(|part| part.mesh.clone()).collect();
        let shape = body_assets
            .shapes
            .get(&asset_path)
            .expect("shape was prepared or already cached");
        let Some(soft_body) =
            spawn_soft_body_from_shape(&mut physics, shape, parts, body.position, body.rotation)
        else {
            warn!(
                "GLB body {} could not be spawned from its cached shape",
                body.asset_name
            );
            continue;
        };
        debug!(
            "spawned {} instance: primitives={} vertices={} triangles={}",
            body.asset_name,
            render_meshes.len(),
            vertices,
            triangles
        );
        let body_entity = commands
            .spawn((
                LogicFrame {
                    handle: body.handle,
                },
                soft_body,
                Transform::default(),
                Visibility::default(),
            ))
            .id();
        if ENABLE_SLIME_SHADER {
            // Each body gets its own copies, as the materials carry the body's pose.
            let slime_handles: Vec<Handle<SlimeMaterial>> = materials
                .iter()
                .map(|handle| {
                    let base = standard_materials.get(handle).cloned().unwrap_or_default();
                    slime_materials.add(slime.material(base))
                })
                .collect();
            commands
                .entity(body_entity)
                .insert(SlimeMaterials(slime_handles.clone()));
            commands.entity(body_entity).with_children(|parent| {
                for (mesh, material) in render_meshes.into_iter().zip(slime_handles) {
                    parent.spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(slime.back_material.clone()),
                        RenderLayers::layer(BACK_FACE_LAYER),
                    ));
                    parent.spawn((Mesh3d(mesh), MeshMaterial3d(material)));
                }
            });
        } else {
            commands.entity(body_entity).with_children(|parent| {
                for (mesh, material) in render_meshes.into_iter().zip(materials) {
                    parent.spawn((Mesh3d(mesh), MeshMaterial3d(material)));
                }
            });
        }
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

fn sync_logic_frames(bodies: Res<GameBodies>, mut query: Query<(&LogicFrame, &mut SoftBody)>) {
    if !bodies.is_changed() {
        return;
    }
    for (frame, mut soft_body) in &mut query {
        let Some(body) = bodies.0.iter().find(|body| body.handle == frame.handle) else {
            continue;
        };
        if !body.active {
            continue;
        }
        soft_body.position = body.position;
        soft_body.rotation = body.rotation;
    }
}

fn update_camera_view(
    camera_view: Res<CameraView>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<(&mut Transform, &Projection), With<Camera3d>>,
) {
    if !camera_view.is_changed() {
        return;
    }
    let Some((x, y, width, height)) = camera_view.0 else {
        return;
    };
    let aspect = windows
        .single()
        .map(|window| window.width() / window.height())
        .unwrap_or(1.0)
        .max(f32::EPSILON);
    let visible_span = height.max(width / aspect);
    let center = Vec3::new(x + width * 0.5, 0.0, y + height * 0.5);
    for (mut transform, projection) in &mut cameras {
        let distance = match projection {
            Projection::Perspective(perspective) => {
                visible_span * 0.5 / (perspective.fov * 0.5).tan() * 1.1
            }
            _ => visible_span + 1.0,
        };
        *transform =
            Transform::from_translation(center + Vec3::Y * distance).looking_at(center, -Vec3::Z);
    }
}
