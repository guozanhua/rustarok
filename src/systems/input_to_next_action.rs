use crate::components::char::{CharacterStateComponent, Team};
use crate::components::controller::{
    CastMode, ControllerComponent, HumanInputComponent, PlayerIntention, SkillKey,
};
use crate::components::skills::skill::SkillTargetType;
use crate::cursor::{CursorFrame, CURSOR_CLICK, CURSOR_NORMAL, CURSOR_STOP, CURSOR_TARGET};
use crate::systems::input_sys::InputConsumerSystem;
use crate::systems::{SystemFrameDurations, SystemVariables};
use crate::ElapsedTime;
use sdl2::keyboard::Scancode;
use specs::prelude::*;
use strum::IntoEnumIterator;

pub struct InputToNextActionSystem;

impl<'a> specs::System<'a> for InputToNextActionSystem {
    type SystemData = (
        specs::Entities<'a>,
        specs::WriteStorage<'a, HumanInputComponent>,
        specs::ReadStorage<'a, CharacterStateComponent>,
        specs::WriteStorage<'a, ControllerComponent>,
        specs::WriteExpect<'a, SystemFrameDurations>,
        specs::ReadExpect<'a, SystemVariables>,
    );

    fn run(
        &mut self,
        (
            entities,
            mut input_storage,
            char_state_storage,
            mut controller_storage,
            mut system_benchmark,
            system_vars,
        ): Self::SystemData,
    ) {
        let _stopwatch = system_benchmark.start_measurement("InputToNextActionSystem");
        for (self_id, self_char_comp, input, controller) in (
            &entities,
            &char_state_storage,
            &mut input_storage,
            &mut controller_storage,
        )
            .join()
        {
            let controller: &mut ControllerComponent = controller;
            let input: &mut HumanInputComponent = input;
            let just_pressed_skill_key = SkillKey::iter()
                .filter(|key| input.is_key_just_pressed(key.scancode()))
                .take(1)
                .collect::<Vec<SkillKey>>()
                .pop();
            let just_released_skill_key = SkillKey::iter()
                .filter(|key| input.is_key_just_released(key.scancode()))
                .take(1)
                .collect::<Vec<SkillKey>>()
                .pop();

            controller.calc_entities_below_cursor(
                self_char_comp.team,
                input.last_mouse_x,
                input.last_mouse_y,
            );

            controller.cell_below_cursor_walkable = system_vars.map_render_data.gat.is_walkable(
                input.mouse_world_pos.x.max(0.0) as usize,
                input.mouse_world_pos.y.abs() as usize,
            );
            let (cursor_frame, cursor_color) = InputToNextActionSystem::determine_cursor(
                system_vars.time,
                self_id,
                input,
                controller,
                &char_state_storage,
                self_char_comp.team,
            );
            controller.cursor_anim_descr.action_index = cursor_frame.1;
            controller.cursor_color = cursor_color;

            if controller.next_action.is_some() {
                // here 'next_action' is the action from the prev frame
                controller.last_action = std::mem::replace(&mut controller.next_action, None);
            }
            let alt_down = input.is_key_down(Scancode::LAlt);
            controller.next_action = if let Some((casting_skill_key, skill)) =
                input.select_skill_target
            {
                match input.cast_mode {
                    CastMode::Normal => {
                        if input.left_mouse_released {
                            log::debug!("Player wants to cast {:?}", skill);
                            input.select_skill_target = None;
                            Some(PlayerIntention::Casting(
                                skill,
                                false,
                                input.mouse_world_pos,
                            ))
                        } else if input.right_mouse_pressed {
                            input.select_skill_target = None;
                            None
                        } else if let Some((skill_key, skill)) =
                            just_pressed_skill_key.and_then(|skill_key| {
                                input
                                    .get_skill_for_key(skill_key)
                                    .map(|skill| (skill_key, skill))
                            })
                        {
                            log::debug!("Player select target for casting {:?}", skill);
                            let shhh = InputConsumerSystem::target_selection_or_casting(
                                skill,
                                input.mouse_world_pos,
                            );
                            if let Some(s) = shhh {
                                Some(s)
                            } else {
                                input.select_skill_target = Some((skill_key, skill));
                                None
                            }
                        } else {
                            None
                        }
                    }
                    CastMode::OnKeyRelease => {
                        if input.is_key_just_released(casting_skill_key.scancode()) {
                            log::debug!("Player wants to cast {:?}", skill);
                            input.select_skill_target = None;
                            Some(
                                PlayerIntention::Casting(
                                    input.get_skill_for_key(casting_skill_key)
                                        .expect("'is_casting_selection' must be Some only if the casting skill is valid! "),
                                    false,
                                    input.mouse_world_pos,
                                )
                            )
                        } else if input.right_mouse_pressed {
                            input.select_skill_target = None;
                            None
                        } else {
                            None
                        }
                    }
                    CastMode::OnKeyPress => {
                        // not possible to get into this state while OnKeyPress is active
                        None
                    }
                }
            } else if let Some((skill_key, skill)) = just_pressed_skill_key.and_then(|skill_key| {
                input
                    .get_skill_for_key(skill_key)
                    .map(|skill| (skill_key, skill))
            }) {
                match input.cast_mode {
                    CastMode::Normal | CastMode::OnKeyRelease => {
                        if !alt_down {
                            log::debug!(
                                "Player select target for casting {:?} (just_pressed_skill_key)",
                                skill
                            );
                            let shh = InputConsumerSystem::target_selection_or_casting(
                                skill,
                                input.mouse_world_pos,
                            );
                            if let Some(s) = shh {
                                Some(s)
                            } else {
                                input.select_skill_target = Some((skill_key, skill));
                                None
                            }
                        } else {
                            None
                        }
                    }
                    CastMode::OnKeyPress => {
                        log::debug!("Player wants to cast {:?}, alt={:?}", skill, alt_down);
                        input.select_skill_target = None;
                        Some(PlayerIntention::Casting(
                            skill,
                            alt_down,
                            input.mouse_world_pos,
                        ))
                    }
                }
            } else if let Some((_skill_key, skill)) =
                just_released_skill_key.and_then(|skill_key| {
                    input
                        .get_skill_for_key(skill_key)
                        .map(|skill| (skill_key, skill))
                })
            {
                // can get here only when alt was down and OnKeyRelease
                if alt_down {
                    log::debug!("Player wants to cast {:?}, SELF", skill);
                    Some(PlayerIntention::Casting(skill, true, input.mouse_world_pos))
                } else {
                    None
                }
            } else if input.right_mouse_pressed {
                Some(PlayerIntention::MoveTowardsMouse(input.mouse_world_pos))
            } else if input.right_mouse_down {
                Some(PlayerIntention::MoveTowardsMouse(input.mouse_world_pos))
            } else if let Some(PlayerIntention::MoveTowardsMouse(pos)) = &controller.last_action {
                // user released the mouse, so it is not a MoveTowardsMouse but a move/attack to command
                if let Some(target_entity_id) = controller.entities_below_cursor.get_enemy() {
                    if char_state_storage
                        .get(target_entity_id)
                        .map(|it| !it.state().is_dead())
                        .unwrap_or(false)
                    {
                        Some(PlayerIntention::Attack(target_entity_id))
                    } else {
                        Some(PlayerIntention::MoveTo(input.mouse_world_pos))
                    }
                } else {
                    Some(PlayerIntention::MoveTo((*pos).clone()))
                }
            } else if let Some(PlayerIntention::Casting(..)) = &controller.last_action {
                // Casting might have been rejected because for example the char was attacking at the time, but
                // we want to cast it as soon as the rejection reason ceases AND there is no other intention
                if !controller.next_action_allowed {
                    controller.last_action.clone()
                } else {
                    None
                }
            } else {
                None
            };
        }
    }
}
impl InputToNextActionSystem {
    pub fn determine_cursor(
        now: ElapsedTime,
        self_id: Entity,
        input: &HumanInputComponent,
        controller: &ControllerComponent,
        char_state_storage: &ReadStorage<CharacterStateComponent>,
        self_team: Team,
    ) -> (CursorFrame, [f32; 3]) {
        // TODO: move select_skill_target into ControllerComponent?
        return if let Some((_skill_key, skill)) = input.select_skill_target {
            let is_castable = char_state_storage
                .get(self_id)
                .unwrap()
                .skill_cast_allowed_at
                .get(&skill)
                .unwrap_or(&ElapsedTime(0.0))
                .is_earlier_than(now);
            if !is_castable {
                (CURSOR_STOP, [1.0, 1.0, 1.0])
            } else if skill.get_skill_target_type() != SkillTargetType::Area {
                (CURSOR_TARGET, [1.0, 1.0, 1.0])
            } else {
                (CURSOR_CLICK, [1.0, 1.0, 1.0])
            }
        } else if let Some(entity_below_cursor) =
            controller.entities_below_cursor.get_enemy_or_friend()
        {
            let ent_is_dead_or_friend = char_state_storage
                .get(entity_below_cursor)
                .map(|it| !it.state().is_alive() || it.team == self_team)
                .unwrap_or(false);
            if entity_below_cursor == self_id || ent_is_dead_or_friend {
                // self or dead
                (CURSOR_NORMAL, [0.2, 0.46, 0.9])
            } else {
                (CURSOR_NORMAL, [1.0, 0.0, 0.0])
            }
        } else if !controller.cell_below_cursor_walkable {
            (CURSOR_STOP, [1.0, 1.0, 1.0])
        } else {
            (CURSOR_NORMAL, [1.0, 1.0, 1.0])
        };
    }
}