extern crate actix_web;
extern crate byteorder;
extern crate encoding;
extern crate gl;
#[macro_use]
extern crate imgui;
extern crate imgui_opengl_renderer;
extern crate imgui_sdl2;
extern crate log;
extern crate nalgebra;
extern crate sdl2;
extern crate specs;
#[macro_use]
extern crate specs_derive;
extern crate config;
extern crate libflate;
extern crate serde;
#[macro_use]
extern crate serde_json;
extern crate crossbeam_channel;
extern crate notify;
extern crate strum;
extern crate strum_macros;
extern crate sublime_fuzzy;
extern crate websocket;

use byteorder::{LittleEndian, WriteBytesExt};
use encoding::types::Encoding;
use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::time::{Duration, Instant, SystemTime};
use strum::IntoEnumIterator;

use imgui::{ImString, ImVec2};
use log::LevelFilter;
use nalgebra::{Matrix4, Point2, Point3, Rotation3, Unit, Vector2, Vector3};
use ncollide2d::shape::ShapeHandle;
use nphysics2d::object::{
    BodyPartHandle, ColliderDesc, DefaultBodySet, DefaultColliderHandle, DefaultColliderSet,
    RigidBodyDesc,
};
use nphysics2d::solver::SignoriniModel;
use rand::Rng;
use specs::prelude::*;
use specs::Builder;
use specs::Join;

use crate::asset::gat::{CellType, Gat};
use crate::asset::gnd::Gnd;
use crate::asset::rsm::{BoundingBox, Rsm};
use crate::asset::rsw::{Rsw, WaterData};
use crate::asset::str::StrFile;
use crate::asset::{AssetLoader, SpriteResource};
use crate::components::char::{
    CharType, CharacterStateComponent, NpcComponent, Percentage, PhysicsComponent,
    SpriteRenderDescriptorComponent, Team,
};
use crate::components::controller::{
    CameraComponent, CastMode, CharEntityId, ControllerComponent, ControllerEntityId,
    HumanInputComponent, SkillKey,
};
use crate::components::{
    AttackType, BrowserClient, FlyingNumberComponent, MinionComponent, SoundEffectComponent,
    StrEffectComponent,
};
use crate::consts::{job_name_table, JobId, MonsterId};
use crate::notify::Watcher;
use crate::systems::atk_calc::AttackSystem;
use crate::systems::char_state_sys::CharacterStateUpdateSystem;
use crate::systems::input_sys::{BrowserInputProducerSystem, InputConsumerSystem};
use crate::systems::next_action_applier_sys::NextActionApplierSystem;
use crate::systems::phys::{FrictionSystem, PhysCollisionCollectorSystem};
use crate::systems::skill_sys::SkillSystem;
use crate::systems::{
    AssetResources, CollisionsFromPrevFrame, EffectSprites, Sex, Sounds, Sprites,
    SystemFrameDurations, SystemVariables, Texts,
};
use crate::video::{
    ortho, DynamicVertexArray, GlTexture, Shader, ShaderProgram, VertexArray,
    VertexAttribDefinition, Video, VIDEO_HEIGHT, VIDEO_WIDTH,
};
use encoding::DecoderTrap;

#[macro_use]
mod common;
mod asset;
mod cam;
mod configs;
mod consts;
mod cursor;
mod shaders;
mod video;
mod web_server;

#[macro_use]
mod components;
mod systems;

use crate::asset::database::AssetDatabase;
use crate::components::skills::fire_bomb::FireBombStatus;
use crate::components::skills::skill::{SkillManifestationComponent, Skills};
use crate::components::status::absorb_shield::AbsorbStatus;
use crate::components::status::attrib_mod::ArmorModifierStatus;
use crate::components::status::heal_area::HealApplierArea;
use crate::components::status::status::{ApplyStatusComponentPayload, MainStatuses};
use crate::components::status::status_applier_area::StatusApplierArea;
use crate::configs::{AppConfig, DevConfig};
use crate::shaders::{
    GroundShaderParameters, ModelShaderParameters, Sprite3dShaderParameters,
    StrEffect3dShaderParameters, Texture2dShaderParameters, Trimesh2dShaderParameters,
    Trimesh3dShaderParameters,
};
use crate::systems::camera_system::CameraSystem;
use crate::systems::console_commands::STATUS_NAMES;
use crate::systems::console_system::{CommandDefinition, ConsoleComponent, ConsoleSystem};
use crate::systems::frame_end_system::FrameEndSystem;
use crate::systems::input_to_next_action::InputToNextActionSystem;
use crate::systems::minion_ai_sys::MinionAiSystem;
use crate::systems::render::opengl_render_sys::OpenGlRenderSystem;
use crate::systems::render::render_command::RenderCommandCollectorComponent;
use crate::systems::render::websocket_browser_render_sys::WebSocketBrowserRenderSystem;
use crate::systems::render_sys::RenderDesktopClientSystem;
use crate::systems::sound_sys::{AudioCommandCollectorComponent, SoundSystem};
use crate::web_server::start_web_server;
use ncollide2d::pipeline::CollisionGroups;
use nphysics2d::force_generator::DefaultForceGeneratorSet;
use nphysics2d::joint::DefaultJointConstraintSet;
use nphysics2d::world::{DefaultGeometricalWorld, DefaultMechanicalWorld};
use std::str::FromStr;
use websocket::OwnedMessage;

// simulations per second
pub const SIMULATION_FREQ: u64 = 30;
pub const MAX_SECONDS_ALLOWED_FOR_SINGLE_FRAME: f32 = (1000 / SIMULATION_FREQ) as f32 / 1000.0;

pub const PLAYABLE_OUTLOOKS: [JobId; 12] = [
    JobId::CRUSADER,
    JobId::SWORDMAN,
    JobId::ARCHER,
    JobId::ASSASSIN,
    JobId::KNIGHT,
    JobId::WIZARD,
    JobId::SAGE,
    JobId::ALCHEMIST,
    JobId::BLACKSMITH,
    JobId::PRIEST,
    JobId::MONK,
    JobId::GUNSLINGER,
];

#[derive(Clone, Copy)]
pub enum CharActionIndex {
    Idle = 0,
    Walking = 8,
    Sitting = 16,
    PickingItem = 24,
    StandBy = 32,
    Attacking1 = 40,
    ReceivingDamage = 48,
    Freeze1 = 56,
    Dead = 65,
    Freeze2 = 72,
    Attacking2 = 80,
    Attacking3 = 88,
    CastingSpell = 96,
}

#[derive(Clone, Copy)]
pub enum MonsterActionIndex {
    Idle = 0,
    Walking = 8,
    Attack = 16,
    ReceivingDamage = 24,
    Die = 32,
}

#[derive(Clone, Copy)]
pub enum CollisionGroup {
    StaticModel,
    Player,
    NonPlayer,
    SkillArea,
}

pub struct Shaders {
    pub ground_shader: ShaderProgram<GroundShaderParameters>,
    pub model_shader: ShaderProgram<ModelShaderParameters>,
    pub sprite_shader: ShaderProgram<Sprite3dShaderParameters>,
    pub str_effect_shader: ShaderProgram<StrEffect3dShaderParameters>,
    pub sprite2d_shader: ShaderProgram<Texture2dShaderParameters>,
    pub trimesh_shader: ShaderProgram<Trimesh3dShaderParameters>,
    pub trimesh2d_shader: ShaderProgram<Trimesh2dShaderParameters>,
}

//áttetsző modellek
//  csak a camera felé néző falak rajzolódjanak ilyenkor ki
//  a modelleket z sorrendben növekvőleg rajzold ki
//jobIDt tartalmazzon ne indexet a sprite
// guild_vs4.rsw
// implement attack range check with proximity events
//3xos gyorsitás = 1 frame alatt 3x annyi minden történik (3 physics etc
// tick helyett idő mértékgeységgel számolj
// legyen egy központi abstract renderer, és neki külkdjenek a rendszerek
//  render commandokat, ő pedig hatékonyan csoportositva rajzolja ki azokat

pub struct RenderMatrices {
    pub projection: Matrix4<f32>,
    pub ortho: Matrix4<f32>,
}

#[derive(Copy, Clone, Debug)]
pub struct DeltaTime(pub f32);

#[derive(Debug, Copy, Clone)]
pub struct ElapsedTime(f32);

impl PartialEq for ElapsedTime {
    fn eq(&self, other: &Self) -> bool {
        (self.0 * 1000.0) as u32 == (other.0 * 1000.0) as u32
    }
}

impl Eq for ElapsedTime {}

impl ElapsedTime {
    pub fn add_seconds(&self, seconds: f32) -> ElapsedTime {
        ElapsedTime(self.0 + seconds as f32)
    }

    pub fn minus(&self, other: ElapsedTime) -> ElapsedTime {
        ElapsedTime(self.0 - other.0)
    }

    pub fn percentage_between(&self, from: ElapsedTime, to: ElapsedTime) -> f32 {
        let current = self.0 - from.0;
        let range = to.0 - from.0;
        return current / range;
    }

    pub fn add(&self, other: ElapsedTime) -> ElapsedTime {
        ElapsedTime(self.0 + other.0)
    }

    pub fn elapsed_since(&self, other: ElapsedTime) -> ElapsedTime {
        ElapsedTime(self.0 - other.0)
    }

    pub fn div(&self, other: f32) -> f32 {
        self.0 / other
    }

    pub fn run_at_least_until_seconds(&mut self, system_time: ElapsedTime, seconds: f32) {
        self.0 = self.0.max(system_time.0 + seconds);
    }

    pub fn is_earlier_than(&self, system_time: ElapsedTime) -> bool {
        self.0 <= system_time.0
    }

    pub fn is_later_than(&self, other: ElapsedTime) -> bool {
        self.0 > other.0
    }

    pub fn max(&self, other: ElapsedTime) -> ElapsedTime {
        ElapsedTime(self.0.max(other.0))
    }

    pub fn min(&self, other: ElapsedTime) -> ElapsedTime {
        ElapsedTime(self.0.min(other.0))
    }

    pub fn as_f32(&self) -> f32 {
        self.0
    }
}

