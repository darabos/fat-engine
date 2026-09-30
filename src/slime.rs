use crate::physics::SoftBody;
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
    MaterialPipeline, MaterialPipelineKey,
};
use bevy::prelude::*;
use bevy::render::camera::RenderTarget;
use bevy::render::mesh::{MeshVertexAttribute, MeshVertexBufferLayoutRef};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, Face, RenderPipelineDescriptor, ShaderRef, ShaderType,
    SpecializedMeshPipelineError, TextureDimension, TextureFormat, TextureUsages, VertexFormat,
};
use bevy::render::view::RenderLayers;
use bevy::window::PrimaryWindow;

/// Layer seen only by the back-face camera.
pub const BACK_FACE_LAYER: usize = 1;

/// Undeformed vertex positions, so the skin pattern sticks to the surface while the body wobbles.
pub const ATTRIBUTE_REST_POSITION: MeshVertexAttribute =
    MeshVertexAttribute::new("RestPosition", 988_540_917, VertexFormat::Float32x3);

pub type SlimeMaterial = ExtendedMaterial<StandardMaterial, SlimeExtension>;

#[derive(Clone, Copy, Debug, Reflect, ShaderType)]
pub struct SlimeParams {
    /// Only its rotation is used, to turn rest-pose gradients into world space.
    pub body_from_world: Mat4,
    /// Distance between warts.
    pub wart_spacing: f32,
    pub wart_height: f32,
    /// Size of the dark blotches.
    pub blotch_scale: f32,
    pub blotch_darkness: f32,
    /// Size of the ripples in the slime layer.
    pub ripple_scale: f32,
    pub ripple_strength: f32,
    /// How fast the slime creeps downward, in body units per second.
    pub flow_speed: f32,
    /// Thickness at which light shining through has faded to about a third.
    pub translucency_depth: f32,
    /// Brightness of the faked sky reflected in the slime.
    pub reflection: f32,
}

impl Default for SlimeParams {
    fn default() -> Self {
        Self {
            body_from_world: Mat4::IDENTITY,
            wart_spacing: 0.1,
            wart_height: -0.01,
            blotch_scale: 0.25,
            blotch_darkness: 0.25,
            ripple_scale: 0.15,
            ripple_strength: 0.25,
            flow_speed: 0.03,
            translucency_depth: 0.15,
            reflection: 600.0,
        }
    }
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct SlimeExtension {
    #[uniform(100)]
    pub params: SlimeParams,
    /// World normal and linear view depth of the nearest back face.
    #[texture(101)]
    pub back_faces: Handle<Image>,
}

impl MaterialExtension for SlimeExtension {
    fn vertex_shader() -> ShaderRef {
        "shaders/slime.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/slime.wgsl".into()
    }

    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Appended rather than rebuilt, so prepass and shadow pipelines keep their own locations.
        let rest = layout
            .0
            .get_layout(&[ATTRIBUTE_REST_POSITION.at_shader_location(8)])?;
        descriptor.vertex.buffers[0].attributes.extend(rest.attributes);
        Ok(())
    }
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
pub struct BackFaceMaterial {}

impl Material for BackFaceMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/back_faces.wgsl".into()
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
pub struct SlimeAssets {
    pub back_buffer: Handle<Image>,
    pub back_material: Handle<BackFaceMaterial>,
}

impl SlimeAssets {
    pub fn material(&self, base: StandardMaterial) -> SlimeMaterial {
        SlimeMaterial {
            base: StandardMaterial {
                perceptual_roughness: 0.45,
                reflectance: 0.5,
                metallic: 0.0,
                clearcoat: 1.0,
                clearcoat_perceptual_roughness: 0.04,
                // The shader scales this down where the body is thick.
                diffuse_transmission: 0.6,
                ..base
            },
            extension: SlimeExtension {
                params: SlimeParams::default(),
                back_faces: self.back_buffer.clone(),
            },
        }
    }
}

/// Put this on the camera that should see slime.
#[derive(Component)]
pub struct SlimeCamera;

/// The slime materials of a body, kept in sync with its pose.
#[derive(Component)]
pub struct SlimeMaterials(pub Vec<Handle<SlimeMaterial>>);

pub struct SlimePlugin;

impl Plugin for SlimePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<SlimeMaterial>::default())
            .add_plugins(MaterialPlugin::<BackFaceMaterial> {
                prepass_enabled: false,
                shadows_enabled: false,
                ..default()
            })
            .add_systems(PreStartup, init_slime_assets)
            .add_systems(
                PostUpdate,
                (attach_back_camera, resize_back_buffer, update_slime_pose),
            );
    }
}

fn init_slime_assets(
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
    commands.insert_resource(SlimeAssets {
        back_buffer: images.add(image),
        back_material: back_materials.add(BackFaceMaterial::default()),
    });
}

fn attach_back_camera(
    mut commands: Commands,
    slime: Res<SlimeAssets>,
    cameras: Query<Entity, Added<SlimeCamera>>,
) {
    for camera in &cameras {
        // A child with an identity transform and the default projection sees exactly what its parent sees.
        commands.entity(camera).with_child((
            Camera3d::default(),
            Camera {
                order: -1,
                target: RenderTarget::Image(slime.back_buffer.clone().into()),
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
    slime: Res<SlimeAssets>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<SlimeMaterial>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let size = window.physical_size();
    if size.x == 0 || size.y == 0 {
        return;
    }
    let Some(image) = images.get(&slime.back_buffer) else {
        return;
    };
    if image.size() == size {
        return;
    }
    images.get_mut(&slime.back_buffer).unwrap().resize(Extent3d {
        width: size.x,
        height: size.y,
        depth_or_array_layers: 1,
    });
    // Bind groups hold the old texture until their material is re-prepared.
    for _ in materials.iter_mut() {}
}

fn update_slime_pose(
    bodies: Query<(&SoftBody, &SlimeMaterials), Changed<SoftBody>>,
    mut materials: ResMut<Assets<SlimeMaterial>>,
) {
    for (body, slime) in &bodies {
        let body_from_world =
            Mat4::from_rotation_translation(body.rotation, body.position).inverse();
        for handle in &slime.0 {
            if let Some(material) = materials.get_mut(handle) {
                material.extension.params.body_from_world = body_from_world;
            }
        }
    }
}
