use crate::ert::{ErtParticles, ErtRange};
use avian3d::prelude::*;
use bevy::prelude::*;
use rand::RngExt;
use std::collections::HashMap;

use super::{spawn_ert, ERT_LENGTH};

// Equations:
//
// Convergance Radius - The radius in which Flame erts turn into a flame and start accumulating
// temperature
//  c_r = 1.34log(n), where n is the number of erts in range
//
// Temperature - The temperature of the flame created
// t = summation(13/d^2), where d is the radius between flame erts within convergance radius
//
// Sporadic Motion - How flame Erts move with respect to their temperature, rand is re evaluated 1
// time a second
// s_v = (rand(0-1)t) / 577, where t is the temperature
//
// Explosion - Flame erts explode when their cores collide
// e_r = 1/3vt, where v is the total velocity between erts and t is the temperature
// e_i = 1/3v^r, where v is the total velocity between erts and r is the convergance radius

#[derive(Component)]
pub struct Flame {
    temperature: f32,
    sporadic_velocity: Vec3,
    /// Sim-clock time (seconds) at which the sporadic direction was last re-rolled.
    /// Starts at -inf so the first frame always rolls.
    last_reroll: f32,
}

impl Default for Flame {
    fn default() -> Self {
        Flame {
            temperature: 0.0,
            sporadic_velocity: Vec3::ZERO,
            last_reroll: f32::NEG_INFINITY,
        }
    }
}

const COUNT: usize = 2;
const CORE_RADIUS: f32 = ERT_LENGTH * 0.25;
const RANGE_RADIUS: f32 = ERT_LENGTH;
const CONVERGANCE_RADIUS_COEFFCIENT: f32 = 1.34;
const TEMPERATURE_COEFFCIENT: f32 = 13.0;

const SPORADIC_VELOCITY_COEFFCIENT: f32 = 1.0 / 57.0;
/// How often (seconds) each flame re-rolls its sporadic direction.
const SPORADIC_REROLL_SECS: f32 = 2.0;
/// Temperature at which a flame reaches its hottest visual state (fully white/bright/big).
const T_HOT: f32 = 20.0;

pub struct FlameErtPlugin;

impl Plugin for FlameErtPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn);
        // `flame_cosmetics` runs after `process_physics` so it sees the temperature
        // that `process_physics` just computed this frame.
        app.add_systems(Update, (process_physics, flame_cosmetics).chain());
    }
}

fn spawn(mut commands: Commands, asset_server: Res<AssetServer>) {
    let effect = asset_server.load("fire.ron");

    for i in 0..COUNT {
        let fi = (i + 1) as f32;
        let x = (fi * 10.9898).sin() * ERT_LENGTH * 1.0;
        let y = (fi * 38.233).sin() * ERT_LENGTH * 1.0;
        let z = (fi * 37.719).sin() * ERT_LENGTH * 1.0;
        spawn_ert(
            &mut commands,
            &effect,
            Vec3::new(x, y, z),
            CORE_RADIUS,
            RANGE_RADIUS,
            Flame::default(),
        );
    }
}

fn process_physics(
    positions: Query<(Entity, &Transform), With<Flame>>,
    mut sensors: Query<(&ChildOf, &CollidingEntities, &mut Collider), With<ErtRange>>,
    mut bodies: Query<(Entity, &mut LinearVelocity, &mut Flame)>,
    time: Res<Time>,
) {
    // Snapshot every ert core's position, keyed by entity.
    let pos: HashMap<Entity, Vec3> = positions.iter().map(|(e, t)| (e, t.translation)).collect();
    let mut temperatures: HashMap<Entity, f32> = HashMap::new();

    // Temperature = sum of 13/d^2 over the cores this ert's sensor currently detects.
    for (child_of, colliding, mut collider) in &mut sensors {
        let me_entity = child_of.parent();
        let Some(&me) = pos.get(&me_entity) else {
            continue;
        };

        for &core in colliding.iter() {
            if let Some(&other) = pos.get(&core) {
                let delta = other - me;
                *temperatures.entry(me_entity).or_insert(0.0) +=
                    TEMPERATURE_COEFFCIENT / delta.length_squared();
            }
        }

        // n is a natural number, and radius counting always counts self, so if colllding
        // count is 0, the ert is always in range of itself so 0+1. therefore log10 can
        // never be fed 0.

        // Only count flame erts by filtering by the flame ert list
        let n = colliding.iter().filter(|e| pos.contains_key(e)).count() + 1;
        let scale: Vec3 =
            Vec3::new(1.0, 1.0, 1.0) * (CONVERGANCE_RADIUS_COEFFCIENT * (n as f32).log10() + 1.0);
        collider.set_scale(scale, 1);
    }

    let now = time.elapsed_secs();
    let mut rng = rand::rng();
    for (entity, mut velocity, mut flame) in &mut bodies {
        // Temperature is instantaneous: 0 when no neighbours are in range this frame.
        flame.temperature = temperatures.get(&entity).copied().unwrap_or(0.0);

        // Re-roll the sporadic direction once per interval, in a fresh random direction.
        if now - flame.last_reroll >= SPORADIC_REROLL_SECS {
            flame.sporadic_velocity = get_sv_slice(flame.temperature, &mut rng);
            flame.last_reroll = now;
        }
        velocity.0 += flame.sporadic_velocity * time.delta_secs();
    }
}