fn main() {
    let config = AppConfig::new().expect("Could not load config file ('config.toml')");

    simple_logging::log_to_stderr(
        LevelFilter::from_str(&config.log_level)
            .expect("Unknown log level. Please set one of the following values for 'log_level' in 'config.toml': \"OFF\", \"ERROR\", \"WARN\", \"INFO\", \"DEBUG\", \"TRACE\"")
    );
    let (elapsed, asset_loader) = measure_time(|| {
        AssetLoader::new(config.grf_paths.as_slice())
            .expect("Could not open asset files. Please configure them in 'config.toml'")
    });
    log::info!("GRF loading: {}ms", elapsed.as_millis());
    let sdl_context = sdl2::init().unwrap();
    let mut video = Video::init(&sdl_context);

    let _audio = sdl_context.audio();
    let frequency = sdl2::mixer::DEFAULT_FREQUENCY;
    let format = sdl2::mixer::DEFAULT_FORMAT; // signed 16 bit samples, in little-endian byte order
    let channels = sdl2::mixer::DEFAULT_CHANNELS; // Stereo
    let chunk_size = 1_024;
    sdl2::mixer::open_audio(frequency, format, channels, chunk_size);
    let _mixer_context = sdl2::mixer::init(
        sdl2::mixer::InitFlag::MP3
            | sdl2::mixer::InitFlag::FLAC
            | sdl2::mixer::InitFlag::MOD
            | sdl2::mixer::InitFlag::OGG,
    )
    .unwrap();
    sdl2::mixer::allocate_channels(4);
    sdl2::mixer::Channel::all().set_volume(32);

    let mut sound_system = SoundSystem::new();
    let sounds = Sounds {
        attack: sound_system
            .load_wav("data\\wav\\_novice_attack.wav", &asset_loader)
            .unwrap(),
        heal: sound_system
            .load_wav("data\\wav\\_heal_effect.wav", &asset_loader)
            .unwrap(),
        firewall: sound_system
            .load_wav("data\\wav\\effect\\ef_firewall.wav", &asset_loader)
            .unwrap(),
    };

    let shaders = Shaders {
        ground_shader: ShaderProgram::from_shaders(
            &[
                Shader::from_source(include_str!("shaders/ground.vert"), gl::VERTEX_SHADER)
                    .unwrap(),
                Shader::from_source(include_str!("shaders/ground.frag"), gl::FRAGMENT_SHADER)
                    .unwrap(),
            ],
            |program_id| GroundShaderParameters::new(program_id),
        )
        .unwrap(),
        model_shader: ShaderProgram::from_shaders(
            &[
                Shader::from_source(include_str!("shaders/model.vert"), gl::VERTEX_SHADER).unwrap(),
                Shader::from_source(include_str!("shaders/model.frag"), gl::FRAGMENT_SHADER)
                    .unwrap(),
            ],
            |program_id| ModelShaderParameters::new(program_id),
        )
        .unwrap(),
        sprite_shader: ShaderProgram::from_shaders(
            &[
                Shader::from_source(include_str!("shaders/sprite.vert"), gl::VERTEX_SHADER)
                    .unwrap(),
                Shader::from_source(include_str!("shaders/sprite.frag"), gl::FRAGMENT_SHADER)
                    .unwrap(),
            ],
            |program_id| Sprite3dShaderParameters::new(program_id),
        )
        .unwrap(),
        str_effect_shader: ShaderProgram::from_shaders(
            &[
                Shader::from_source(include_str!("shaders/str_effect.vert"), gl::VERTEX_SHADER)
                    .unwrap(),
                Shader::from_source(include_str!("shaders/str_effect.frag"), gl::FRAGMENT_SHADER)
                    .unwrap(),
            ],
            |program_id| StrEffect3dShaderParameters::new(program_id),
        )
        .unwrap(),
        sprite2d_shader: ShaderProgram::from_shaders(
            &[
                Shader::from_source(include_str!("shaders/sprite2d.vert"), gl::VERTEX_SHADER)
                    .unwrap(),
                Shader::from_source(include_str!("shaders/sprite2d.frag"), gl::FRAGMENT_SHADER)
                    .unwrap(),
            ],
            |program_id| Texture2dShaderParameters::new(program_id),
        )
        .unwrap(),
        trimesh_shader: ShaderProgram::from_shaders(
            &[
                Shader::from_source(include_str!("shaders/trimesh.vert"), gl::VERTEX_SHADER)
                    .unwrap(),
                Shader::from_source(include_str!("shaders/trimesh.frag"), gl::FRAGMENT_SHADER)
                    .unwrap(),
            ],
            |program_id| Trimesh3dShaderParameters::new(program_id),
        )
        .unwrap(),
        trimesh2d_shader: ShaderProgram::from_shaders(
            &[
                Shader::from_source(include_str!("shaders/trimesh2d.vert"), gl::VERTEX_SHADER)
                    .unwrap(),
                Shader::from_source(include_str!("shaders/trimesh2d.frag"), gl::FRAGMENT_SHADER)
                    .unwrap(),
            ],
            |program_id| Trimesh2dShaderParameters::new(program_id),
        )
        .unwrap(),
    };

    let ttf_context = sdl2::ttf::init().map_err(|e| e.to_string()).unwrap();

    let mut asset_database = AssetDatabase::new();

    let (elapsed, sprites) = measure_time(|| {
        let job_name_table = job_name_table();
        Sprites {
            cursors: asset_loader
                .load_spr_and_act("data\\sprite\\cursors", &mut asset_database)
                .unwrap(),
            numbers: GlTexture::from_file("assets\\damage.bmp", &mut asset_database),
            mounted_character_sprites: {
                let mut mounted_sprites = HashMap::new();
                let mounted_file_name = &job_name_table[&JobId::CRUSADER2];
                let folder1 = encoding::all::WINDOWS_1252
                    .decode(&[0xC0, 0xCE, 0xB0, 0xA3, 0xC1, 0xB7], DecoderTrap::Strict)
                    .unwrap();
                let folder2 = encoding::all::WINDOWS_1252
                    .decode(&[0xB8, 0xF6, 0xC5, 0xEB], DecoderTrap::Strict)
                    .unwrap();
                let male_file_name = format!(
                    "data\\sprite\\{}\\{}\\³²\\{}_³²",
                    folder1, folder2, mounted_file_name
                );
                let mut male = asset_loader
                    .load_spr_and_act(&male_file_name, &mut asset_database)
                    .expect(&format!("Failed loading {:?}", JobId::CRUSADER2));
                // for Idle action, character sprites contains head rotating animations, we don't need them
                male.action
                    .remove_frames_in_every_direction(CharActionIndex::Idle as usize, 1..);
                let female = male.clone();
                mounted_sprites.insert(JobId::CRUSADER, [male, female]);
                mounted_sprites
            },
            character_sprites: PLAYABLE_OUTLOOKS
                .iter()
                .map(|&job_id| {
                    let job_file_name = &job_name_table[&job_id];
                    let folder1 = encoding::all::WINDOWS_1252
                        .decode(&[0xC0, 0xCE, 0xB0, 0xA3, 0xC1, 0xB7], DecoderTrap::Strict)
                        .unwrap();
                    let folder2 = encoding::all::WINDOWS_1252
                        .decode(&[0xB8, 0xF6, 0xC5, 0xEB], DecoderTrap::Strict)
                        .unwrap();
                    let male_file_name = format!(
                        "data\\sprite\\{}\\{}\\³²\\{}_³²",
                        folder1, folder2, job_file_name
                    );
                    let female_file_name = format!(
                        "data\\sprite\\{}\\{}\\¿©\\{}_¿©",
                        folder1, folder2, job_file_name
                    );
                    let (male, female) = if !asset_loader
                        .exists(&format!("{}.act", female_file_name))
                    {
                        let mut male = asset_loader
                            .load_spr_and_act(&male_file_name, &mut asset_database)
                            .expect(&format!("Failed loading {:?}", job_id));
                        // for Idle action, character sprites contains head rotating animations, we don't need them
                        male.action
                            .remove_frames_in_every_direction(CharActionIndex::Idle as usize, 1..);
                        let female = male.clone();
                        (male, female)
                    } else if !asset_loader.exists(&format!("{}.act", male_file_name)) {
                        let mut female = asset_loader
                            .load_spr_and_act(&female_file_name, &mut asset_database)
                            .expect(&format!("Failed loading {:?}", job_id));
                        // for Idle action, character sprites contains head rotating animations, we don't need them
                        female
                            .action
                            .remove_frames_in_every_direction(CharActionIndex::Idle as usize, 1..);
                        let male = female.clone();
                        (male, female)
                    } else {
                        let mut male = asset_loader
                            .load_spr_and_act(&male_file_name, &mut asset_database)
                            .expect(&format!("Failed loading {:?}", job_id));
                        // for Idle action, character sprites contains head rotating animations, we don't need them
                        male.action
                            .remove_frames_in_every_direction(CharActionIndex::Idle as usize, 1..);
                        let mut female = asset_loader
                            .load_spr_and_act(&female_file_name, &mut asset_database)
                            .expect(&format!("Failed loading {:?}", job_id));
                        // for Idle action, character sprites contains head rotating animations, we don't need them
                        female
                            .action
                            .remove_frames_in_every_direction(CharActionIndex::Idle as usize, 1..);
                        (male, female)
                    };
                    (job_id, [male, female])
                })
                .collect::<HashMap<JobId, [SpriteResource; 2]>>(),
            head_sprites: [
                (1..=25)
                    .map(|i| {
                        let male_file_name = format!(
                            "data\\sprite\\ÀÎ°£Á·\\¸Ó¸®Åë\\³²\\{}_³²",
                            i.to_string()
                        );
                        let male = if asset_loader.exists(&(male_file_name.clone() + ".act")) {
                            let mut head = asset_loader
                                .load_spr_and_act(&male_file_name, &mut asset_database)
                                .expect(&format!("Failed loading head({})", i));
                            // for Idle action, character sprites contains head rotating animations, we don't need them
                            head.action.remove_frames_in_every_direction(
                                CharActionIndex::Idle as usize,
                                1..,
                            );
                            Some(head)
                        } else {
                            None
                        };
                        male
                    })
                    .filter_map(|it| it)
                    .collect::<Vec<SpriteResource>>(),
                (1..=25)
                    .map(|i| {
                        let female_file_name = format!(
                            "data\\sprite\\ÀÎ°£Á·\\¸Ó¸®Åë\\¿©\\{}_¿©",
                            i.to_string()
                        );
                        let female = if asset_loader.exists(&(female_file_name.clone() + ".act")) {
                            let mut head = asset_loader
                                .load_spr_and_act(&female_file_name, &mut asset_database)
                                .expect(&format!("Failed loading head({})", i));
                            // for Idle action, character sprites contains head rotating animations, we don't need them
                            head.action.remove_frames_in_every_direction(
                                CharActionIndex::Idle as usize,
                                1..,
                            );
                            Some(head)
                        } else {
                            None
                        };
                        female
                    })
                    .filter_map(|it| it)
                    .collect::<Vec<SpriteResource>>(),
            ],
            monster_sprites: MonsterId::iter()
                .map(|monster_id| {
                    let file_name = format!(
                        "data\\sprite\\npc\\{}",
                        monster_id.to_string().to_lowercase()
                    );
                    (
                        monster_id,
                        asset_loader
                            .load_spr_and_act(&file_name, &mut asset_database)
                            .unwrap(),
                    )
                })
                .collect::<HashMap<MonsterId, SpriteResource>>(),
            effect_sprites: EffectSprites {
                torch: asset_loader
                    .load_spr_and_act("data\\sprite\\ÀÌÆÑÆ®\\torch_01", &mut asset_database)
                    .unwrap(),
                fire_wall: asset_loader
                    .load_spr_and_act("data\\sprite\\ÀÌÆÑÆ®\\firewall", &mut asset_database)
                    .unwrap(),
                fire_ball: asset_loader
                    .load_spr_and_act("data\\sprite\\ÀÌÆÑÆ®\\fireball", &mut asset_database)
                    .unwrap(),
            },
        }
    });

    log::info!(
        "act and spr files loaded[{}]: {}ms",
        (sprites.character_sprites.len() * 2)
            + sprites.head_sprites[0].len()
            + sprites.head_sprites[1].len()
            + sprites.monster_sprites.len(),
        elapsed.as_millis()
    );

    let mut map_name_filter = ImString::new("prontera");
    let mut filtered_map_names: Vec<String> = vec![];
    let all_map_names = asset_loader
        .read_dir("data")
        .into_iter()
        .filter(|file_name| file_name.ends_with("rsw"))
        .map(|mut file_name| {
            file_name.drain(..5); // remove "data\\" from the begining
            let len = file_name.len();
            file_name.truncate(len - 4); // and extension from the end
            file_name
        })
        .collect::<Vec<String>>();
    let all_str_names = asset_loader
        .read_dir("data\\texture\\effect")
        .into_iter()
        .filter(|file_name| file_name.ends_with("str"))
        .map(|mut file_name| {
            file_name.drain(.."data\\texture\\effect\\".len()); // remove dir from the beginning
            let len = file_name.len();
            file_name.truncate(len - 4); // and extension from the end
            file_name
        })
        .collect::<Vec<String>>();

    let mut fov = 0.638;
    let mut window_opened = false;
    let mut cam_angle = -60.0;
    let render_matrices = RenderMatrices {
        projection: Matrix4::new_perspective(
            VIDEO_WIDTH as f32 / VIDEO_HEIGHT as f32,
            fov,
            0.1f32,
            1000.0f32,
        ),
        ortho: ortho(0.0, VIDEO_WIDTH as f32, VIDEO_HEIGHT as f32, 0.0, -1.0, 1.0),
    };

    let map_name = "prontera";
    let (map_render_data, physics_world) = load_map(
        map_name,
        &asset_loader,
        &mut asset_database,
        config.quick_startup,
    );

    let skill_name_font =
        Video::load_font(&ttf_context, "assets/fonts/UbuntuMono-B.ttf", 32).unwrap();
    let mut skill_name_font_outline =
        Video::load_font(&ttf_context, "assets/fonts/UbuntuMono-B.ttf", 32).unwrap();
    skill_name_font_outline.set_outline_width(2);

    let skill_key_font =
        Video::load_font(&ttf_context, "assets/fonts/UbuntuMono-B.ttf", 20).unwrap();
    let mut skill_key_font_bold_outline =
        Video::load_font(&ttf_context, "assets/fonts/UbuntuMono-B.ttf", 20).unwrap();
    skill_key_font_bold_outline.set_outline_width(2);

    let mut skill_key_font_outline =
        Video::load_font(&ttf_context, "assets/fonts/UbuntuMono-B.ttf", 20).unwrap();
    skill_key_font_outline.set_outline_width(1);

    let small_font = Video::load_font(&ttf_context, "assets/fonts/UbuntuMono-B.ttf", 14).unwrap();
    let mut small_font_outline =
        Video::load_font(&ttf_context, "assets/fonts/UbuntuMono-B.ttf", 14).unwrap();
    small_font_outline.set_outline_width(1);

    let mut texts = Texts {
        skill_name_texts: HashMap::new(),
        skill_key_texts: HashMap::new(),
        custom_texts: HashMap::new(),
        attack_absorbed: Video::create_outline_text_texture(
            &skill_key_font,
            &skill_key_font_bold_outline,
            "absorb",
            &mut asset_database,
        ),
        attack_blocked: Video::create_outline_text_texture(
            &skill_key_font,
            &skill_key_font_bold_outline,
            "block",
            &mut asset_database,
        ),
        minus: Video::create_outline_text_texture(
            &small_font,
            &small_font_outline,
            "-",
            &mut asset_database,
        ),
        plus: Video::create_outline_text_texture(
            &small_font,
            &small_font_outline,
            "+",
            &mut asset_database,
        ),
    };

    for name in &[
        "Poison",
        "AbsorbShield",
        "FireBomb",
        "ArmorUp",
        "ArmorDown",
        "Heal",
        "Damage",
    ] {
        texts.custom_texts.insert(
            name.to_string(),
            Video::create_outline_text_texture(
                &skill_key_font,
                &skill_key_font_outline,
                name,
                &mut asset_database,
            ),
        );
    }
    STATUS_NAMES.iter().for_each(|name| {
        texts.custom_texts.insert(
            name.to_string(),
            Video::create_outline_text_texture(
                &skill_key_font,
                &skill_key_font_outline,
                name,
                &mut asset_database,
            ),
        );
    });

    for i in -200..=200 {
        texts.custom_texts.insert(
            i.to_string(),
            Video::create_outline_text_texture(
                &small_font,
                &small_font_outline,
                &format!("{:+}", i),
                &mut asset_database,
            ),
        );
    }

    let mut skill_icons = HashMap::new();
    for skill in Skills::iter() {
        let texture = Video::create_outline_text_texture(
            &skill_name_font,
            &skill_name_font_outline,
            &format!("{:?}", skill),
            &mut asset_database,
        );
        texts.skill_name_texts.insert(skill, texture);

        let skill_icon = asset_loader
            .load_texture(skill.get_icon_path(), gl::NEAREST, &mut asset_database)
            .unwrap();
        skill_icons.insert(skill, skill_icon);
    }

    let mut status_icons = HashMap::new();
    status_icons.insert(
        "shield",
        asset_loader
            .load_texture(
                "data\\texture\\À¯ÀúÀÎÅÍÆäÀÌ½º\\item\\pa_shieldchain.bmp",
                gl::NEAREST,
                &mut asset_database,
            )
            .unwrap(),
    );

    for skill_key in SkillKey::iter() {
        let texture = Video::create_outline_text_texture(
            &skill_key_font,
            &skill_key_font_bold_outline,
            &format!("{:?}", skill_key),
            &mut asset_database,
        );
        texts.skill_key_texts.insert(skill_key, texture);
    }
    let (tx, runtime_conf_watcher_rx) = crossbeam_channel::unbounded();
    let mut watcher = notify::watcher(tx, Duration::from_secs(2)).unwrap();
    watcher
        .watch("config-runtime.toml", notify::RecursiveMode::NonRecursive)
        .unwrap();

    /////////////////////////
    ///// STR Effects
    /////////////////////////
    let mut gl_render_sys = OpenGlRenderSystem::new(&ttf_context);
    let (elapsed, str_effects) = measure_time(|| {
        let mut str_effects: Vec<StrFile> = Vec::new();

        load_and_prepare_effect(
            "firewall",
            StrEffectType::FireWall,
            &mut str_effects,
            &asset_loader,
            &mut asset_database,
            &mut gl_render_sys,
        );
        load_and_prepare_effect(
            "stormgust",
            StrEffectType::StormGust,
            &mut str_effects,
            &asset_loader,
            &mut asset_database,
            &mut gl_render_sys,
        );
        load_and_prepare_effect(
            "lord",
            StrEffectType::LordOfVermilion,
            &mut str_effects,
            &asset_loader,
            &mut asset_database,
            &mut gl_render_sys,
        );

        load_and_prepare_effect(
            "lightning",
            StrEffectType::Lightning,
            &mut str_effects,
            &asset_loader,
            &mut asset_database,
            &mut gl_render_sys,
        );
        load_and_prepare_effect(
            "concentration",
            StrEffectType::Concentration,
            &mut str_effects,
            &asset_loader,
            &mut asset_database,
            &mut gl_render_sys,
        );
        load_and_prepare_effect(
            "moonstar",
            StrEffectType::Moonstar,
            &mut str_effects,
            &asset_loader,
            &mut asset_database,
            &mut gl_render_sys,
        );
        load_and_prepare_effect(
            "hunter_poison",
            StrEffectType::Poison,
            &mut str_effects,
            &asset_loader,
            &mut asset_database,
            &mut gl_render_sys,
        );
        load_and_prepare_effect(
            "quagmire",
            StrEffectType::Quagmire,
            &mut str_effects,
            &asset_loader,
            &mut asset_database,
            &mut gl_render_sys,
        );
        load_and_prepare_effect(
            "firewall_blue",
            StrEffectType::FireWallBlue,
            &mut str_effects,
            &asset_loader,
            &mut asset_database,
            &mut gl_render_sys,
        );

        load_and_prepare_effect(
            "firepillarbomb",
            StrEffectType::FirePillarBomb,
            &mut str_effects,
            &asset_loader,
            &mut asset_database,
            &mut gl_render_sys,
        );
        load_and_prepare_effect(
            "ramadan",
            StrEffectType::Ramadan,
            &mut str_effects,
            &asset_loader,
            &mut asset_database,
            &mut gl_render_sys,
        );

        str_effects
    });
    log::info!("str loaded: {}ms", elapsed.as_millis());

    let mut ecs_world = specs::World::new();
    ecs_world.register::<BrowserClient>();
    ecs_world.register::<NpcComponent>();
    ecs_world.register::<HumanInputComponent>();
    ecs_world.register::<RenderCommandCollectorComponent>();
    ecs_world.register::<AudioCommandCollectorComponent>();
    ecs_world.register::<SpriteRenderDescriptorComponent>();
    ecs_world.register::<CharacterStateComponent>();
    ecs_world.register::<PhysicsComponent>();
    ecs_world.register::<FlyingNumberComponent>();
    ecs_world.register::<SoundEffectComponent>();
    ecs_world.register::<StrEffectComponent>();
    ecs_world.register::<SkillManifestationComponent>();
    ecs_world.register::<CameraComponent>();
    ecs_world.register::<ControllerComponent>();
    ecs_world.register::<MinionComponent>();
    ecs_world.register::<ConsoleComponent>();
    let command_defs: HashMap<String, CommandDefinition> =
        ConsoleSystem::init_commands(all_str_names);

    let mut ecs_dispatcher = specs::DispatcherBuilder::new()
        .with(BrowserInputProducerSystem, "browser_input_processor", &[])
        .with(
            InputConsumerSystem,
            "input_handler",
            &["browser_input_processor"],
        )
        .with(CameraSystem, "camera_system", &["input_handler"])
        .with(FrictionSystem, "friction_sys", &[])
        .with(
            InputToNextActionSystem,
            "input_to_next_action_sys",
            &["input_handler", "browser_input_processor"],
        )
        .with(MinionAiSystem, "minion_ai_sys", &[])
        .with(
            NextActionApplierSystem,
            "char_control",
            &[
                "friction_sys",
                "input_to_next_action_sys",
                "browser_input_processor",
            ],
        )
        .with(
            CharacterStateUpdateSystem,
            "char_state_update",
            &["char_control"],
        )
        .with(
            PhysCollisionCollectorSystem,
            "collision_collector",
            &["char_state_update"],
        )
        .with(SkillSystem, "skill_sys", &["collision_collector"])
        .with(AttackSystem, "attack_sys", &["collision_collector"])
        .with_thread_local(ConsoleSystem::new(&command_defs)) // thread_local to avoid Send fields
        .with_thread_local(RenderDesktopClientSystem::new())
        .with_thread_local(gl_render_sys)
        .with_thread_local(WebSocketBrowserRenderSystem::new())
        .with_thread_local(sound_system)
        .with_thread_local(FrameEndSystem)
        .build();

    ecs_world.add_resource(asset_database);

    ecs_world.add_resource(SystemVariables {
        asset_loader,
        dev_configs: DevConfig::new().unwrap(),
        assets: AssetResources {
            shaders,
            sprites,
            texts,
            skill_icons,
            status_icons,
            sounds,
        },
        tick: 1,
        dt: DeltaTime(0.0),
        time: ElapsedTime(0.0),
        matrices: render_matrices,
        map_render_data,
        attacks: Vec::with_capacity(128),
        area_attacks: Vec::with_capacity(128),
        pushes: Vec::with_capacity(128),
        apply_statuses: Vec::with_capacity(128),
        apply_area_statuses: Vec::with_capacity(128),
        remove_statuses: Vec::with_capacity(128),
        str_effect_vao: DynamicVertexArray::new(
            gl::TRIANGLE_STRIP,
            vec![
                1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0,
            ],
            4,
            vec![
                VertexAttribDefinition {
                    // xy
                    number_of_components: 2,
                    offset_of_first_element: 0,
                },
                VertexAttribDefinition {
                    // uv
                    number_of_components: 2,
                    offset_of_first_element: 2,
                },
            ],
        ),
        str_effects,
    });

    ecs_world.add_resource(CollisionsFromPrevFrame {
        collisions: HashMap::new(),
    });

    ecs_world.add_resource(physics_world);
    ecs_world.add_resource(SystemFrameDurations(HashMap::new()));
    let desktop_client_char = CharEntityId(ecs_world.create_entity().build());
    let desktop_client_controller = ControllerEntityId(ecs_world.create_entity().build());
    components::char::attach_human_player_components(
        "sharp",
        desktop_client_char,
        desktop_client_controller,
        &ecs_world.read_resource::<LazyUpdate>(),
        &mut ecs_world.write_resource::<PhysicEngine>(),
        ecs_world
            .read_resource::<SystemVariables>()
            .matrices
            .projection,
        Point2::new(250.0, -200.0),
        Sex::Male,
        JobId::CRUSADER,
        1,
        1,
        Team::Right,
        &ecs_world.read_resource::<SystemVariables>().dev_configs,
    );
    ecs_world
        .read_resource::<LazyUpdate>()
        .insert(desktop_client_controller.0, ConsoleComponent::new());
    ecs_world.maintain();

    let mut next_second: SystemTime = std::time::SystemTime::now()
        .checked_add(Duration::from_secs(1))
        .unwrap();
    let mut last_tick_time: u64 = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let mut next_minion_spawn = ElapsedTime(2.0);
    let mut fps_counter: u64 = 0;
    let mut fps: u64 = 0;
    let mut fps_history: Vec<f32> = Vec::with_capacity(30);
    let mut system_frame_durations = SystemFrameDurations(HashMap::new());

    let mut sent_bytes_per_second: usize = 0;
    let mut sent_bytes_per_second_counter: usize = 0;
    let mut websocket_server = websocket::sync::Server::bind("0.0.0.0:6969").unwrap();
    websocket_server.set_nonblocking(true).unwrap();

    // Add static skill manifestations
    {
        let area_status_id = ecs_world.create_entity().build();
        ecs_world
            .write_storage()
            .insert(
                area_status_id,
                SkillManifestationComponent::new(
                    area_status_id,
                    Box::new(StatusApplierArea::new(
                        "Poison".to_owned(),
                        move |_now| {
                            ApplyStatusComponentPayload::from_main_status(MainStatuses::Poison)
                        },
                        &v2!(251, -213),
                        v2!(2, 3),
                        desktop_client_char,
                        &mut ecs_world.write_resource::<PhysicEngine>(),
                    )),
                ),
            )
            .unwrap();

        let area_status_id = ecs_world.create_entity().build();
        ecs_world
            .write_storage()
            .insert(
                area_status_id,
                SkillManifestationComponent::new(
                    area_status_id,
                    Box::new(StatusApplierArea::new(
                        "AbsorbShield".to_owned(),
                        move |now| {
                            ApplyStatusComponentPayload::from_secondary(Box::new(
                                AbsorbStatus::new(desktop_client_char, now, 3.0),
                            ))
                        },
                        &v2!(255, -213),
                        v2!(2, 3),
                        desktop_client_char,
                        &mut ecs_world.write_resource::<PhysicEngine>(),
                    )),
                ),
            )
            .unwrap();

        let area_status_id = ecs_world.create_entity().build();
        ecs_world
            .write_storage()
            .insert(
                area_status_id,
                SkillManifestationComponent::new(
                    area_status_id,
                    Box::new(StatusApplierArea::new(
                        "FireBomb".to_owned(),
                        move |now| {
                            ApplyStatusComponentPayload::from_secondary(Box::new(FireBombStatus {
                                caster_entity_id: desktop_client_char,
                                started: now,
                                until: now.add_seconds(2.0),
                            }))
                        },
                        &v2!(260, -213),
                        v2!(2, 3),
                        desktop_client_char,
                        &mut ecs_world.write_resource::<PhysicEngine>(),
                    )),
                ),
            )
            .unwrap();

        // armor up
        let area_status_id = ecs_world.create_entity().build();
        ecs_world
            .write_storage()
            .insert(
                area_status_id,
                SkillManifestationComponent::new(
                    area_status_id,
                    Box::new(StatusApplierArea::new(
                        "ArmorUp".to_owned(),
                        move |now| {
                            ApplyStatusComponentPayload::from_secondary(Box::new(
                                ArmorModifierStatus::new(now, Percentage(70)),
                            ))
                        },
                        &v2!(265, -213),
                        v2!(2, 3),
                        desktop_client_char,
                        &mut ecs_world.write_resource::<PhysicEngine>(),
                    )),
                ),
            )
            .unwrap();

        // armor down
        let area_status_id = ecs_world.create_entity().build();
        ecs_world
            .write_storage()
            .insert(
                area_status_id,
                SkillManifestationComponent::new(
                    area_status_id,
                    Box::new(StatusApplierArea::new(
                        "ArmorDown".to_owned(),
                        move |now| {
                            ApplyStatusComponentPayload::from_secondary(Box::new(
                                ArmorModifierStatus::new(now, Percentage(-30)),
                            ))
                        },
                        &v2!(270, -213),
                        v2!(2, 3),
                        desktop_client_char,
                        &mut ecs_world.write_resource::<PhysicEngine>(),
                    )),
                ),
            )
            .unwrap();

        // HEAL
        let area_status_id = ecs_world.create_entity().build();
        ecs_world
            .write_storage()
            .insert(
                area_status_id,
                SkillManifestationComponent::new(
                    area_status_id,
                    Box::new(HealApplierArea::new(
                        "Heal",
                        AttackType::Heal(50),
                        &v2!(273, -213),
                        v2!(2, 3),
                        0.5,
                        desktop_client_char,
                        &mut ecs_world.write_resource::<PhysicEngine>(),
                    )),
                ),
            )
            .unwrap();
    }
    start_web_server();

    'running: loop {
        match websocket_server.accept() {
            Ok(wsupgrade) => {
                let browser_socket = wsupgrade.accept().unwrap();
                browser_socket.set_nonblocking(true).unwrap();

                let mut browser_client = BrowserClient::new(browser_socket);
                {
                    let asset_db: &AssetDatabase = &ecs_world.read_resource();
                    let system_vars = &ecs_world.read_resource::<SystemVariables>();
                    let welcome_data = json!({
                        "screen_width": VIDEO_WIDTH,
                        "screen_height": VIDEO_HEIGHT,
                        "map_name": map_name,
                        "asset_database": serde_json::to_value(asset_db).unwrap(),
                        "ground": json!({
                            "light_dir" : system_vars.map_render_data.rsw.light.direction,
                            "light_ambient" : system_vars.map_render_data.rsw.light.ambient,
                            "light_diffuse" : system_vars.map_render_data.rsw.light.diffuse,
                            "light_opacity" : system_vars.map_render_data.rsw.light.opacity,
                        }),
                        "projection_mat": system_vars
                                            .matrices
                                            .projection.as_slice(),
                        "ortho_mat": system_vars
                                            .matrices
                                            .ortho.as_slice()
                    });
                    let welcome_msg = serde_json::to_vec(&welcome_data).unwrap();
                    browser_client.send_message(&welcome_msg);
                };

                let browser_client_entity = ecs_world.create_entity().with(browser_client).build();
                log::info!("Client connected: {:?}", browser_client_entity);
            }
            _ => { /* Nobody tried to connect, move on.*/ }
        };

        {
            let projection_mat = ecs_world
                .read_resource::<SystemVariables>()
                .matrices
                .projection;
            let entities = &ecs_world.entities();
            let updater = ecs_world.read_resource::<LazyUpdate>();
            for (controller_id, client, _not_camera) in (
                &ecs_world.entities(),
                &mut ecs_world.write_storage::<BrowserClient>(),
                !&ecs_world.read_storage::<CameraComponent>(),
            )
                .join()
            {
                let controller_id = ControllerEntityId(controller_id);
                if let Ok(msg) = client.receive() {
                    match msg {
                        OwnedMessage::Binary(_buf) => {}
                        OwnedMessage::Text(text) => {
                            let deserialized: serde_json::Value =
                                serde_json::from_str(&text).unwrap();
                            if let Some(mismatched_textures) =
                                deserialized["mismatched_textures"].as_array()
                            {
                                log::trace!("mismatched_textures: {:?}", mismatched_textures);
                                let mut response_buf =
                                    Vec::with_capacity(mismatched_textures.len() * 256 * 256);
                                for mismatched_texture in mismatched_textures {
                                    ecs_world
                                        .read_resource::<AssetDatabase>()
                                        .copy_texture_into(
                                            mismatched_texture.as_str().unwrap_or(""),
                                            &mut response_buf,
                                        );
                                    client.send_message(&response_buf);
                                    response_buf.clear();
                                }
                                // send closing message
                                {
                                    response_buf.push(0xB1);
                                    response_buf.push(0x6B);
                                    response_buf.push(0x00);
                                    response_buf.push(0xB5);
                                    client.send_message(&response_buf);
                                }
                            }
                            if let Some(mismatched_vertex_buffers) =
                                deserialized["mismatched_vertex_buffers"].as_array()
                            {
                                log::trace!(
                                    "mismatched_vertex_buffers: {:?}",
                                    mismatched_vertex_buffers
                                );
                                let mut response_buf = Vec::with_capacity(256 * 256 * 4);
                                for mismatched_vertex_buffer in mismatched_vertex_buffers {
                                    if let Some("3d_ground") = mismatched_vertex_buffer.as_str() {
                                        response_buf.write_u8(1).unwrap();
                                        let ground_vao = &ecs_world
                                            .read_resource::<SystemVariables>()
                                            .map_render_data
                                            .ground_vertex_array;
                                        ground_vao.write_into(&mut response_buf);
                                        client.send_message(&response_buf);
                                        response_buf.clear();
                                    }
                                }
                                // send closing message
                                {
                                    response_buf.push(0xB1);
                                    response_buf.push(0x6B);
                                    response_buf.push(0x00);
                                    response_buf.push(0xB5);
                                    client.send_message(&response_buf);
                                }
                            }
                            if deserialized["send_me_model_instances"].as_bool().is_some() {
                                let mut response_buf = Vec::with_capacity(256 * 256 * 4);
                                for model_instance in &ecs_world
                                    .read_resource::<SystemVariables>()
                                    .map_render_data
                                    .model_instances
                                {
                                    response_buf
                                        .write_u32::<LittleEndian>(
                                            model_instance.asset_db_model_index as u32,
                                        )
                                        .unwrap();
                                    for v in &model_instance.matrix {
                                        response_buf.write_f32::<LittleEndian>(*v).unwrap();
                                    }
                                }
                                client.send_message(&response_buf);
                            }
                            if let Some(missing_models) = deserialized["missing_models"].as_array()
                            {
                                log::trace!("missing_models: {:?}", missing_models);
                                let mut response_buf =
                                    Vec::with_capacity(missing_models.len() * 256 * 256);
                                for missing_model in missing_models {
                                    ecs_world.read_resource::<AssetDatabase>().copy_model_into(
                                        missing_model.as_str().unwrap_or(""),
                                        &mut response_buf,
                                    );
                                    client.send_message(&response_buf);
                                    response_buf.clear();
                                }
                                // send closing message
                                {
                                    response_buf.push(0xB1);
                                    response_buf.push(0x6B);
                                    response_buf.push(0x00);
                                    response_buf.push(0xB5);
                                    client.send_message(&response_buf);
                                }
                            }
                            if deserialized["ready"].as_bool().is_some() {
                                let char_entity_id = CharEntityId(entities.create());
                                components::char::attach_human_player_components(
                                    "browser",
                                    char_entity_id,
                                    controller_id,
                                    &updater,
                                    &mut ecs_world.write_resource::<PhysicEngine>(),
                                    projection_mat,
                                    Point2::new(250.0, -200.0),
                                    Sex::Male,
                                    JobId::CRUSADER,
                                    2,
                                    1,
                                    Team::Right,
                                    &ecs_world.read_resource::<SystemVariables>().dev_configs,
                                );
                            }
                        }
                        OwnedMessage::Close(_) => {}
                        OwnedMessage::Ping(_) => {}
                        OwnedMessage::Pong(buf) => {
                            let now_ms = SystemTime::now()
                                .duration_since(SystemTime::UNIX_EPOCH)
                                .unwrap()
                                .as_millis();
                            let (int_bytes, _rest) = buf.split_at(std::mem::size_of::<u128>());
                            let ping_sent = u128::from_le_bytes(int_bytes.try_into().unwrap());
                            client.set_ping(now_ms - ping_sent);
                        }
                    }
                }
            }
        }

        {
            let mut storage = ecs_world.write_storage::<HumanInputComponent>();
            let inputs = storage.get_mut(desktop_client_controller.0).unwrap();

            for event in video.event_pump.poll_iter() {
                video.imgui_sdl2.handle_event(&mut video.imgui, &event);
                match event {
                    sdl2::event::Event::Quit { .. } => {
                        break 'running;
                    }
                    _ => {
                        inputs.inputs.push(event);
                    }
                }
            }
        }

        ecs_dispatcher.dispatch(&mut ecs_world.res);
        {
            // Run console commands
            let console_args = {
                let mut storage = ecs_world.write_storage::<ConsoleComponent>();
                let console = storage.get_mut(desktop_client_controller.0).unwrap();
                std::mem::replace(&mut console.command_to_execute, None)
            };
            if let Some(args) = console_args {
                let command_def = &command_defs[args.get_command_name().unwrap()];
                if let Err(e) = (command_def.action)(
                    desktop_client_controller,
                    desktop_client_char,
                    &args,
                    &mut ecs_world,
                ) {
                    ecs_world
                        .write_storage::<ConsoleComponent>()
                        .get_mut(desktop_client_controller.0)
                        .unwrap()
                        .error(&e);
                }
            }
        }
        ecs_world.maintain();

        let (new_map, show_cursor) = imgui_frame(
            desktop_client_controller,
            &mut video,
            &mut ecs_world,
            sent_bytes_per_second,
            &mut map_name_filter,
            &all_map_names,
            &mut filtered_map_names,
            fps,
            fps_history.as_slice(),
            &mut fov,
            &mut cam_angle,
            &mut window_opened,
            &system_frame_durations,
        );
        sdl_context.mouse().show_cursor(show_cursor);
        if let Some(new_map_name) = new_map {
            ecs_world.delete_all();
            let (map_render_data, physics_world) = load_map(
                &new_map_name,
                &ecs_world.read_resource::<SystemVariables>().asset_loader,
                &mut ecs_world.write_resource::<AssetDatabase>(),
                config.quick_startup,
            );
            ecs_world
                .write_resource::<SystemVariables>()
                .map_render_data = map_render_data;
            ecs_world.add_resource(physics_world);

            // TODO
        }

        video.gl_swap_window();
        std::thread::sleep(Duration::from_millis(
            ecs_world
                .read_resource::<SystemVariables>()
                .dev_configs
                .sleep_ms,
        ));
        let now = std::time::SystemTime::now();
        let now_ms = now
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let dt = (now_ms - last_tick_time) as f32 / 1000.0;
        last_tick_time = now_ms;
        if now >= next_second {
            fps = fps_counter;
            fps_history.push(fps as f32);
            if fps_history.len() > 30 {
                fps_history.remove(0);
            }
            //
            let sh = &mut ecs_world.write_resource::<SystemFrameDurations>().0;
            system_frame_durations.0 = sh.clone();
            sh.clear();

            fps_counter = 0;
            sent_bytes_per_second = sent_bytes_per_second_counter;
            sent_bytes_per_second_counter = 0;
            next_second = std::time::SystemTime::now()
                .checked_add(Duration::from_secs(1))
                .unwrap();

            video.set_title(&format!("Rustarok {} FPS", fps));

            // send a ping packet every second
            let now_ms = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis();
            let data = now_ms.to_le_bytes();
            let mut browser_storage = ecs_world.write_storage::<BrowserClient>();
            for browser_client in (&mut browser_storage).join() {
                browser_client.send_ping(&data);
                browser_client.reset_byte_per_second();
            }
        }
        fps_counter += 1;
        ecs_world.write_resource::<SystemVariables>().tick += 1;
        ecs_world.write_resource::<SystemVariables>().dt.0 =
            dt.min(MAX_SECONDS_ALLOWED_FOR_SINGLE_FRAME);
        ecs_world.write_resource::<SystemVariables>().time.0 +=
            dt.min(MAX_SECONDS_ALLOWED_FOR_SINGLE_FRAME);

        let now = ecs_world.read_resource::<SystemVariables>().time;
        if next_minion_spawn.is_earlier_than(now)
            && ecs_world
                .read_resource::<SystemVariables>()
                .dev_configs
                .minions_enabled
        {
            next_minion_spawn = now.add_seconds(2.0);

            {
                let char_entity_id = create_random_char_minion(
                    &mut ecs_world,
                    p2!(
                        MinionAiSystem::CHECKPOINTS[0][0],
                        MinionAiSystem::CHECKPOINTS[0][1]
                    ),
                    Team::Right,
                );
                ecs_world
                    .create_entity()
                    .with(ControllerComponent::new(char_entity_id))
                    .with(MinionComponent { fountain_up: false });
            }

            {
                let entity_id = create_random_char_minion(
                    &mut ecs_world,
                    p2!(
                        MinionAiSystem::CHECKPOINTS[5][0],
                        MinionAiSystem::CHECKPOINTS[5][1]
                    ),
                    Team::Left,
                );
                let mut storage = ecs_world.write_storage();
                storage
                    .insert(entity_id.0, MinionComponent { fountain_up: false })
                    .unwrap();
            }
        }

        // runtime configs
        match runtime_conf_watcher_rx.try_recv() {
            Ok(_event) => {
                if let Ok(new_config) = DevConfig::new() {
                    ecs_world.write_resource::<SystemVariables>().dev_configs = new_config;
                    for char_state in
                        (&mut ecs_world.write_storage::<CharacterStateComponent>()).join()
                    {
                        char_state.update_base_attributes(
                            &ecs_world.write_resource::<SystemVariables>().dev_configs,
                        );
                    }
                    log::info!("Configs has been reloaded");
                } else {
                    log::warn!("Config error");
                }
            }
            _ => {}
        };
    }
}

