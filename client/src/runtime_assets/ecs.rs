use crate::audio::sound_sys::AudioCommandCollectorComponent;
use crate::components::char::{
    CharacterStateComponent, NpcComponent, SpriteRenderDescriptorComponent, TurretComponent,
    TurretControllerComponent,
};
use crate::components::controller::{
    CameraComponent, HumanInputComponent, LocalPlayerControllerComponent,
};
use crate::components::skills::skills::SkillManifestationComponent;
use crate::components::{
    FlyingNumberComponent, MinionComponent, SoundEffectComponent, StrEffectComponent,
};
use crate::render::render_command::RenderCommandCollector;
use crate::systems::console_system::ConsoleComponent;
use crate::systems::falcon_ai_sys::FalconComponent;
use rustarok_common::components::char::AuthorizedCharStateComponent;
use rustarok_common::components::controller::ControllerComponent;
use specs::World;

pub fn create_ecs_world() -> World {
    let mut ecs_world = specs::World::new();
    ecs_world.register::<NpcComponent>();
    ecs_world.register::<TurretComponent>();
    ecs_world.register::<TurretControllerComponent>();
    ecs_world.register::<FalconComponent>();
    ecs_world.register::<HumanInputComponent>();
    ecs_world.register::<RenderCommandCollector>();
    ecs_world.register::<AudioCommandCollectorComponent>();
    ecs_world.register::<SpriteRenderDescriptorComponent>();
    ecs_world.register::<CharacterStateComponent>();
    ecs_world.register::<FlyingNumberComponent>();
    ecs_world.register::<SoundEffectComponent>();
    ecs_world.register::<StrEffectComponent>();
    ecs_world.register::<SkillManifestationComponent>();
    ecs_world.register::<CameraComponent>();
    ecs_world.register::<LocalPlayerControllerComponent>();
    ecs_world.register::<MinionComponent>();
    ecs_world.register::<ConsoleComponent>();

    ecs_world.register::<AuthorizedCharStateComponent>();
    ecs_world.register::<ControllerComponent>();
    ecs_world
}
