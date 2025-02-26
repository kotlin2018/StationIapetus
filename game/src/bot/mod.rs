use crate::{
    bot::{
        behavior::{BehaviorContext, BotBehavior},
        lower_body::{LowerBodyMachine, LowerBodyMachineInput},
        upper_body::{UpperBodyMachine, UpperBodyMachineInput},
    },
    character::{Character, CharacterCommand},
    current_level_mut, current_level_ref,
    door::{door_mut, door_ref, DoorContainer},
    game_ref,
    inventory::{Inventory, ItemEntry},
    level::item::ItemKind,
    sound::SoundManager,
    utils::{is_probability_event_occurred, BodyImpactHandler},
    weapon::projectile::Damage,
    Weapon,
};
use fyrox::{
    animation::machine::{Machine, PoseNode},
    core::{
        algebra::{Point3, UnitQuaternion, Vector3},
        arrayvec::ArrayVec,
        color::Color,
        futures::executor::block_on,
        inspect::prelude::*,
        math::SmoothAngle,
        pool::Handle,
        rand::{seq::IteratorRandom, Rng},
        reflect::Reflect,
        uuid::{uuid, Uuid},
        visitor::{Visit, VisitResult, Visitor},
    },
    engine::resource_manager::ResourceManager,
    impl_component_provider,
    lazy_static::lazy_static,
    rand,
    rand::prelude::SliceRandom,
    scene::{
        self,
        debug::SceneDrawingContext,
        graph::{
            physics::{Intersection, RayCastOptions},
            Graph,
        },
        node::{Node, TypeUuidProvider},
        rigidbody::RigidBody,
        Scene,
    },
    script::{ScriptContext, ScriptDeinitContext, ScriptTrait},
    utils::navmesh::{NavmeshAgent, NavmeshAgentBuilder},
};
use serde::Deserialize;
use std::{
    collections::{HashMap, VecDeque},
    fs::File,
    ops::{Deref, DerefMut},
};
use strum_macros::{AsRefStr, EnumString, EnumVariantNames};

mod behavior;
mod lower_body;
mod upper_body;

#[derive(
    Deserialize,
    Copy,
    Clone,
    PartialEq,
    Eq,
    Hash,
    Debug,
    Visit,
    Reflect,
    Inspect,
    AsRefStr,
    EnumString,
    EnumVariantNames,
)]
#[repr(i32)]
pub enum BotKind {
    Mutant = 0,
    Parasite = 1,
    Zombie = 2,
}

impl BotKind {
    pub fn description(self) -> &'static str {
        match self {
            BotKind::Mutant => "Mutant",
            BotKind::Parasite => "Parasite",
            BotKind::Zombie => "Zombie",
        }
    }
}

#[derive(Deserialize, Copy, Clone, PartialOrd, PartialEq, Ord, Eq, Hash, Debug)]
#[repr(u32)]
pub enum BotHostility {
    Everyone = 0,
    OtherSpecies = 1,
    Player = 2,
}

#[derive(Debug, Visit, Default, Clone)]
pub struct Target {
    position: Vector3<f32>,
    handle: Handle<Node>,
}

#[derive(Debug, Clone)]
pub enum BotCommand {
    HandleImpact {
        handle: Handle<Node>,
        impact_point: Vector3<f32>,
        direction: Vector3<f32>,
    },
}