fn imgui_frame(
    desktop_client_entity: ControllerEntityId,
    video: &mut Video,
    ecs_world: &mut specs::world::World,
    sent_bytes_per_second: usize,
    mut map_name_filter: &mut ImString,
    all_map_names: &Vec<String>,
    filtered_map_names: &mut Vec<String>,
    fps: u64,
    fps_history: &[f32],
    fov: &mut f32,
    cam_angle: &mut f32,
    window_opened: &mut bool,
    system_frame_durations: &SystemFrameDurations,
) -> (Option<String>, bool) {
    let ui = video.imgui_sdl2.frame(
        &video.window,
        &mut video.imgui,
        &video.event_pump.mouse_state(),
    );
    let mut ret = (None, false); // (map, show_cursor)
    {
        // IMGUI
        ui.window(im_str!("Graphic options"))
            .position((0.0, 0.0), imgui::ImGuiCond::FirstUseEver)
            .size((300.0, 600.0), imgui::ImGuiCond::FirstUseEver)
            .opened(window_opened)
            .build(|| {
                ret.1 = ui.is_window_hovered();
                let map_name_filter_clone = map_name_filter.clone();
                if ui
                    .input_text(im_str!("Map name:"), &mut map_name_filter)
                    .enter_returns_true(false)
                    .build()
                {
                    filtered_map_names.clear();
                    filtered_map_names.extend(
                        all_map_names
                            .iter()
                            .filter(|map_name| {
                                let matc = sublime_fuzzy::best_match(
                                    map_name_filter_clone.to_str(),
                                    map_name,
                                );
                                matc.is_some()
                            })
                            .map(|it| it.to_owned()),
                    );
                }
                for map_name in filtered_map_names.iter() {
                    if ui.small_button(&ImString::new(map_name.as_str())) {
                        ret.0 = Some(map_name.to_owned());
                    }
                }

                if ui
                    .slider_float(im_str!("Perspective"), fov, 0.1, std::f32::consts::PI)
                    .build()
                {
                    ecs_world
                        .write_resource::<SystemVariables>()
                        .matrices
                        .projection = Matrix4::new_perspective(
                        VIDEO_WIDTH as f32 / VIDEO_HEIGHT as f32,
                        *fov,
                        0.1f32,
                        1000.0f32,
                    );
                }

                if ui
                    .slider_float(im_str!("Camera"), cam_angle, -120.0, 120.0)
                    .build()
                {
                    let mut storage = ecs_world.write_storage::<CameraComponent>();
                    let controller = storage.get_mut(desktop_client_entity.0).unwrap();
                    controller.camera.rotate(*cam_angle, 270.0);
                }

                let mut map_render_data = &mut ecs_world
                    .write_resource::<SystemVariables>()
                    .map_render_data;
                ui.checkbox(
                    im_str!("Use tile_colors"),
                    &mut map_render_data.use_tile_colors,
                );
                if ui.checkbox(
                    im_str!("Use use_lighting"),
                    &mut map_render_data.use_lighting,
                ) {
                    map_render_data.use_lightmaps =
                        map_render_data.use_lighting && map_render_data.use_lightmaps;
                }
                if ui.checkbox(im_str!("Use lightmaps"), &mut map_render_data.use_lightmaps) {
                    map_render_data.use_lighting =
                        map_render_data.use_lighting || map_render_data.use_lightmaps;
                }
                ui.checkbox(im_str!("Models"), &mut map_render_data.draw_models);
                ui.checkbox(im_str!("Ground"), &mut map_render_data.draw_ground);

                let camera = ecs_world
                    .read_storage::<CameraComponent>()
                    .get(desktop_client_entity.0)
                    .unwrap()
                    .clone();
                let mut storage = ecs_world.write_storage::<HumanInputComponent>();

                {
                    let controller = storage.get_mut(desktop_client_entity.0).unwrap();
                    let mut cast_mode = match controller.cast_mode {
                        CastMode::Normal => 0,
                        CastMode::OnKeyPress => 1,
                        CastMode::OnKeyRelease => 2,
                    };
                    if ui.combo(
                        im_str!("quick_cast"),
                        &mut cast_mode,
                        &[im_str!("Off"), im_str!("On"), im_str!("On Release")],
                        10,
                    ) {
                        controller.cast_mode = match cast_mode {
                            0 => CastMode::Normal,
                            1 => CastMode::OnKeyPress,
                            _ => CastMode::OnKeyRelease,
                        };
                    }
                    ui.text(im_str!(
                        "Mouse world pos: {}, {}",
                        controller.mouse_world_pos.x,
                        controller.mouse_world_pos.y,
                    ));
                }

                ui.drag_float3(
                    im_str!("light_dir"),
                    &mut map_render_data.rsw.light.direction,
                )
                .min(-1.0)
                .max(1.0)
                .speed(0.05)
                .build();
                ui.color_edit(
                    im_str!("light_ambient"),
                    &mut map_render_data.rsw.light.ambient,
                )
                .inputs(false)
                .format(imgui::ColorFormat::Float)
                .build();
                ui.color_edit(
                    im_str!("light_diffuse"),
                    &mut map_render_data.rsw.light.diffuse,
                )
                .inputs(false)
                .format(imgui::ColorFormat::Float)
                .build();
                ui.drag_float(
                    im_str!("light_opacity"),
                    &mut map_render_data.rsw.light.opacity,
                )
                .min(0.0)
                .max(1.0)
                .speed(0.05)
                .build();

                ui.text(im_str!(
                    "Maps: {},{},{}",
                    camera.camera.pos().x,
                    camera.camera.pos().y,
                    camera.camera.pos().z
                ));
                ui.text(im_str!("yaw: {}, pitch: {}", camera.yaw, camera.pitch));
                ui.text(im_str!("FPS: {}", fps));
                let (traffic, unit) = if sent_bytes_per_second > 1024 * 1024 {
                    (sent_bytes_per_second / 1024 / 1024, "Mb")
                } else if sent_bytes_per_second > 1024 {
                    (sent_bytes_per_second / 1024, "Kb")
                } else {
                    (sent_bytes_per_second, "bytes")
                };

                ui.plot_histogram(im_str!("FPS"), fps_history)
                    .scale_min(100.0)
                    .scale_max(145.0)
                    .graph_size(ImVec2::new(0.0f32, 200.0f32))
                    .build();
                ui.text(im_str!("Systems[micro sec]: "));
                for (sys_name, durations) in system_frame_durations.0.iter() {
                    let diff = (durations.max / 100) as f32 / (durations.min / 100).max(1) as f32;

                    let color = if diff < 1.5 && durations.avg < 5000 {
                        (0.0, 1.0, 0.0, 1.0)
                    } else if diff < 2.0 && durations.avg < 5000 {
                        (1.0, 0.75, 0.0, 1.0)
                    } else if diff < 2.5 && durations.avg < 5000 {
                        (1.0, 0.5, 0.0, 1.0)
                    } else if durations.avg < 5000 {
                        (1.0, 0.25, 0.0, 1.0)
                    } else {
                        (1.0, 0.0, 0.0, 1.0)
                    };
                    ui.text_colored(
                        color,
                        im_str!(
                            "{}: {}, {}, {}",
                            sys_name,
                            durations.min,
                            durations.max,
                            durations.avg
                        ),
                    );
                }
                ui.text(im_str!("Traffic: {} {}", traffic, unit));

                let browser_storage = ecs_world.read_storage::<BrowserClient>();
                for browser_client in browser_storage.join() {
                    ui.bullet_text(im_str!("Ping: {} ms", browser_client.ping));
                }
            });
    }
    video.renderer.render(ui);
    return ret;
}

