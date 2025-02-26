use fyrox::{
    core::{
        algebra::{Point3, UnitQuaternion, Vector3},
        color::Color,
        inspect::prelude::*,
        math::vector_to_quat,
        pool::Handle,
        reflect::Reflect,
        uuid::{uuid, Uuid},
        visitor::prelude::*,
    },
    engine::resource_manager::ResourceManager,
    impl_component_provider,
    resource::texture::Texture,
    scene::{
        base::BaseBuilder,
        decal::DecalBuilder,
        graph::Graph,
        node::{Node, TypeUuidProvider},
        transform::TransformBuilder,
    },
    script::{Script, ScriptContext, ScriptTrait},
};

#[derive(Visit, Reflect, Inspect, Debug, Clone)]
pub struct Decal {
    lifetime: f32,
    fade_interval: f32,
}

impl Default for Decal {
    fn default() -> Self {
        Self {
            lifetime: 10.0,
            fade_interval: 1.0,
        }
    }
}

impl_component_provider!(Decal);

impl TypeUuidProvider for Decal {
    fn type_uuid() -> Uuid {
        uuid!("e7710ced-9c3f-4ea6-9874-a6d35a7a86f3")
    }
}

impl ScriptTrait for Decal {
    fn on_update(&mut self, ctx: &mut ScriptContext) {
        self.lifetime -= ctx.dt;

        let abs_lifetime = self.lifetime.abs();

        let alpha = if self.lifetime <= 0.0 {
            1.0 - (abs_lifetime / self.fade_interval).min(1.0)
        } else {
            1.0
        };

        let decal_node = ctx.scene.graph[ctx.handle].as_decal_mut();

        decal_node.set_color(decal_node.color().with_new_alpha((255.0 * alpha) as u8));

        if self.lifetime < 0.0 && abs_lifetime > self.fade_interval {
            ctx.scene.graph.remove_node(ctx.handle);
        }
    }

    fn id(&self) -> Uuid {
        Self::type_uuid()
    }
}

impl Decal {
    pub fn add_to_graph(
        graph: &mut Graph,
        position: Vector3<f32>,
        face_towards: Vector3<f32>,
        parent: Handle<Node>,
        color: Color,
        scale: Vector3<f32>,
        texture: Texture,
    ) -> Handle<Node> {
        let (position, face_towards, scale) = if parent.is_some() {
            let parent_scale = graph.global_scale(parent);

            let parent_inv_transform = graph[parent]
                .global_transform()
                .try_inverse()
                .unwrap_or_default();

            (
                parent_inv_transform
                    .transform_point(&Point3::from(position))
                    .coords,
                parent_inv_transform.transform_vector(&face_towards),
                // Discard parent's scale.
                Vector3::new(
                    scale.x / parent_scale.x,
                    scale.y / parent_scale.y,
                    scale.z / parent_scale.z,
                ),
            )
        } else {
            (position, face_towards, scale)
        };

        let rotation = vector_to_quat(face_towards)
            * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), 90.0f32.to_radians());

        let decal = DecalBuilder::new(
            BaseBuilder::new()
                .with_local_transform(
                    TransformBuilder::new()
                        .with_local_position(position)
                        .with_local_rotation(rotation)
                        .with_local_scale(scale)
                        .build(),
                )
                .with_script(Script::new(Decal::default())),
        )
        .with_diffuse_texture(texture)
        .with_color(color)
        .build(graph);

        if decal.is_some() && parent.is_some() {
            graph.link_nodes(decal, parent);
        }

        decal
    }

    pub fn new_bullet_hole(
        resource_manager: &ResourceManager,
        graph: &mut Graph,
        position: Vector3<f32>,
        face_towards: Vector3<f32>,
        parent: Handle<Node>,
        color: Color,
    ) -> Handle<Node> {
        let default_scale = Vector3::new(0.05, 0.05, 0.05);

        Self::add_to_graph(
            graph,
            position,
            face_towards,
            parent,
            color,
            default_scale,
            resource_manager.request_texture("data/textures/decals/BulletImpact_BaseColor.png"),
        )
    }
}