#[derive(Visit, Reflect, Inspect, Debug, Clone)]
pub struct Bot {
    #[reflect(hidden)]
    #[inspect(skip)]
    target: Option<Target>,
    pub kind: BotKind,
    model: Handle<Node>,
    character: Character,
    #[visit(skip)]
    #[reflect(hidden)]
    #[inspect(skip)]
    pub definition: &'static BotDefinition,
    #[reflect(hidden)]
    #[inspect(skip)]
    lower_body_machine: LowerBodyMachine,
    #[reflect(hidden)]
    #[inspect(skip)]
    upper_body_machine: UpperBodyMachine,
    pub restoration_time: f32,
    hips: Handle<Node>,
    #[reflect(hidden)]
    #[inspect(skip)]
    agent: NavmeshAgent,
    head_exploded: bool,
    #[visit(skip)]
    #[reflect(hidden)]
    #[inspect(skip)]
    pub impact_handler: BodyImpactHandler,
    #[reflect(hidden)]
    #[inspect(skip)]
    behavior: BotBehavior,
    v_recoil: SmoothAngle,
    h_recoil: SmoothAngle,
    spine: Handle<Node>,
    move_speed: f32,
    target_move_speed: f32,
    threaten_timeout: f32,
    #[visit(skip)]
    #[reflect(hidden)]
    #[inspect(skip)]
    pub commands_queue: VecDeque<BotCommand>,
}

impl_component_provider!(Bot, character: Character);

impl Deref for Bot {
    type Target = Character;

    fn deref(&self) -> &Self::Target {
        &self.character
    }
}

impl DerefMut for Bot {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.character
    }
}