fn create_random_char_minion(
    ecs_world: &mut World,
    pos2d: Point2<f32>,
    team: Team,
) -> CharEntityId {
    let mut rng = rand::thread_rng();
    let sex = if rng.gen::<usize>() % 2 == 0 {
        Sex::Male
    } else {
        Sex::Female
    };

    let job_id = if rng.gen::<usize>() % 2 == 0 {
        JobId::SWORDMAN
    } else {
        JobId::ARCHER
    };
    let head_count = ecs_world
        .read_resource::<SystemVariables>()
        .assets
        .sprites
        .head_sprites[Sex::Male as usize]
        .len();
    let entity_id = CharEntityId(ecs_world.create_entity().build());
    ecs_world
        .read_resource::<LazyUpdate>()
        .insert(entity_id.0, NpcComponent);
    components::char::attach_char_components(
        "minion".to_owned(),
        entity_id,
        &ecs_world.read_resource::<LazyUpdate>(),
        &mut ecs_world.write_resource::<PhysicEngine>(),
        pos2d,
        sex,
        job_id,
        rng.gen::<usize>() % head_count,
        1,
        team,
        CharType::Minion,
        CollisionGroup::NonPlayer,
        &[
            //CollisionGroup::NonPlayer,
            CollisionGroup::Player,
            CollisionGroup::StaticModel,
        ],
        &ecs_world.read_resource::<SystemVariables>().dev_configs,
    );
    entity_id
}

