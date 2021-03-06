use crate::components::char::CharacterStateComponent;
use crate::systems::{Collision, CollisionsFromPrevFrame, SystemFrameDurations};
use crate::PhysicEngine;
use nalgebra::Vector2;
use ncollide2d::narrow_phase::ContactEvent;
use ncollide2d::query::Proximity;
use rustarok_common::common::EngineTime;
use specs::prelude::*;

pub struct FrictionSystem;

impl<'a> System<'a> for FrictionSystem {
    type SystemData = (
        WriteExpect<'a, PhysicEngine>,
        WriteExpect<'a, SystemFrameDurations>,
        ReadExpect<'a, EngineTime>,
        WriteStorage<'a, CharacterStateComponent>,
    );

    fn run(
        &mut self,
        (mut physics_world, mut system_benchmark, time, char_storage): Self::SystemData,
    ) {
        // TODO2 ??
        //        let _stopwatch = system_benchmark.start_measurement("FrictionSystem");
        //        for char_state in (&mut char_storage).join() {
        //            let body = physics_world.bodies.rigid_body_mut(char_state.body_handle);
        //            if let Some(body) = body {
        //                if char_state
        //                    .cannot_control_until
        //                    .has_already_passed(time.now())
        //                {
        //                    body.set_linear_velocity(Vector2::zeros());
        //                } else {
        //                    // damping seems unpredictable so I use this to stop arrived players
        //                    let linear = body.velocity().linear;
        //                    if linear.x != 0.0 || linear.y != 0.0 {
        //                        let dir = linear.normalize();
        //                        let slowing_vector = body.velocity().linear - (dir * 1.0);
        //                        let len = slowing_vector.magnitude();
        //                        if len <= 0.001 {
        //                            body.set_linear_velocity(Vector2::zeros());
        //                        } else {
        //                            body.set_linear_velocity(slowing_vector);
        //                        }
        //                    }
        //                }
        //                let body_pos = body.position().translation.vector;
        //                char_state.set_pos_dont_use_it(body_pos);
        //            }
        //        }
    }
}

pub struct PhysCollisionCollectorSystem;

impl<'a> System<'a> for PhysCollisionCollectorSystem {
    type SystemData = (
        Entities<'a>,
        WriteExpect<'a, PhysicEngine>,
        WriteExpect<'a, SystemFrameDurations>,
        ReadExpect<'a, EngineTime>,
        WriteExpect<'a, CollisionsFromPrevFrame>,
    );

    fn run(
        &mut self,
        (
            _entities,
            mut physics_world,
            mut system_benchmark,
            time,
            mut collisions_resource,
        ): Self::SystemData,
    ) {
        let _stopwatch = system_benchmark.start_measurement("PhysicsSystem");

        physics_world.step(time.dt());

        for event in physics_world.geometrical_world.proximity_events() {
            let collider1 = physics_world.colliders.get(event.collider1).unwrap();
            let (character_coll_handle, other_coll_handle) = if collider1.user_data().is_some() {
                (event.collider1, event.collider2)
            } else {
                (event.collider2, event.collider1)
            };
            let collision = Collision {
                character_coll_handle,
                other_coll_handle,
            };
            match event.new_status {
                Proximity::Intersecting => {
                    collisions_resource
                        .collisions
                        .insert((event.collider1, event.collider2), collision);
                    dbg!(&collisions_resource.collisions);
                }
                Proximity::WithinMargin => {
                    if event.prev_status == Proximity::Intersecting {
                        collisions_resource
                            .collisions
                            .remove(&(event.collider1, event.collider2));
                        dbg!(&collisions_resource.collisions);
                    }
                }
                Proximity::Disjoint => {
                    collisions_resource
                        .collisions
                        .remove(&(event.collider1, event.collider2));
                    dbg!(&collisions_resource.collisions);
                }
            }
        }

        for event in physics_world.geometrical_world.contact_events() {
            match event {
                ContactEvent::Started(handle1, handle2) => {
                    let collider1 = physics_world.colliders.get(*handle1).unwrap();
                    let (character_coll_handle, other_coll_handle) =
                        if collider1.user_data().is_some() {
                            (handle1, handle2)
                        } else {
                            (handle2, handle1)
                        };
                    let collision = Collision {
                        character_coll_handle: *character_coll_handle,
                        other_coll_handle: *other_coll_handle,
                    };
                    collisions_resource
                        .collisions
                        .insert((*handle1, *handle2), collision);
                }
                ContactEvent::Stopped(handle1, handle2) => {
                    let collider1 = physics_world.colliders.get(*handle1).unwrap();
                    let collider1_body = physics_world.bodies.get(collider1.body()).unwrap();
                    let (character_coll_handle, other_coll_handle) = if collider1_body.is_ground() {
                        (handle2, handle1)
                    } else {
                        (handle1, handle2)
                    };
                    let _collision = Collision {
                        character_coll_handle: *character_coll_handle,
                        other_coll_handle: *other_coll_handle,
                    };
                    collisions_resource.collisions.remove(&(*handle1, *handle2));
                }
            }
        }
    }
}