impl Default for Bot {
    fn default() -> Self {
        Self {
            character: Default::default(),
            kind: BotKind::Mutant,
            model: Default::default(),
            target: Default::default(),
            definition: Self::get_definition(BotKind::Mutant),
            lower_body_machine: Default::default(),
            upper_body_machine: Default::default(),
            restoration_time: 0.0,
            hips: Default::default(),
            agent: Default::default(),
            head_exploded: false,
            impact_handler: Default::default(),
            behavior: Default::default(),
            v_recoil: Default::default(),
            h_recoil: Default::default(),
            spine: Default::default(),
            move_speed: 0.0,
            target_move_speed: 0.0,
            threaten_timeout: 0.0,
            commands_queue: Default::default(),
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct AttackAnimationDefinition {
    path: String,
    stick_timestamp: f32,
    timestamp: f32,
    damage: Damage,
    speed: f32,
}

#[derive(Deserialize, Debug)]
pub struct BotDefinition {
    pub scale: f32,
    pub health: f32,
    pub walk_speed: f32,
    pub weapon_scale: f32,
    pub model: String,
    pub weapon_hand_name: String,
    pub left_leg_name: String,
    pub right_leg_name: String,
    pub spine: String,
    pub head_name: String,
    pub hips: String,
    pub v_aim_angle_hack: f32,
    pub can_use_weapons: bool,
    pub close_combat_distance: f32,
    pub pain_sounds: Vec<String>,
    pub scream_sounds: Vec<String>,
    pub idle_sounds: Vec<String>,
    pub attack_sounds: Vec<String>,
    pub hostility: BotHostility,

    // Animations.
    pub idle_animation: String,
    pub scream_animation: String,
    pub attack_animations: Vec<AttackAnimationDefinition>,
    pub walk_animation: String,
    pub aim_animation: String,
    pub dying_animation: String,
}

#[derive(Deserialize, Default)]
pub struct BotDefinitionsContainer {
    map: HashMap<BotKind, BotDefinition>,
}

impl BotDefinitionsContainer {
    pub fn new() -> Self {
        let file = File::open("data/configs/bots.ron").unwrap();
        ron::de::from_reader(file).unwrap()
    }
}

lazy_static! {
    static ref DEFINITIONS: BotDefinitionsContainer = BotDefinitionsContainer::new();
}

impl Bot {
    pub fn get_definition(kind: BotKind) -> &'static BotDefinition {
        DEFINITIONS.map.get(&kind).unwrap()
    }

    pub fn add_to_scene(
        scene: &mut Scene,
        kind: BotKind,
        resource_manager: &ResourceManager,
        position: Vector3<f32>,
        rotation: UnitQuaternion<f32>,
    ) -> Handle<Node> {
        let bot =
            block_on(resource_manager.request_model(Self::get_definition(kind).model.clone()))
                .unwrap()
                .instantiate_geometry(scene);

        let node = &mut scene.graph[bot];

        assert!(node.has_script::<Bot>());

        node.local_transform_mut()
            .set_position(position)
            .set_rotation(rotation);

        bot
    }

    #[allow(clippy::unnecessary_to_owned)] // false positive
    fn check_doors(&mut self, scene: &mut Scene, door_container: &DoorContainer) {
        if let Some(target) = self.target.as_ref() {
            let mut query_storage = ArrayVec::<Intersection, 64>::new();

            let position = self.position(&scene.graph);
            let ray_direction = target.position - position;

            scene.graph.physics.cast_ray(
                RayCastOptions {
                    ray_origin: Point3::from(position),
                    ray_direction,
                    max_len: ray_direction.norm(),
                    groups: Default::default(),
                    sort_results: true,
                },
                &mut query_storage,
            );

            for intersection in query_storage {
                for &door_handle in &door_container.doors {
                    let door = door_ref(door_handle, &scene.graph);

                    let close_enough = position.metric_distance(&door.initial_position()) < 1.25;
                    if !close_enough {
                        continue;
                    }

                    for child in scene.graph[door_handle].children().to_vec() {
                        if let Some(rigid_body) = scene.graph[child].cast::<RigidBody>() {
                            for collider in rigid_body.children().to_vec() {
                                if collider == intersection.collider {
                                    let has_key = self.inventory.has_key();
                                    door_mut(door_handle, &mut scene.graph).try_open(has_key);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn can_be_removed(&self, scene: &Scene) -> bool {
        scene
            .animations
            .get(self.upper_body_machine.dying_animation)
            .has_ended()
    }

    pub fn debug_draw(&self, context: &mut SceneDrawingContext) {
        for pts in self.agent.path().windows(2) {
            let a = pts[0];
            let b = pts[1];
            context.add_line(scene::debug::Line {
                begin: a,
                end: b,
                color: Color::from_rgba(255, 0, 0, 255),
            });
        }

        // context.draw_frustum(&self.frustum, Color::from_rgba(0, 200, 0, 255)); TODO
    }

    pub fn set_target(&mut self, handle: Handle<Node>, position: Vector3<f32>) {
        self.target = Some(Target { position, handle });
    }

    pub fn blow_up_head(&mut self, _graph: &mut Graph) {
        self.head_exploded = true;

        // TODO: Add effect.
    }

    pub fn clean_up(&mut self, scene: &mut Scene) {
        self.upper_body_machine.clean_up(scene);
        self.lower_body_machine.clean_up(scene);
        self.character.clean_up(scene);
    }

    pub fn on_actor_removed(&mut self, handle: Handle<Node>) {
        if let Some(target) = self.target.as_ref() {
            if target.handle == handle {
                self.target = None;
            }
        }
    }

    pub fn resolve(&mut self) {
        self.definition = Self::get_definition(self.kind);
    }

    fn poll_commands(
        &mut self,
        scene: &mut Scene,
        self_handle: Handle<Node>,
        resource_manager: &ResourceManager,
        sound_manager: &SoundManager,
    ) {
        while let Some(command) =
            self.character
                .poll_command(scene, self_handle, resource_manager, sound_manager)
        {
            if let CharacterCommand::Damage {
                who,
                amount,
                hitbox,
                critical_shot_probability,
            } = command
            {
                if let Some(shooter_script) = scene.graph.try_get(who).and_then(|n| n.script()) {
                    if let Some(character) = shooter_script.query_component_ref::<Character>() {
                        self.set_target(who, character.position(&scene.graph));
                    } else if let Some(weapon) = shooter_script.query_component_ref::<Weapon>() {
                        if let Some(weapon_owner_script) =
                            scene.graph.try_get(weapon.owner()).and_then(|n| n.script())
                        {
                            if let Some(character_owner) =
                                weapon_owner_script.query_component_ref::<Character>()
                            {
                                self.set_target(
                                    weapon.owner(),
                                    character_owner.position(&scene.graph),
                                );
                            }
                        }
                    }
                }

                if let Some(hitbox) = hitbox {
                    // Handle critical head shots.
                    let critical_head_shot_probability = critical_shot_probability.clamp(0.0, 1.0); // * 100.0%
                    if hitbox.is_head
                        && is_probability_event_occurred(critical_head_shot_probability)
                    {
                        self.damage(amount * 1000.0);

                        self.blow_up_head(&mut scene.graph);
                    }
                }

                // Prevent spamming with grunt sounds.
                if self.last_health - self.health > 20.0 && !self.is_dead() {
                    self.last_health = self.health;
                    self.restoration_time = 0.8;

                    if let Some(grunt_sound) =
                        self.definition.pain_sounds.choose(&mut rand::thread_rng())
                    {
                        let position = self.position(&scene.graph);
                        sound_manager.play_sound(
                            &mut scene.graph,
                            grunt_sound,
                            position,
                            0.8,
                            1.0,
                            0.6,
                        );
                    }
                }
            }
        }

        while let Some(bot_command) = self.commands_queue.pop_front() {
            match bot_command {
                BotCommand::HandleImpact {
                    handle,
                    impact_point,
                    direction,
                } => self
                    .impact_handler
                    .handle_impact(scene, handle, impact_point, direction),
            }
        }
    }
}

fn clean_machine(machine: &Machine, scene: &mut Scene) {
    for node in machine.nodes() {
        if let PoseNode::PlayAnimation(node) = node {
            scene.animations.remove(node.animation);
        }
    }
}

impl TypeUuidProvider for Bot {
    fn type_uuid() -> Uuid {
        uuid!("15a8ecd6-a09f-4c5d-b9f9-b7f0e8a44ac9")
    }
}

impl ScriptTrait for Bot {
    fn on_init(&mut self, context: &mut ScriptContext) {
        self.definition = Self::get_definition(self.kind);

        self.lower_body_machine = block_on(LowerBodyMachine::new(
            context.resource_manager.clone(),
            self.definition,
            self.model,
            context.scene,
        ));
        self.upper_body_machine = block_on(UpperBodyMachine::new(
            context.resource_manager.clone(),
            self.definition,
            self.model,
            context.scene,
            self.hips,
        ));

        let possible_item = [
            (ItemKind::Ammo, 10),
            (ItemKind::Medkit, 1),
            (ItemKind::Medpack, 1),
        ];
        let mut items =
            if let Some((item, count)) = possible_item.iter().choose(&mut rand::thread_rng()) {
                vec![ItemEntry {
                    kind: *item,
                    amount: *count,
                }]
            } else {
                Default::default()
            };

        if self.definition.can_use_weapons {
            items.push(ItemEntry {
                kind: ItemKind::Ammo,
                amount: rand::thread_rng().gen_range(32..96),
            });
        }

        self.inventory = Inventory::from_inner(items);

        self.agent = NavmeshAgentBuilder::new()
            .with_position(context.scene.graph[context.handle].global_position())
            .with_speed(self.definition.walk_speed)
            .build();
        self.behavior = BotBehavior::new(self.spine, self.definition);

        current_level_mut(context.plugins)
            .unwrap()
            .actors
            .push(context.handle);
    }

    fn on_start(&mut self, _ctx: &mut ScriptContext) {
        self.definition = Self::get_definition(self.kind);
    }

    fn on_deinit(&mut self, context: &mut ScriptDeinitContext) {
        if let Some(level) = current_level_mut(context.plugins) {
            if let Some(position) = level.actors.iter().position(|a| *a == context.node_handle) {
                level.actors.remove(position);
            }
        }
    }

    fn on_update(&mut self, ctx: &mut ScriptContext) {
        let game = game_ref(ctx.plugins);
        let level = current_level_ref(ctx.plugins).unwrap();

        self.poll_commands(
            ctx.scene,
            ctx.handle,
            ctx.resource_manager,
            &level.sound_manager,
        );

        let movement_speed_factor;
        let is_attacking;
        let is_moving;
        let is_aiming;
        let attack_animation_index;
        let is_screaming;
        {
            let mut behavior_ctx = BehaviorContext {
                scene: ctx.scene,
                actors: &level.actors,
                bot_handle: ctx.handle,
                sender: &game.message_sender,
                dt: ctx.dt,
                elapsed_time: ctx.elapsed_time,
                upper_body_machine: &self.upper_body_machine,
                lower_body_machine: &self.lower_body_machine,
                target: &mut self.target,
                definition: self.definition,
                character: &mut self.character,
                kind: self.kind,
                agent: &mut self.agent,
                impact_handler: &self.impact_handler,
                model: self.model,
                restoration_time: self.restoration_time,
                v_recoil: &mut self.v_recoil,
                h_recoil: &mut self.h_recoil,
                target_move_speed: &mut self.target_move_speed,
                move_speed: self.move_speed,
                threaten_timeout: &mut self.threaten_timeout,
                sound_manager: &level.sound_manager,

                // Output
                attack_animation_index: 0,
                movement_speed_factor: 1.0,
                is_moving: false,
                is_attacking: false,
                is_aiming_weapon: false,
                is_screaming: false,
            };

            self.behavior.tree.tick(&mut behavior_ctx);

            movement_speed_factor = behavior_ctx.movement_speed_factor;
            is_attacking = behavior_ctx.is_attacking;
            is_moving = behavior_ctx.is_moving;
            is_aiming = behavior_ctx.is_aiming_weapon;
            attack_animation_index = behavior_ctx.attack_animation_index;
            is_screaming = behavior_ctx.is_screaming;
        }

        self.restoration_time -= ctx.dt;
        self.move_speed += (self.target_move_speed - self.move_speed) * 0.1;
        self.threaten_timeout -= ctx.dt;

        self.check_doors(ctx.scene, &level.doors_container);

        self.lower_body_machine.apply(
            ctx.scene,
            ctx.dt,
            LowerBodyMachineInput {
                walk: is_moving,
                scream: is_screaming,
                dead: self.is_dead(),
                movement_speed_factor,
            },
        );

        self.upper_body_machine.apply(
            ctx.scene,
            ctx.dt,
            UpperBodyMachineInput {
                attack: is_attacking,
                walk: is_moving,
                scream: is_screaming,
                dead: self.is_dead(),
                aim: is_aiming,
                attack_animation_index: attack_animation_index as u32,
            },
        );
        self.impact_handler.update_and_apply(ctx.dt, ctx.scene);

        self.v_recoil.update(ctx.dt);
        self.h_recoil.update(ctx.dt);

        let spine_transform = ctx.scene.graph[self.spine].local_transform_mut();
        let rotation = **spine_transform.rotation();
        spine_transform.set_rotation(
            rotation
                * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), self.v_recoil.angle())
                * UnitQuaternion::from_axis_angle(&Vector3::y_axis(), self.h_recoil.angle()),
        );

        if self.head_exploded {
            let head = ctx
                .scene
                .graph
                .find_by_name(self.model, &self.definition.head_name);
            if head.is_some() {
                ctx.scene.graph[head]
                    .local_transform_mut()
                    .set_scale(Vector3::new(0.0, 0.0, 0.0));
            }
        }
    }

    fn id(&self) -> Uuid {
        Self::type_uuid()
    }
}

pub fn try_get_bot_mut(handle: Handle<Node>, graph: &mut Graph) -> Option<&mut Bot> {
    graph
        .try_get_mut(handle)
        .and_then(|b| b.try_get_script_mut::<Bot>())
}