pub struct ModelInstance {
    asset_db_model_index: usize,
    matrix: Matrix4<f32>,
    bottom_left_front: Vector3<f32>,
    top_right_back: Vector3<f32>,
}

pub struct MapRenderData {
    pub gat: Gat,
    pub gnd: Gnd,
    pub rsw: Rsw,
    pub use_tile_colors: bool,
    pub use_lightmaps: bool,
    pub use_lighting: bool,
    pub ground_vertex_array: VertexArray,
    pub centered_sprite_vertex_array: VertexArray,
    pub sprite_vertex_array: VertexArray,
    pub rectangle_vertex_array: VertexArray,
    pub texture_atlas: GlTexture,
    pub tile_color_texture: GlTexture,
    pub lightmap_texture: GlTexture,
    pub model_instances: Vec<ModelInstance>,
    pub draw_models: bool,
    pub draw_ground: bool,
    pub ground_walkability_mesh: VertexArray,
    pub ground_walkability_mesh2: VertexArray,
    pub ground_walkability_mesh3: VertexArray,
}

pub struct ModelRenderData {
    pub bounding_box: BoundingBox,
    pub alpha: u8,
    pub model: Vec<DataForRenderingSingleNode>,
}

pub type DataForRenderingSingleNode = Vec<SameTextureNodeFaces>;

