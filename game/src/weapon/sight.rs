use crate::CollisionGroups;
use fyrox::{
    core::{
        algebra::{Point3, UnitQuaternion, Vector3},
        arrayvec::ArrayVec,
        color::Color,
        inspect::prelude::*,
        math::{lerpf, ray::Ray},
        pool::Handle,
        reflect::Reflect,
        sstorage::ImmutableString,
        visitor::prelude::*,
    },
    engine::resource_manager::ResourceManager,
    material::{Material, PropertyValue, SharedMaterial},
    scene::{
        base::BaseBuilder,
        collider::{BitMask, InteractionGroups},
        graph::{physics::RayCastOptions, Graph},
        light::{point::PointLightBuilder, BaseLight, BaseLightBuilder},
        mesh::{
            surface::{SurfaceBuilder, SurfaceData, SurfaceSharedData},
            MeshBuilder, RenderPath,
        },
        node::Node,
        sprite::SpriteBuilder,
        Scene,
    },
    utils::log::Log,
};

#[derive(Visit, Reflect, Inspect, Default, Debug, Clone)]
pub struct LaserSight {
    ray: Handle<Node>,
    tip: Handle<Node>,
    light: Handle<Node>,
    pub enabled: bool,

    #[reflect(hidden)]
    #[inspect(skip)]
    reaction_state: Option<ReactionState>,
}

#[derive(Visit, Reflect, Inspect, Debug, Clone)]
pub enum ReactionState {
    HitDetected {
        time_remaining: f32,
        begin_color: Color,
        end_color: Color,
    },
    EnemyKilled {
        time_remaining: f32,
        dilation_factor: f32,
        begin_color: Color,
        end_color: Color,
    },
}

