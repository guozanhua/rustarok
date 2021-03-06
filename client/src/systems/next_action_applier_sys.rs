use crate::components::char::{
    get_sprite_and_action_index, CastingSkillData, CharacterStateComponent, ClientCharState,
    SpriteRenderDescriptorComponent,
};
use crate::components::controller::{EntitiesBelowCursor, LocalPlayerControllerComponent};
use crate::components::skills::skills::{SkillTargetType, Skills};
use crate::configs::DevConfig;
use crate::systems::{SystemEvent, SystemFrameDurations, SystemVariables};
use crate::ElapsedTime;
use rustarok_common::common::{EngineTime, Vec2};
use rustarok_common::components::char::{
    AuthorizedCharStateComponent, CharEntityId, CharState, EntityTarget,
};
use rustarok_common::components::controller::PlayerIntention;
use specs::prelude::*;

// TODO2
// ezt itthagyom, mert az eredeti kód beállítja a controller repeat_next_action fieldjét
// amit a szerver nem tes meg ugyebár, mert ez cska kliensoldali dolog.

//pub struct NextActionApplierSystem;
//
//impl<'a> System<'a> for NextActionApplierSystem {
//    type SystemData = (
//        WriteStorage<'a, CharacterStateComponent>,
//        WriteStorage<'a, ControllerComponent>,
//        ReadExpect<'a, SystemVariables>,
//        ReadExpect<'a, DevConfig>,
//        WriteExpect<'a, SystemFrameDurations>,
//    );
//
//    fn run(
//        &mut self,
//        (
//            mut char_state_storage,
//            mut controller_storage,
//            sys_vars,
//            dev_configs,
//            mut system_benchmark,
//        ): Self::SystemData,
//    ) {
//        let _stopwatch = system_benchmark.start_measurement("NextActionApplierSystem");
//        let now = time.now();
//        for controller in (&mut controller_storage).join() {
//            let char_state = char_state_storage.get_mut(controller.controlled_entity.into());
//
//            // the controlled character might have been removed due to death etc
//            if let Some(char_state) = char_state {
//                if char_state.statuses.can_be_controlled() == false {
//                    continue;
//                }
//                controller.repeat_next_action = match controller.next_action {
//                    Some(PlayerIntention::MoveTo(pos)) => {
//                        char_state.target = Some(EntityTarget::Pos(pos));
//                        false
//                    }
//                    Some(PlayerIntention::Attack(target_entity_id)) => {
//                        char_state.target = Some(EntityTarget::OtherEntity(target_entity_id));
//                        false
//                    }
//                    Some(PlayerIntention::MoveTowardsMouse(pos)) => {
//                        char_state.target = Some(EntityTarget::Pos(pos));
//                        false
//                    }
//                    Some(PlayerIntention::AttackTowards(pos)) => {
//                        char_state.target = Some(EntityTarget::PosWhileAttacking(pos, None));
//                        false
//                    }
//                    // TODO2
//                    //                    Some(PlayerIntention::Casting(skill, is_self_cast, mouse_world_pos)) => {
//                    //                        NextActionApplierSystem::try_cast_skill(
//                    //                            skill,
//                    //                            now,
//                    //                            &dev_configs,
//                    //                            char_state,
//                    //                            &mouse_world_pos,
//                    //                            &controller.entities_below_cursor,
//                    //                            controller.controlled_entity,
//                    //                            is_self_cast,
//                    //                        )
//                    //                    }
//                    None => false,
//                };
//            }
//        }
//    }
//}

pub struct UpdateCharSpriteBasedOnStateSystem;

impl<'a> System<'a> for UpdateCharSpriteBasedOnStateSystem {
    type SystemData = (
        ReadStorage<'a, CharacterStateComponent>,
        WriteStorage<'a, SpriteRenderDescriptorComponent>,
        ReadStorage<'a, AuthorizedCharStateComponent>,
        ReadExpect<'a, SystemVariables>,
        ReadExpect<'a, EngineTime>,
    );

