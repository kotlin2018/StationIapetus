use crate::{
    bot::{behavior::BehaviorContext, upper_body::UpperBodyMachine, BotDefinition},
    character::{try_get_character_mut, CharacterCommand},
};
use fyrox::{
    asset::core::rand::prelude::IteratorRandom,
    core::{rand::Rng, visitor::prelude::*},
    utils::behavior::{Behavior, Status},
};

#[derive(Default, Debug, PartialEq, Visit, Clone)]
pub struct DoMeleeAttack {
    attack_timeout: f32,
    attack_animation_index: u32,
}

fn can_shoot(upper_body_machine: &UpperBodyMachine, definition: &BotDefinition) -> bool {
    upper_body_machine.machine.active_state() == upper_body_machine.aim_state
        && definition.can_use_weapons
}

impl<'a> Behavior<'a> for DoMeleeAttack {
    type Context = BehaviorContext<'a>;

    fn tick(&mut self, context: &mut Self::Context) -> Status {
        let current_attack_animation =
            context.upper_body_machine.attack_animations[self.attack_animation_index as usize];

        let attack_animation = context.scene.animations.get_mut(current_attack_animation);
        let attack_animation_ended = attack_animation.has_ended();

        if self.attack_timeout <= 0.0 && (attack_animation_ended || !attack_animation.is_enabled())
        {
            // HACK: setting this to false messes up animation, so set speed to 0.0.
            attack_animation.set_enabled(true).set_speed(0.0).rewind();

            self.attack_animation_index = fyrox::core::rand::thread_rng()
                .gen_range(0..context.upper_body_machine.attack_animations.len())
                as u32;

            context
                .scene
                .animations
                .get_mut(
                    context.upper_body_machine.attack_animations
                        [self.attack_animation_index as usize],
                )
                .set_enabled(true)
                .set_speed(1.3)
                .rewind();

            context.is_attacking = true;
        }

        if self.attack_timeout < 0.0 && attack_animation_ended {
            self.attack_timeout = 0.3;
        }
        self.attack_timeout -= context.dt;

        context.attack_animation_index = self.attack_animation_index as usize;

        // Apply damage to target from melee attack
        if let Some(target) = context.target.as_ref() {
            while let Some(event) = context
                .scene
                .animations
                .get_mut(current_attack_animation)
                .pop_event()
            {
                if event.signal_id == UpperBodyMachine::HIT_SIGNAL
                    && context.upper_body_machine.machine.active_state()
                        == context.upper_body_machine.attack_state
                    && !can_shoot(context.upper_body_machine, context.definition)
                {
                    if let Some(character) =
                        try_get_character_mut(target.handle, &mut context.scene.graph)
                    {
                        character.push_command(CharacterCommand::Damage {
                            who: Default::default(),
                            hitbox: None,
                            /// TODO: Find hit box maybe?
                            amount: context.definition.attack_animations
                                [self.attack_animation_index as usize]
                                .damage
                                .amount(),
                            critical_shot_probability: 0.0,
                        });
                    }

                    if let Some(attack_sound) = context
                        .definition
                        .attack_sounds
                        .iter()
                        .choose(&mut fyrox::rand::thread_rng())
                    {
                        let position = context.character.position(&context.scene.graph);
                        context.sound_manager.play_sound(
                            &mut context.scene.graph,
                            attack_sound,
                            position,
                            1.0,
                            1.0,
                            1.0,
                        );
                    }
                }
            }
            Status::Success
        } else {
            Status::Failure
        }
    }
}

#[derive(Default, Debug, PartialEq, Visit, Eq, Clone)]
pub struct CanMeleeAttack;

impl<'a> Behavior<'a> for CanMeleeAttack {
    type Context = BehaviorContext<'a>;

    fn tick(&mut self, context: &mut Self::Context) -> Status {
        match context.target {
            None => Status::Failure,
            Some(_) => {
                if context.restoration_time <= 0.0 {
                    Status::Success
                } else {
                    Status::Failure
                }
            }
        }
    }
}