pub struct SameTextureNodeFaces {
    pub vao: VertexArray,
    pub texture: GlTexture,
    pub texture_name: String,
}

pub fn measure_time<T, F: FnOnce() -> T>(f: F) -> (Duration, T) {
    let start = Instant::now();
    let r = f();
    (start.elapsed(), r)
}

struct GroundLoadResult {
    ground_vertex_array: VertexArray,
    ground_walkability_mesh: VertexArray,
    ground_walkability_mesh2: VertexArray,
    ground_walkability_mesh3: VertexArray,
    ground: Gnd,
    texture_atlas: GlTexture,
    tile_color_texture: GlTexture,
    lightmap_texture: GlTexture,
}

fn load_ground(
    map_name: &str,
    gat: &Gat,
    water: &WaterData,
    colliders: &Vec<(Vector2<f32>, Vector2<f32>)>,
    asset_loader: &AssetLoader,
    asset_database: &mut AssetDatabase,
) -> GroundLoadResult {
    let mut v = Vector3::<f32>::new(0.0, 0.0, 0.0);
    let rot = Rotation3::<f32>::new(Vector3::new(180f32.to_radians(), 0.0, 0.0));
    let mut rotate_around_x_axis = |mut pos: Point3<f32>| {
        v.x = pos[0];
        v.y = pos[1];
        v.z = pos[2];
        v = rot * v;
        pos[0] = v.x;
        pos[1] = v.y;
        pos[2] = v.z;
        pos
    };

    let vertices: Vec<Point3<f32>> = gat
        .rectangles
        .iter()
        .map(|cell| {
            let x = cell.start_x as f32;
            let x2 = (cell.start_x + cell.width) as f32;
            let y = (cell.bottom - cell.height + 1) as f32;
            let y2 = (cell.bottom + 1) as f32;
            vec![
                rotate_around_x_axis(Point3::new(x, -2.0, y2)),
                rotate_around_x_axis(Point3::new(x2, -2.0, y2)),
                rotate_around_x_axis(Point3::new(x, -2.0, y)),
                rotate_around_x_axis(Point3::new(x, -2.0, y)),
                rotate_around_x_axis(Point3::new(x2, -2.0, y2)),
                rotate_around_x_axis(Point3::new(x2, -2.0, y)),
            ]
        })
        .flatten()
        .collect();

    let vertices2: Vec<Point3<f32>> = gat
        .cells
        .iter()
        .enumerate()
        .map(|(i, cell)| {
            let x = (i as u32 % gat.width) as f32;
            let y = (i as u32 / gat.width) as f32;
            if cell.cell_type & CellType::Walkable as u8 == 0 {
                vec![
                    rotate_around_x_axis(Point3::new(x + 0.0, -1.0, y + 1.0)),
                    rotate_around_x_axis(Point3::new(x + 1.0, -1.0, y + 1.0)),
                    rotate_around_x_axis(Point3::new(x + 0.0, -1.0, y + 0.0)),
                    rotate_around_x_axis(Point3::new(x + 0.0, -1.0, y + 0.0)),
                    rotate_around_x_axis(Point3::new(x + 1.0, -1.0, y + 1.0)),
                    rotate_around_x_axis(Point3::new(x + 1.0, -1.0, y + 0.0)),
                ]
            } else {
                vec![]
            }
        })
        .flatten()
        .collect();
    let ground_walkability_mesh = VertexArray::new(
        gl::TRIANGLES,
        vertices,
        vec![VertexAttribDefinition {
            number_of_components: 3,
            offset_of_first_element: 0,
        }],
    );
    let ground_walkability_mesh2 = VertexArray::new(
        gl::TRIANGLES,
        vertices2,
        vec![VertexAttribDefinition {
            number_of_components: 3,
            offset_of_first_element: 0,
        }],
    );
    let vertices: Vec<Point3<f32>> = colliders
        .iter()
        .map(|(extents, pos)| {
            let x = pos.x - extents.x;
            let x2 = pos.x + extents.x;
            let y = pos.y - extents.y;
            let y2 = pos.y + extents.y;
            vec![
                Point3::new(x, 3.0, y2),
                Point3::new(x2, 3.0, y2),
                Point3::new(x, 3.0, y),
                Point3::new(x, 3.0, y),
                Point3::new(x2, 3.0, y2),
                Point3::new(x2, 3.0, y),
            ]
        })
        .flatten()
        .collect();
    let ground_walkability_mesh3 = VertexArray::new(
        gl::TRIANGLES,
        vertices,
        vec![VertexAttribDefinition {
            number_of_components: 3,
            offset_of_first_element: 0,
        }],
    );
    let (elapsed, mut ground) = measure_time(|| {
        asset_loader
            .load_gnd(map_name, water.level, water.wave_height)
            .unwrap()
    });
    log::info!("gnd loaded: {}ms", elapsed.as_millis());
    let (elapsed, texture_atlas) = measure_time(|| {
        Gnd::create_gl_texture_atlas(&asset_loader, asset_database, &ground.texture_names)
    });
    log::info!("gnd texture_atlas loaded: {}ms", elapsed.as_millis());

    let tile_color_texture = Gnd::create_tile_color_texture(
        &mut ground.tiles_color_image,
        ground.width,
        ground.height,
        asset_database,
    );
    let lightmap_texture = Gnd::create_lightmap_texture(
        &ground.lightmap_image,
        ground.lightmaps.count,
        asset_database,
    );
    let ground_vertex_array = VertexArray::new(
        gl::TRIANGLES,
        std::mem::replace(&mut ground.mesh, vec![]),
        vec![
            VertexAttribDefinition {
                number_of_components: 3,
                offset_of_first_element: 0,
            },
            VertexAttribDefinition {
                // normals
                number_of_components: 3,
                offset_of_first_element: 3,
            },
            VertexAttribDefinition {
                // texcoords
                number_of_components: 2,
                offset_of_first_element: 6,
            },
            VertexAttribDefinition {
                // lightmap_coord
                number_of_components: 2,
                offset_of_first_element: 8,
            },
            VertexAttribDefinition {
                // tile color coordinate
                number_of_components: 2,
                offset_of_first_element: 10,
            },
        ],
    );
    GroundLoadResult {
        ground_vertex_array,
        ground_walkability_mesh,
        ground_walkability_mesh2,
        ground_walkability_mesh3,
        ground,
        texture_atlas,
        tile_color_texture,
        lightmap_texture,
    }
}