    fn run(
        &mut self,
        (char_state_storage, mut sprite_storage, auth_state_storage, sys_vars, time): Self::SystemData,
    ) {
        // update character's sprite based on its state
        let now = time.now();
        for (char_comp, auth_state, sprite) in (
            &char_state_storage,
            &auth_state_storage,
            &mut sprite_storage,
        )
            .join()
        {
            // e.g. don't switch to IDLE immediately when prev state is ReceivingDamage.
            // let ReceivingDamage animation play till to the end
            let state: CharState = auth_state.state().clone();
            let prev_state: CharState = char_comp.prev_state().clone();
            let prev_animation_has_ended = sprite.animation_ends_at.has_already_passed(now);
            let prev_animation_must_stop_at_end = match char_comp.prev_state() {
                CharState::Walking(_) => true,
                _ => false,
            };
            let state_has_changed = char_comp.state_type_has_changed(auth_state.state());
            if (state_has_changed && state != CharState::Idle)
                || (state == CharState::Idle && prev_animation_has_ended)
                || (state == CharState::Idle && prev_animation_must_stop_at_end)
            {
                sprite.animation_started = now;
                let forced_duration = match &state {
                    // TODO2
                    //                    CharState::Attacking { .. } => Some(char_comp.attack_delay_ends_at.minus(now)),
                    // HACK: '100.0', so the first frame is rendered during casting :)
                    //                    CharState::CastingSkill(casting_info) => {
                    //                        Some(casting_info.cast_ends.add_seconds(100.0))
                    //                    }
                    _ => None,
                };
                sprite.forced_duration = forced_duration;
                sprite.fps_multiplier = if state.is_walking() {
                    char_comp.calculated_attribs().movement_speed.as_f32()
                } else {
                    1.0
                };
                let (sprite_res, action_index) = get_sprite_and_action_index(
                    &char_comp.outlook,
                    &sys_vars.assets.sprites,
                    &state,
                );
                sprite.action_index = action_index;
                sprite.animation_ends_at = now.add(forced_duration.unwrap_or_else(|| {
                    let duration = sprite_res.action.actions[action_index].duration;
                    ElapsedTime(duration)
                }));
            } else if char_comp.went_from_casting_to_idle(auth_state.state()) {
                // During casting, only the first frame is rendered
                // when casting is finished, we let the animation runs till the end
                sprite.animation_started = now.add_seconds(-0.1);
                sprite.forced_duration = None;
                let (sprite_res, action_index) = get_sprite_and_action_index(
                    &char_comp.outlook,
                    &sys_vars.assets.sprites,
                    &prev_state,
                );
                let duration = sprite_res.action.actions[action_index].duration;
                sprite.animation_ends_at = sprite.animation_started.add_seconds(duration);
            }
            sprite.direction = auth_state.dir();
        }
    }
}

pub struct SavePreviousCharStateSystem;

impl<'a> System<'a> for SavePreviousCharStateSystem {
    type SystemData = (
        Entities<'a>,
        WriteStorage<'a, CharacterStateComponent>,
        ReadStorage<'a, AuthorizedCharStateComponent>,
        ReadExpect<'a, EngineTime>,
        Option<Write<'a, Vec<SystemEvent>>>,
    );

    fn run(
        &mut self,
        (entities, mut char_state_storage, auth_char_state_storage, time, mut events): Self::SystemData,
    ) {
        for (char_id, char_comp, auth_state) in
            (&entities, &mut char_state_storage, &auth_char_state_storage).join()
        {
            // TODO: if debug
            let state_has_changed = char_comp.state_type_has_changed(auth_state.state());
            if state_has_changed {
                let state: CharState = auth_state.state().clone();
                let prev_state: CharState = char_comp.prev_state().clone();
                if let Some(events) = &mut events {
                    events.push(SystemEvent::CharStatusChange(
                        time.tick - 1, // we detected the change here, but it happened in the prev state
                        CharEntityId::new(char_id),
                        prev_state.clone(),
                        state.clone(),
                    ));
                }
                log::debug!(
                    "[{}] {:?} state has changed {:?} ==> {:?}",
                    time.tick,
                    char_id,
                    prev_state,
                    state
                );
            }
            char_comp.save_prev_state(auth_state.state());
        }
    }
}