impl Default for ReactionState {
    fn default() -> Self {
        Self::HitDetected {
            time_remaining: 0.0,
            begin_color: Default::default(),
            end_color: Default::default(),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum SightReaction {
    HitDetected,
    EnemyKilled,
}

const NORMAL_COLOR: Color = Color::from_rgba(0, 162, 232, 200);
const NORMAL_RADIUS: f32 = 0.0012;
const ENEMY_KILLED_TIME: f32 = 0.55;
const HIT_DETECTED_TIME: f32 = 0.4;

impl LaserSight {
    pub fn new(scene: &mut Scene, resource_manager: ResourceManager) -> Self {
        let ray = MeshBuilder::new(
            BaseBuilder::new()
                .with_cast_shadows(false)
                .with_visibility(false),
        )
        .with_surfaces(vec![SurfaceBuilder::new(SurfaceSharedData::new(
            SurfaceData::make_cylinder(
                6,
                1.0,
                1.0,
                false,
                &UnitQuaternion::from_axis_angle(&Vector3::x_axis(), 90.0f32.to_radians())
                    .to_homogeneous(),
            ),
        ))
        .with_material(SharedMaterial::new({
            let mut material = Material::standard();
            Log::verify(material.set_property(
                &ImmutableString::new("diffuseColor"),
                PropertyValue::Color(NORMAL_COLOR),
            ));
            material
        }))
        .build()])
        .with_render_path(RenderPath::Forward)
        .build(&mut scene.graph);

        let light;
        let tip = SpriteBuilder::new(BaseBuilder::new().with_visibility(false).with_children(&[{
            light = PointLightBuilder::new(
                BaseLightBuilder::new(BaseBuilder::new())
                    .cast_shadows(false)
                    .with_scatter_enabled(false)
                    .with_color(NORMAL_COLOR),
            )
            .with_radius(0.30)
            .build(&mut scene.graph);
            light
        }]))
        .with_texture(resource_manager.request_texture("data/particles/star_09.png"))
        .with_color(NORMAL_COLOR)
        .with_size(0.025)
        .build(&mut scene.graph);

        Self {
            ray,
            tip,
            light,
            enabled: true,
            reaction_state: None,
        }
    }

    pub fn update(
        &mut self,
        scene: &mut Scene,
        position: Vector3<f32>,
        direction: Vector3<f32>,
        ignore_collider: Handle<Node>,
        dt: f32,
    ) {
        scene.graph[self.tip].set_visibility(self.enabled);
        scene.graph[self.ray].set_visibility(self.enabled);

        let mut intersections = ArrayVec::<_, 64>::new();

        let max_toi = 100.0;

        let ray = Ray::new(position, direction.scale(max_toi));

        scene.graph.physics.cast_ray(
            RayCastOptions {
                ray_origin: Point3::from(ray.origin),
                ray_direction: ray.dir,
                max_len: max_toi,
                groups: InteractionGroups::new(
                    BitMask(0xFFFF),
                    BitMask(!(CollisionGroups::ActorCapsule as u32)),
                ),
                sort_results: true,
            },
            &mut intersections,
        );

        let ray_node = &mut scene.graph[self.ray];
        if let Some(result) = intersections
            .into_iter()
            .find(|i| i.collider != ignore_collider)
        {
            ray_node
                .local_transform_mut()
                .set_position(position)
                .set_rotation(UnitQuaternion::face_towards(&direction, &Vector3::y()))
                .set_scale(Vector3::new(NORMAL_RADIUS, NORMAL_RADIUS, result.toi));

            scene.graph[self.tip]
                .local_transform_mut()
                .set_position(result.position.coords - direction.scale(0.02));
        }

        if let Some(reaction_state) = self.reaction_state.as_mut() {
            match reaction_state {
                ReactionState::HitDetected {
                    time_remaining,
                    begin_color,
                    end_color,
                } => {
                    *time_remaining -= dt;
                    if *time_remaining <= 0.0 {
                        self.reaction_state = None;
                    } else {
                        let t = *time_remaining / HIT_DETECTED_TIME;
                        let color = end_color.lerp(*begin_color, t);
                        self.set_color(&mut scene.graph, color);
                    }
                }
                ReactionState::EnemyKilled {
                    time_remaining,
                    dilation_factor,
                    begin_color,
                    end_color,
                } => {
                    *time_remaining -= dt;
                    if *time_remaining <= 0.0 {
                        self.reaction_state = None;
                    } else {
                        let t = *time_remaining / HIT_DETECTED_TIME;
                        let color = end_color.lerp(*begin_color, t);
                        let dilation_factor = lerpf(1.0, *dilation_factor, t);
                        self.set_color(&mut scene.graph, color);
                        self.dilate(&mut scene.graph, dilation_factor);
                    }
                }
            }
        }
    }

    pub fn set_reaction(&mut self, reaction: SightReaction) {
        self.reaction_state = Some(match reaction {
            SightReaction::HitDetected => ReactionState::HitDetected {
                time_remaining: HIT_DETECTED_TIME,
                begin_color: Color::from_rgba(200, 0, 0, 200),
                end_color: NORMAL_COLOR,
            },
            SightReaction::EnemyKilled => ReactionState::EnemyKilled {
                time_remaining: ENEMY_KILLED_TIME,
                dilation_factor: 1.1,
                begin_color: Color::from_rgba(255, 0, 0, 200),
                end_color: NORMAL_COLOR,
            },
        });
    }

    fn set_color(&self, graph: &mut Graph, color: Color) {
        Log::verify(
            graph[self.ray]
                .as_mesh_mut()
                .surfaces()
                .first()
                .unwrap()
                .material()
                .lock()
                .set_property(
                    &ImmutableString::new("diffuseColor"),
                    PropertyValue::Color(color),
                ),
        );
        graph[self.light]
            .query_component_mut::<BaseLight>()
            .unwrap()
            .set_color(color);
        graph[self.tip].as_sprite_mut().set_color(color);
    }

    fn dilate(&self, graph: &mut Graph, factor: f32) {
        let transform = graph[self.ray].local_transform_mut();
        let scale = **transform.scale();
        transform.set_scale(Vector3::new(
            NORMAL_RADIUS * factor,
            NORMAL_RADIUS * factor,
            scale.z,
        ));
    }

    pub fn clean_up(&mut self, scene: &mut Scene) {
        scene.graph.remove_node(self.ray);
        scene.graph.remove_node(self.tip);
    }
}
