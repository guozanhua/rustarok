use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nalgebra::Vector2;
use nphysics2d::object::DefaultColliderHandle;
use serde::Deserialize;
use specs::prelude::*;
use strum_macros::EnumIter;

use crate::common::v2_to_v3;
use crate::components::char::{CastingSkillData, CharacterStateComponent};
use crate::components::controller::{CharEntityId, WorldCoords};
use crate::components::skills::absorb_shield::ABSORB_SHIELD_SKILL;
use crate::components::skills::brutal_test_skill::BRUTAL_TEST_SKILL;
use crate::components::skills::cure::CURE_SKILL;
use crate::components::skills::fire_bomb::FIRE_BOMB_SKILL;
use crate::components::skills::firewall::FIRE_WALL_SKILL;
use crate::components::skills::heal::HEAL_SKILL;
use crate::components::skills::lightning::LIGHTNING_SKILL;
use crate::components::skills::mounting::MOUNTING_SKILL;
use crate::components::skills::poison::POISON_SKILL;
use crate::components::skills::wiz_pyroblast::WIZ_PYRO_BLAST_SKILL;

use crate::components::skills::assa_blade_dash::ASSA_BLADE_DASH_SKILL;
use crate::configs::DevConfig;
use crate::effect::StrEffectType;
use crate::systems::render::render_command::RenderCommandCollector;
use crate::systems::render_sys::RenderDesktopClientSystem;
use crate::systems::sound_sys::AudioCommandCollectorComponent;
use crate::systems::{AssetResources, Collision, SystemVariables};
use crate::{ElapsedTime, PhysicEngine};

pub type WorldCollisions = HashMap<(DefaultColliderHandle, DefaultColliderHandle), Collision>;

pub trait SkillManifestation {
    fn update(
        &mut self,
        entity_id: Entity,
        all_collisions_in_world: &WorldCollisions,
        system_vars: &mut SystemVariables,
        entities: &specs::Entities,
        char_storage: &mut specs::WriteStorage<CharacterStateComponent>,
        physics_world: &mut PhysicEngine,
        updater: &mut specs::Write<LazyUpdate>,
    );

    fn render(
        &self,
        now: ElapsedTime,
        tick: u64,
        assets: &AssetResources,
        configs: &DevConfig,
        render_commands: &mut RenderCommandCollector,
        audio_command_collector: &mut AudioCommandCollectorComponent,
    );
}

#[storage(HashMapStorage)]
#[derive(Component)]
pub struct SkillManifestationComponent {
    pub self_entity_id: Entity,
    pub skill: Arc<Mutex<Box<dyn SkillManifestation>>>,
}

impl SkillManifestationComponent {
    pub fn new(
        self_entity_id: Entity,
        skill: Box<dyn SkillManifestation>,
    ) -> SkillManifestationComponent {
        SkillManifestationComponent {
            self_entity_id,
            skill: Arc::new(Mutex::new(skill)),
        }
    }

    pub fn update(
        &mut self,
        self_entity_id: Entity,
        all_collisions_in_world: &WorldCollisions,
        system_vars: &mut SystemVariables,
        entities: &specs::Entities,
        char_storage: &mut specs::WriteStorage<CharacterStateComponent>,
        physics_world: &mut PhysicEngine,
        updater: &mut specs::Write<LazyUpdate>,
    ) {
        let mut skill = self.skill.lock().unwrap();
        skill.update(
            self_entity_id,
            all_collisions_in_world,
            system_vars,
            entities,
            char_storage,
            physics_world,
            updater,
        );
    }

    pub fn render(
        &self,
        now: ElapsedTime,
        tick: u64,
        assets: &AssetResources,
        configs: &DevConfig,
        render_commands: &mut RenderCommandCollector,
        audio_commands: &mut AudioCommandCollectorComponent,
    ) {
        let skill = self.skill.lock().unwrap();
        skill.render(now, tick, assets, configs, render_commands, audio_commands);
    }
}

unsafe impl Sync for SkillManifestationComponent {}

unsafe impl Send for SkillManifestationComponent {}

pub trait SkillDef {
    fn get_icon_path(&self) -> &'static str;
    fn finish_cast(
        &self,
        caster_entity_id: CharEntityId,
        caster: &CharacterStateComponent,
        skill_pos: Option<Vector2<f32>>,
        char_to_skill_dir: &Vector2<f32>,
        target_entity: Option<CharEntityId>,
        physics_world: &mut PhysicEngine,
        system_vars: &mut SystemVariables,
        entities: &specs::Entities,
        updater: &mut specs::Write<LazyUpdate>,
    ) -> Option<Box<dyn SkillManifestation>>;

