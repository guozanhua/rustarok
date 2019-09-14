use nalgebra::{Isometry2, Vector2};
use specs::{Entities, LazyUpdate};

use crate::components::char::CharacterStateComponent;
use crate::components::controller::{CharEntityId, WorldCoords};
use crate::components::skills::skill::{SkillDef, SkillManifestation, SkillTargetType};
use crate::components::status::status::{
    ApplyStatusComponent, ApplyStatusComponentPayload, ApplyStatusInAreaComponent, Status,
    StatusNature, StatusUpdateResult,
};
use crate::components::{AreaAttackComponent, AttackType, DamageDisplayType, StrEffectComponent};
use crate::effect::StrEffectType;
use crate::runtime_assets::map::PhysicEngine;
use crate::systems::render::render_command::RenderCommandCollectorComponent;
use crate::systems::render_sys::RenderDesktopClientSystem;
use crate::systems::SystemVariables;
use crate::ElapsedTime;

pub struct FireBombSkill;

pub const FIRE_BOMB_SKILL: &'static FireBombSkill = &FireBombSkill;

impl SkillDef for FireBombSkill {
    fn get_icon_path(&self) -> &'static str {
        "data\\texture\\À¯ÀúÀÎÅÍÆäÀÌ½º\\item\\gn_makebomb.bmp"
    }

    fn finish_cast(
        &self,
        caster_entity_id: CharEntityId,
        caster: &CharacterStateComponent,
        skill_pos: Option<Vector2<f32>>,
        char_to_skill_dir: &Vector2<f32>,
        target_entity: Option<CharEntityId>,
        physics_world: &mut PhysicEngine,
        system_vars: &mut SystemVariables,
        entities: &Entities,
        updater: &mut specs::Write<LazyUpdate>,
    ) -> Option<Box<dyn SkillManifestation>> {
        system_vars
            .apply_statuses
            .push(ApplyStatusComponent::from_secondary_status(
                caster_entity_id,
                target_entity.unwrap(),
                Box::new(FireBombStatus {
                    caster_entity_id,
                    started: system_vars.time,
                    until: system_vars.time.add_seconds(2.0),
                    damage: system_vars.dev_configs.skills.firebomb.damage,
                }),
            ));
        None
    }

    fn get_skill_target_type(&self) -> SkillTargetType {
        SkillTargetType::OnlyEnemy
    }
}

#[derive(Clone)]
pub struct FireBombStatus {
    pub caster_entity_id: CharEntityId,
    pub damage: u32,
    pub started: ElapsedTime,
    pub until: ElapsedTime,
}

impl Status for FireBombStatus {
    fn dupl(&self) -> Box<dyn Status> {
        Box::new(self.clone())
    }

    fn update(
        &mut self,
        self_char_id: CharEntityId,
        char_pos: &WorldCoords,
        system_vars: &mut SystemVariables,
        entities: &specs::Entities,
        updater: &mut specs::Write<LazyUpdate>,
    ) -> StatusUpdateResult {
        if self.until.has_already_passed(system_vars.time) {
            let area_shape = Box::new(ncollide2d::shape::Ball::new(2.0));
            let area_isom = Isometry2::new(*char_pos, 0.0);
            system_vars.area_attacks.push(AreaAttackComponent {
                area_shape: area_shape.clone(),
                area_isom: area_isom.clone(),
                source_entity_id: self.caster_entity_id,
                typ: AttackType::SpellDamage(self.damage, DamageDisplayType::Combo(10)),
            });
            system_vars
                .apply_area_statuses
                .push(ApplyStatusInAreaComponent {
                    source_entity_id: self.caster_entity_id,
                    status: ApplyStatusComponentPayload::from_secondary(Box::new(FireBombStatus {
                        caster_entity_id: self.caster_entity_id,
                        started: system_vars.time,
                        until: system_vars.time.add_seconds(2.0),
                        damage: self.damage,
                    })),
                    area_shape: area_shape.clone(),
                    area_isom: area_isom.clone(),
                    except: Some(self_char_id),
                });
            let effect_comp = StrEffectComponent {
                effect_id: StrEffectType::FirePillarBomb.into(),
                pos: *char_pos,
                start_time: system_vars.time.add_seconds(-0.5),
                die_at: system_vars.time.add_seconds(1.0),
            };
            updater.insert(entities.create(), effect_comp);

            StatusUpdateResult::RemoveIt
        } else {
            StatusUpdateResult::KeepIt
        }
    }

    fn render(
        &self,
        char_pos: &WorldCoords,
        system_vars: &SystemVariables,
        render_commands: &mut RenderCommandCollectorComponent,
    ) {
        RenderDesktopClientSystem::render_str(
            StrEffectType::FireWall,
            self.started,
            char_pos,
            system_vars,
            render_commands,
        );
    }

    fn get_status_completion_percent(&self, now: ElapsedTime) -> Option<(ElapsedTime, f32)> {
        Some((self.until, now.percentage_between(self.started, self.until)))
    }

    fn typ(&self) -> StatusNature {
        StatusNature::Harmful
    }
}