fn load_map(
    map_name: &str,
    asset_loader: &AssetLoader,
    asset_database: &mut AssetDatabase,
    quick_loading: bool,
) -> (MapRenderData, PhysicEngine) {
    let (elapsed, world) = measure_time(|| asset_loader.load_map(&map_name).unwrap());
    log::info!("rsw loaded: {}ms", elapsed.as_millis());
    let (elapsed, gat) = measure_time(|| asset_loader.load_gat(map_name).unwrap());
    log::info!("gat loaded: {}ms", elapsed.as_millis());

    let mut physics_world = PhysicEngine {
        mechanical_world: DefaultMechanicalWorld::new(Vector2::zeros()),
        geometrical_world: DefaultGeometricalWorld::new(),

        bodies: DefaultBodySet::new(),
        colliders: DefaultColliderSet::new(),
        joint_constraints: DefaultJointConstraintSet::new(),
        force_generators: DefaultForceGeneratorSet::new(),
    };

    let colliders: Vec<(Vector2<f32>, Vector2<f32>)> = gat
        .rectangles
        .iter()
        .map(|cell| {
            let rot = Rotation3::<f32>::new(Vector3::new(180f32.to_radians(), 0.0, 0.0));
            let half_w = cell.width as f32 / 2.0;
            let x = cell.start_x as f32 + half_w;
            let half_h = cell.height as f32 / 2.0;
            let y = (cell.bottom - cell.height) as f32 + 1.0 + half_h;
            let half_extents = Vector2::new(half_w, half_h);

            let cuboid = ShapeHandle::new(ncollide2d::shape::Cuboid::new(half_extents));
            let v = rot * Vector3::new(x, 0.0, y);
            let v2 = Vector2::new(v.x, v.z);
            let parent_rigid_body = RigidBodyDesc::new()
                .translation(v2)
                .gravity_enabled(false)
                .status(nphysics2d::object::BodyStatus::Static)
                .build();
            let parent_handle = physics_world.bodies.insert(parent_rigid_body);
            let cuboid = ColliderDesc::new(cuboid)
                .density(10.0)
                .collision_groups(
                    CollisionGroups::new()
                        .with_membership(&[CollisionGroup::StaticModel as usize])
                        .with_blacklist(&[CollisionGroup::StaticModel as usize]),
                )
                .build(BodyPartHandle(parent_handle, 0));
            let cuboid_pos = cuboid.position_wrt_body().translation.vector;
            physics_world.colliders.insert(cuboid);
            (half_extents, cuboid_pos)
        })
        .collect();

    let ground_data = load_ground(
        map_name,
        &gat,
        &world.water,
        &colliders,
        asset_loader,
        asset_database,
    );

    ////////////////////////////
    //// MODELS
    ////////////////////////////
    {
        let (elapsed, models) = measure_time(|| {
            if !quick_loading {
                let model_names: HashSet<_> =
                    world.models.iter().map(|m| m.filename.clone()).collect();
                return model_names
                    .iter()
                    .map(|filename| {
                        let rsm = asset_loader.load_model(filename).unwrap();
                        (filename.clone(), rsm)
                    })
                    .collect::<Vec<(String, Rsm)>>();
            } else {
                vec![]
            }
        });
        log::info!("models[{}] loaded: {}ms", models.len(), elapsed.as_millis());

        let (elapsed, model_render_datas) = measure_time(|| {
            models
                .iter()
                .map(|(name, rsm)| {
                    let textures =
                        Rsm::load_textures(&asset_loader, asset_database, &rsm.texture_names);
                    log::trace!("{} textures loaded for model {}", textures.len(), name);
                    let (data_for_rendering_full_model, bbox): (
                        Vec<DataForRenderingSingleNode>,
                        BoundingBox,
                    ) = Rsm::generate_meshes_by_texture_id(
                        &rsm.bounding_box,
                        rsm.shade_type,
                        rsm.nodes.len() == 1,
                        &rsm.nodes,
                        &textures,
                    );
                    (
                        name.clone(),
                        ModelRenderData {
                            bounding_box: bbox,
                            alpha: rsm.alpha,
                            model: data_for_rendering_full_model,
                        },
                    )
                })
                .collect::<HashMap<String, ModelRenderData>>()
        });
        log::info!("model_render_datas loaded: {}ms", elapsed.as_millis());
        asset_database.register_models(model_render_datas);
    };

    let model_instances_iter = if quick_loading {
        world.models.iter().take(0)
    } else {
        let len = world.models.len();
        world.models.iter().take(len)
    };
    let model_instances: Vec<ModelInstance> = model_instances_iter
        .map(|model_instance| {
            let mut only_transition_matrix = Matrix4::<f32>::identity();
            only_transition_matrix.prepend_translation_mut(
                &(model_instance.pos
                    + Vector3::new(
                        ground_data.ground.width as f32,
                        0f32,
                        ground_data.ground.height as f32,
                    )),
            );

            let mut instance_matrix = only_transition_matrix.clone();
            // rot_z
            let rotation = Rotation3::from_axis_angle(
                &Unit::new_normalize(Vector3::z()),
                model_instance.rot.z.to_radians(),
            )
            .to_homogeneous();
            instance_matrix = instance_matrix * rotation;
            // rot x
            let rotation = Rotation3::from_axis_angle(
                &Unit::new_normalize(Vector3::x()),
                model_instance.rot.x.to_radians(),
            )
            .to_homogeneous();
            instance_matrix = instance_matrix * rotation;
            // rot y
            let rotation = Rotation3::from_axis_angle(
                &Unit::new_normalize(Vector3::y()),
                model_instance.rot.y.to_radians(),
            )
            .to_homogeneous();
            instance_matrix = instance_matrix * rotation;

            instance_matrix.prepend_nonuniform_scaling_mut(&model_instance.scale);
            only_transition_matrix.prepend_nonuniform_scaling_mut(&model_instance.scale);

            let rotation =
                Rotation3::from_axis_angle(&Unit::new_normalize(Vector3::x()), 180f32.to_radians())
                    .to_homogeneous();
            instance_matrix = rotation * instance_matrix;
            only_transition_matrix = rotation * only_transition_matrix;

            let model_db_index = asset_database.get_model_index(&model_instance.filename);
            let model_render_data = asset_database.get_model(model_db_index);
            let tmin = only_transition_matrix
                .transform_point(&model_render_data.bounding_box.min)
                .coords;
            let tmax = only_transition_matrix
                .transform_point(&model_render_data.bounding_box.max)
                .coords;
            let min = Vector3::new(
                tmin[0].min(tmax[0]),
                tmin[1].min(tmax[1]),
                tmin[2].max(tmax[2]),
            );
            let max = Vector3::new(
                tmax[0].max(tmin[0]),
                tmax[1].max(tmin[1]),
                tmax[2].min(tmin[2]),
            );
            ModelInstance {
                asset_db_model_index: model_db_index,
                matrix: instance_matrix,
                bottom_left_front: min,
                top_right_back: max,
            }
        })
        .collect();

    let s: Vec<[f32; 4]> = vec![
        [-0.5, 0.5, 0.0, 0.0],
        [0.5, 0.5, 1.0, 0.0],
        [-0.5, -0.5, 0.0, 1.0],
        [0.5, -0.5, 1.0, 1.0],
    ];
    let centered_sprite_vertex_array = VertexArray::new(
        gl::TRIANGLE_STRIP,
        s,
        vec![
            VertexAttribDefinition {
                number_of_components: 2,
                offset_of_first_element: 0,
            },
            VertexAttribDefinition {
                // uv
                number_of_components: 2,
                offset_of_first_element: 2,
            },
        ],
    );
    let s: Vec<[f32; 4]> = vec![
        [0.0, 0.0, 0.0, 0.0],
        [1.0, 0.0, 1.0, 0.0],
        [0.0, 1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0, 1.0],
    ];
    let sprite_vertex_array = VertexArray::new(
        gl::TRIANGLE_STRIP,
        s,
        vec![
            VertexAttribDefinition {
                number_of_components: 2,
                offset_of_first_element: 0,
            },
            VertexAttribDefinition {
                // uv
                number_of_components: 2,
                offset_of_first_element: 2,
            },
        ],
    );
    let s: Vec<[f32; 2]> = vec![[0.0, 1.0], [1.0, 1.0], [0.0, 0.0], [1.0, 0.0]];
    let rectangle_vertex_array = VertexArray::new(
        gl::TRIANGLE_STRIP,
        s,
        vec![VertexAttribDefinition {
            number_of_components: 2,
            offset_of_first_element: 0,
        }],
    );

    physics_world
        .mechanical_world
        .solver
        .set_contact_model(Box::new(SignoriniModel::new()));

    (
        MapRenderData {
            gat,
            gnd: ground_data.ground,
            rsw: world,
            ground_vertex_array: ground_data.ground_vertex_array,
            texture_atlas: ground_data.texture_atlas,
            tile_color_texture: ground_data.tile_color_texture,
            lightmap_texture: ground_data.lightmap_texture,
            model_instances,
            centered_sprite_vertex_array,
            sprite_vertex_array,
            rectangle_vertex_array,
            use_tile_colors: true,
            use_lightmaps: true,
            use_lighting: true,
            draw_models: true,
            draw_ground: true,
            ground_walkability_mesh: ground_data.ground_walkability_mesh,
            ground_walkability_mesh2: ground_data.ground_walkability_mesh2,
            ground_walkability_mesh3: ground_data.ground_walkability_mesh3,
        },
        physics_world,
    )
}