    fn get_skill_target_type(&self) -> SkillTargetType;
    fn render_casting(
        &self,
        char_pos: &Vector2<f32>,
        casting_state: &CastingSkillData,
        system_vars: &SystemVariables,
        render_commands: &mut RenderCommandCollector,
        char_storage: &ReadStorage<CharacterStateComponent>,
    ) {
        RenderDesktopClientSystem::render_str(
            StrEffectType::Moonstar,
            casting_state.cast_started,
            char_pos,
            system_vars,
            render_commands,
        );
        if let Some(target_area_pos) = casting_state.target_area_pos {
            self.render_target_selection(
                true,
                &target_area_pos,
                &casting_state.char_to_skill_dir_when_casted,
                render_commands,
                &system_vars.dev_configs,
            );
        } else if let Some(target_entity) = casting_state.target_entity {
            if let Some(target_char) = char_storage.get(target_entity.0) {
                render_commands
                    .horizontal_texture_3d()
                    .rotation_rad(system_vars.time.0 % 6.28)
                    .pos(&target_char.pos())
                    .add(&system_vars.assets.sprites.magic_target)
            }
        }
    }
    fn render_target_selection(
        &self,
        is_castable: bool,
        skill_pos: &Vector2<f32>,
        char_to_skill_dir: &Vector2<f32>,
        render_commands: &mut RenderCommandCollector,
        configs: &DevConfig,
    ) {
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash, EnumIter)]
pub enum Skills {
    FireWall,
    BrutalTestSkill,
    Lightning,
    Heal,
    Mounting,
    Poison,
    Cure,
    FireBomb,
    AbsorbShield,
    WizPyroBlast,
    AssaBladeDash,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SkillCastingAttributes {
    pub casting_time: ElapsedTime,
    pub cast_delay: ElapsedTime,
    pub casting_range: f32,
    // in case of Directional skills
    pub width: Option<f32>,
}

impl Skills {
    pub fn get_definition(&self) -> &'static dyn SkillDef {
        match self {
            Skills::WizPyroBlast => WIZ_PYRO_BLAST_SKILL,
            Skills::FireWall => FIRE_WALL_SKILL,
            Skills::Heal => HEAL_SKILL,
            Skills::BrutalTestSkill => BRUTAL_TEST_SKILL,
            Skills::Lightning => LIGHTNING_SKILL,
            Skills::Mounting => MOUNTING_SKILL,
            Skills::Poison => POISON_SKILL,
            Skills::Cure => CURE_SKILL,
            Skills::FireBomb => FIRE_BOMB_SKILL,
            Skills::AbsorbShield => ABSORB_SHIELD_SKILL,
            Skills::AssaBladeDash => ASSA_BLADE_DASH_SKILL,
        }
    }

    pub fn get_cast_attributes<'a>(
        &'a self,
        configs: &'a DevConfig,
        char_state: &CharacterStateComponent,
    ) -> &'a SkillCastingAttributes {
        match self {
            Skills::WizPyroBlast => &configs.skills.wiz_pyroblast.attributes,
            Skills::FireWall => &configs.skills.firewall.attributes,
            Skills::Heal => &configs.skills.heal.attributes,
            Skills::BrutalTestSkill => &configs.skills.brutal_test_skill.attributes,
            Skills::Lightning => &configs.skills.lightning.attributes,
            Skills::Mounting => {
                if char_state.statuses.is_mounted() {
                    &configs.skills.unmounting
                } else {
                    &configs.skills.mounting
                }
            }
            Skills::Poison => &configs.skills.poison.attributes,
            Skills::Cure => &configs.skills.cure,
            Skills::FireBomb => &configs.skills.firebomb.attributes,
            Skills::AbsorbShield => &configs.skills.absorb_shield.attributes,
            Skills::AssaBladeDash => &configs.skills.assa_blade_dash.attributes,
        }
    }

    fn get_icon_path(&self) -> &'static str {
        self.get_definition().get_icon_path()
    }

    fn get_skill_target_type(&self) -> SkillTargetType {
        self.get_definition().get_skill_target_type()
    }

    pub fn limit_vector_into_range(
        char_pos: &Vector2<f32>,
        mouse_pos: &WorldCoords,
        range: f32,
    ) -> (Vector2<f32>, Vector2<f32>) {
        let dir2d = mouse_pos - char_pos;
        let dir_vector = dir2d.normalize();
        let pos = char_pos + dir_vector * dir2d.magnitude().min(range);
        return (pos, dir_vector);
    }

    pub fn render_casting_box(
        is_castable: bool,
        casting_area_size: &Vector2<u16>,
        skill_pos: &Vector2<f32>,
        char_to_skill_dir: &Vector2<f32>,
        render_commands: &mut RenderCommandCollector,
    ) {
        let angle = char_to_skill_dir.angle(&Vector2::y());
        let angle = if char_to_skill_dir.x > 0.0 {
            angle
        } else {
            -angle
        };
        let skill_pos = v2_to_v3(skill_pos);

        render_commands
            .rectangle_3d()
            .pos(&skill_pos)
            .rotation_rad(angle)
            .color(
                &(if is_castable {
                    [0, 255, 0, 255]
                } else {
                    [179, 179, 179, 255]
                }),
            )
            .size(casting_area_size.x, casting_area_size.y)
            .add()
    }

    pub fn is_casting_allowed_based_on_target(
        skill_target_type: SkillTargetType,
        skill_casting_range: f32,
        caster_id: CharEntityId,
        target_entity: Option<CharEntityId>,
        target_distance: f32,
    ) -> bool {
        match skill_target_type {
            SkillTargetType::Area => true,
            SkillTargetType::Directional => true,
            SkillTargetType::NoTarget => true,
            SkillTargetType::AnyEntity => {
                target_entity.is_some() && skill_casting_range >= target_distance
            }
            SkillTargetType::OnlyAllyButNoSelf => {
                target_entity.map(|it| it != caster_id).unwrap_or(false)
                    && skill_casting_range >= target_distance
            }
            SkillTargetType::OnlyAllyAndSelf => {
                target_entity.is_some() && skill_casting_range >= target_distance
            }
            SkillTargetType::OnlyEnemy => {
                target_entity.is_some() && skill_casting_range >= target_distance
            }
            SkillTargetType::OnlySelf => target_entity.map(|it| it == caster_id).unwrap_or(false),
        }
    }
}

#[derive(Eq, PartialEq)]
pub enum SkillTargetType {
    /// casts immediately
    NoTarget,
    Area,
    Directional,
    AnyEntity,
    OnlyAllyButNoSelf,
    OnlyAllyAndSelf,
    OnlyEnemy,
    OnlySelf,
}
