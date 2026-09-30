use crate::physics::SoftBody;
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialPipeline, MaterialPipelineKey,
};
use bevy::prelude::*;
use bevy::render::camera::RenderTarget;
use bevy::render::mesh::MeshVertexBufferLayoutRef;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, Face, RenderPipelineDescriptor, ShaderRef, ShaderType,
    SpecializedMeshPipelineError, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::view::RenderLayers;
use bevy::window::PrimaryWindow;

/// Layer seen only by the back-face camera.
pub const BACK_FACE_LAYER: usize = 1;

pub type FurMaterial = ExtendedMaterial<StandardMaterial, FurExtension>;

#[derive(Clone, Copy, Debug, Reflect, ShaderType)]
pub struct FurParams {
    /// Hair is laid out in body space so it stays put while the body moves.
    pub body_from_world: Mat4,
    pub fur_depth: f32,
    pub strand_spacing: f32,
    pub strand_radius: f32,
    /// Brightness at the roots relative to the tips.
    pub root_shade: f32,
    /// How far individual hairs stray from the combed direction.
    pub messiness: f32,
}

impl Default for FurParams {
    fn default() -> Self {
        Self {
            body_from_world: Mat4::IDENTITY,
            fur_depth: 0.08,
            strand_spacing: 0.02,
            strand_radius: 0.004,
            root_shade: 0.35,
            messiness: 0.4,
        }
    }
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct FurExtension {
    #[uniform(100)]
    pub params: FurParams,
    /// World normal and linear view depth of the nearest back face.
    #[texture(101)]
    pub back_faces: Handle<Image>,
}

impl MaterialExtension for FurExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/fur.wgsl".into()
    }
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
pub struct BackFaceMaterial {}

impl Material for BackFaceMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/fur_back.wgsl".into()
    }

    fn specialize(
        _pipeline: &MaterialPipeline<Self>,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = Some(Face::Front);
        Ok(())
    }
}

#[derive(Resource, Clone)]
pub struct FurAssets {
    pub back_buffer: Handle<Image>,
    pub back_material: Handle<BackFaceMaterial>,
}

impl FurAssets {
    pub fn material(&self, mut base: StandardMaterial) -> FurMaterial {
        base.alpha_mode = AlphaMode::Blend;
        FurMaterial {
            base,
            extension: FurExtension {
                params: FurParams::default(),
                back_faces: self.back_buffer.clone(),
            },
        }
    }
}

/// Put this on the camera that should see fur.
#[derive(Component)]
pub struct FurCamera;

/// The fur materials of a body, kept in sync with its pose.
#[derive(Component)]
pub struct FurMaterials(pub Vec<Handle<FurMaterial>>);

pub struct FurPlugin;

impl Plugin for FurPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<FurMaterial>::default())
            .add_plugins(MaterialPlugin::<BackFaceMaterial> {
                prepass_enabled: false,
                shadows_enabled: false,
                ..default()
            })
            .add_systems(PreStartup, init_fur_assets)
            .add_systems(
                PostUpdate,
                (attach_back_camera, resize_back_buffer, update_fur_pose),
            );
    }
}

fn init_fur_assets(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut back_materials: ResMut<Assets<BackFaceMaterial>>,
) {
    let mut image = Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0; 8],
        TextureFormat::Rgba16Float,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    commands.insert_resource(FurAssets {
        back_buffer: images.add(image),
        back_material: back_materials.add(BackFaceMaterial::default()),
    });
}

fn attach_back_camera(
    mut commands: Commands,
    fur: Res<FurAssets>,
    cameras: Query<Entity, Added<FurCamera>>,
) {
    for camera in &cameras {
        // A child with an identity transform and the default projection sees exactly what its parent sees.
        commands.entity(camera).with_child((
            Camera3d::default(),
            Camera {
                order: -1,
                target: RenderTarget::Image(fur.back_buffer.clone().into()),
                clear_color: ClearColorConfig::Custom(Color::NONE),
                // HDR keeps the intermediate texture in Rgba16Float instead of 8-bit sRGB.
                hdr: true,
                ..default()
            },
            Tonemapping::None,
            DebandDither::Disabled,
            Msaa::Off,
            RenderLayers::layer(BACK_FACE_LAYER),
        ));
    }
}

fn resize_back_buffer(
    fur: Res<FurAssets>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<FurMaterial>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let size = window.physical_size();
    if size.x == 0 || size.y == 0 {
        return;
    }
    let Some(image) = images.get(&fur.back_buffer) else {
        return;
    };
    if image.size() == size {
        return;
    }
    images.get_mut(&fur.back_buffer).unwrap().resize(Extent3d {
        width: size.x,
        height: size.y,
        depth_or_array_layers: 1,
    });
    // Bind groups hold the old texture until their material is re-prepared.
    for _ in materials.iter_mut() {}
}

fn update_fur_pose(
    bodies: Query<(&SoftBody, &FurMaterials), Changed<SoftBody>>,
    mut materials: ResMut<Assets<FurMaterial>>,
) {
    for (body, fur) in &bodies {
        let body_from_world =
            Mat4::from_rotation_translation(body.rotation, body.position).inverse();
        for handle in &fur.0 {
            if let Some(material) = materials.get_mut(handle) {
                material.extension.params.body_from_world = body_from_world;
            }
        }
    }
}