pub fn load_and_prepare_effect(
    name: &str,
    effect_id: StrEffectType,
    str_effects: &mut Vec<StrFile>,
    asset_loader: &AssetLoader,
    asset_database: &mut AssetDatabase,
    gl_render_sys: &mut OpenGlRenderSystem,
) {
    let str_file = asset_loader.load_effect(name, asset_database).unwrap();
    gl_render_sys.precache_effect(effect_id.into(), &str_file);
    str_effects.push(str_file);
}

pub struct PhysicEngine {
    mechanical_world: DefaultMechanicalWorld<f32>,
    geometrical_world: DefaultGeometricalWorld<f32>,

    bodies: DefaultBodySet<f32>,
    colliders: DefaultColliderSet<f32>,
    joint_constraints: DefaultJointConstraintSet<f32>,
    force_generators: DefaultForceGeneratorSet<f32>,
}

impl PhysicEngine {
    pub fn step(&mut self, dt: f32) {
        self.mechanical_world.set_timestep(dt);
        self.mechanical_world.step(
            &mut self.geometrical_world,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.joint_constraints,
            &mut self.force_generators,
        );
    }

    pub fn add_cuboid_skill(
        &mut self,
        pos: Vector2<f32>,
        rot_angle_in_rad: f32,
        extent: Vector2<f32>,
    ) -> DefaultColliderHandle {
        let cuboid = ShapeHandle::new(ncollide2d::shape::Cuboid::new(extent / 2.0));
        let body_handle = self.bodies.insert(
            RigidBodyDesc::new()
                .status(nphysics2d::object::BodyStatus::Static)
                .gravity_enabled(false)
                .build(),
        );
        return self.colliders.insert(
            ColliderDesc::new(cuboid)
                .translation(pos)
                .rotation(rot_angle_in_rad.to_degrees())
                .collision_groups(
                    CollisionGroups::new()
                        .with_membership(&[CollisionGroup::SkillArea as usize])
                        .with_blacklist(&[CollisionGroup::StaticModel as usize]),
                )
                .sensor(true)
                .build(BodyPartHandle(body_handle, 0)),
        );
    }
}

// so we can load effects dynamically
#[derive(Hash, Eq, PartialEq, Clone, Copy)]
pub struct StrEffectId(pub usize);

#[derive(Eq, Hash, PartialEq, Debug, Copy, Clone)]
pub enum StrEffectType {
    FireWall,
    StormGust,
    LordOfVermilion,
    Lightning,
    Concentration,
    Moonstar,
    Poison,
    Quagmire,
    FireWallBlue,
    FirePillarBomb,
    Ramadan,
}

impl From<StrEffectType> for StrEffectId {
    fn from(typ: StrEffectType) -> Self {
        StrEffectId(typ as usize)
    }
}