/// All per-frame flame COSMETICS in one place, driven by each flame's temperature
/// (computed by `process_physics`):
/// 1. the per-emitter particle overrides (the "fire" / "smoke" look), and
/// 2. emitter on/off — emission stops when a flame goes cold (`temperature <= 0`, no
///    neighbours in range) and resumes when it heats back up.
///
/// The stop is graceful: emission simply pauses, so particles already alive finish
/// their lifetime and fade out rather than popping away. (For an instant clear
/// instead, use `EmitterRuntime::stop(None)`.)
fn flame_cosmetics(
    flames: Query<&Flame>,
    mut emitter_overrides: Query<
        (
            Entity,
            &ChildOf,
            &mut bevy_sprinkles::prelude::ParticleEmitterOverrides,
        ),
        With<ErtParticles>,
    >,
    mut emitters: Query<(
        &bevy_sprinkles::prelude::EmitterEntity,
        &mut bevy_sprinkles::prelude::EmitterRuntime,
    )>,
) {
    // The emitter now lives on a scaled child, so bridge each parent flame's temperature
    // to its child emitter. Key the temperature by the emitter entity itself — that's
    // what `EmitterEntity.parent_system` points at — so the on/off pass can look it up.
    let mut emitter_temp: HashMap<Entity, f32> = HashMap::new();
    for (emitter_entity, child_of, mut ovr) in &mut emitter_overrides {
        let Ok(flame) = flames.get(child_of.parent()) else {
            continue; // not a flame ert (e.g. glacial) — leave its emitter untouched
        };
        emitter_temp.insert(emitter_entity, flame.temperature);
        ovr.0.clear();
        ovr.0
            .insert("fire".to_string(), fire_override(flame.temperature));
    }

    // Emitter on/off, driven by each flame emitter's temperature. Only our flame
    // emitters are in `emitter_temp`, so any other particle system is left alone.
    for (emitter, mut runtime) in &mut emitters {
        let Some(&temp) = emitter_temp.get(&emitter.parent_system) else {
            continue;
        };
        let should_emit = temp > 0.0;
        // Only mutate on a transition, so we don't mark EmitterRuntime changed every frame.
        if runtime.is_emitting() != should_emit {
            runtime.set_emitting(should_emit);
        }
    }
}

/// A random unit direction, scaled so the resulting speed is `temperature / 577`
/// (the Sporadic Motion equation above).
fn get_sv_slice(temperature: f32, rng: &mut impl RngExt) -> Vec3 {
    let dir = Vec3::new(
        rng.random_range(-1.0..=1.0),
        rng.random_range(-1.0..=1.0),
        rng.random_range(-1.0..=1.0),
    );

    // normalize_or_zero avoids NaN in the (astronomically unlikely) all-zero draw.
    dir.normalize_or_zero() * temperature * SPORADIC_VELOCITY_COEFFCIENT
}

/// Maps an instantaneous flame temperature to the "Fire" emitter's per-instance
/// particle look: the hot core — grows, whitens, glows brighter, flares faster
/// with heat. Saturates at `T_HOT` so hotter-than-max flames don't produce
/// runaway values.
fn fire_override(temperature: f32) -> bevy_sprinkles::prelude::ParticleOverride {
    use bevy::prelude::LinearRgba;
    use bevy_sprinkles::prelude::ParticleOverride;

    let heat = (temperature / T_HOT).clamp(0.0, 1.0);
    let dim_red = LinearRgba::new(1.0, 0.25, 0.05, 1.0);
    let white = LinearRgba::new(1.0, 1.0, 1.0, 1.0);
    let tint = LinearRgba::new(
        dim_red.red + (white.red - dim_red.red) * heat,
        dim_red.green + (white.green - dim_red.green) * heat,
        dim_red.blue + (white.blue - dim_red.blue) * heat,
        1.0,
    );

    ParticleOverride {
        tint: Some(tint),
        // Sprite size is world-absolute in bevy_sprinkles' world mode (the emitter's
        // transform scale doesn't reach it), so fold in ERT_LENGTH here to keep the
        // hot core sized in ert-units alongside the rest of the effect.
        size_mul: Some((0.5 + 1.0 * heat) * ERT_LENGTH),
        // emissive scales from a dim ember to a bright hot core (HDR values drive bloom).
        emissive: Some(LinearRgba::new(
            0.4 + 5.0 * heat,
            0.05 + 2.0 * heat,
            0.02 + 0.5 * heat,
            1.0,
        )),
        speed_mul: Some(1.0 + 1.0 * heat),
        ..Default::default()
    }
}
