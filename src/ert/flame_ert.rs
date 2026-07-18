use crate::ert::{ErtParticles, ErtRange};
use avian3d::prelude::*;
use bevy::{math::FloatPow, prelude::*};
use bevy_sprinkles::prelude::{ParticleEmitterOverrides, Particles3d};
use rand::RngExt;
use std::collections::{HashMap, HashSet};

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
    /// `c_r` from the equations above, recomputed each frame by `process_physics` and
    /// kept because the explosion equations need it as an exponent.
    convergance_radius: f32,
    sporadic_velocity: Vec3,
    /// Sim-clock time (seconds) at which the sporadic direction was last re-rolled.
    /// Starts at -inf so the first frame always rolls.
    last_reroll: f32,
}

impl Default for Flame {
    fn default() -> Self {
        Flame {
            temperature: 0.0,
            convergance_radius: 1.0,
            sporadic_velocity: Vec3::ZERO,
            last_reroll: f32::NEG_INFINITY,
        }
    }
}

const CORE_RADIUS: f32 = ERT_LENGTH * 0.25;
const RANGE_RADIUS: f32 = ERT_LENGTH;
const CONVERGANCE_RADIUS_COEFFCIENT: f32 = 3.34;
const TEMPERATURE_COEFFCIENT: f32 = 0.8;

/// Floor on the distance between two cores when accumulating temperature. Two cores
/// cannot be closer than their combined radii without overlapping, and without this floor
/// a coincident pair divides by ~0 and gains unbounded heat.
///
/// This is the ceiling on what a SINGLE neighbour can contribute
/// (`TEMPERATURE_COEFFCIENT / MIN_TEMPERATURE_DISTANCE^2`), so `T_EXPLODE` is defined in
/// terms of it — see the invariant asserted below.
const MIN_TEMPERATURE_DISTANCE: f32 = CORE_RADIUS * 2.0;

const SPORADIC_VELOCITY_COEFFCIENT: f32 = (ERT_LENGTH * ERT_LENGTH) / 57.0;
/// How often (seconds) each flame re-rolls its sporadic direction.
const SPORADIC_REROLL_SECS: f32 = 0.5;
/// Coldest temperature that still shows a flame — roughly one neighbour sitting out at
/// the edge of range, `TEMPERATURE_COEFFCIENT / 0.5^2`. Below this, `heat` is 0 (soft
/// orange). Derived from the coefficient rather than hardcoded, so retuning the physics
/// constant can't silently strand the whole colour ramp at one end.
const T_COLD: f32 = TEMPERATURE_COEFFCIENT / (0.5 * 0.5);
/// Temperature at which a flame reaches its hottest visual state (furious red): a pair
/// packed down to `d ≈ 0.057`, i.e. cores nearly coincident.
///
/// Temperature is a sum of `TEMPERATURE_COEFFCIENT / d^2` terms, so it spans orders of
/// magnitude as erts pack together. The cold→hot ramp is therefore normalised in LOG
/// space — a linear map pins `heat` at 1.0 the moment any two erts get close, which is
/// why every flame used to render at the fully-saturated end of the gradient.
const T_HOT: f32 = TEMPERATURE_COEFFCIENT / (0.057 * 0.057);
/// Ceiling applied to temperature when it drives sporadic MOTION. Separate from the
/// visual `T_HOT` on purpose — this one bounds physics (and stops `inf` reaching
/// `LinearVelocity`), so it should only change when you want erts to move differently.
const T_SPORADIC_MAX: f32 = 200.0;
/// Temperature at which a flame ert becomes explosive, defined as the configuration
/// "two cores in contact, plus a third ert out at the edge of range":
///
///   `coeff / MIN_TEMPERATURE_DISTANCE^2`  — the ert it is touching, at its closest
/// + `coeff / RANGE_RADIUS^2`              — the third ert, at the far edge of range
///
/// The first term is deliberately the SAME distance the temperature clamp floors at.
/// That is what makes two erts alone unable to detonate no matter how they are stacked:
/// a single neighbour tops out at exactly the first term, so the second term is the
/// margin that a third ert has to supply. Writing that first distance as anything larger
/// than the clamp floor drops the threshold below what one neighbour can already produce,
/// and a coincident pair detonates on its own.
const T_EXPLODE: f32 = TEMPERATURE_COEFFCIENT
    * (1.0 / (MIN_TEMPERATURE_DISTANCE * MIN_TEMPERATURE_DISTANCE)
        + 1.0 / (RANGE_RADIUS * RANGE_RADIUS));

/// Enforces the invariant above at compile time: the hottest a lone pair can get must
/// stay strictly under the explosion threshold, or two erts detonate by themselves.
const _: () = assert!(
    T_EXPLODE > TEMPERATURE_COEFFCIENT / (MIN_TEMPERATURE_DISTANCE * MIN_TEMPERATURE_DISTANCE)
);
/// Minimum closing speed for a detonation. Temperature says how hot a pair is, not how
/// hard they met, so without this two hot erts drifting into contact still explode.
///
/// Scaled to the sporadic drift an ert generates at exactly `T_EXPLODE`
/// (`s_v = t * SPORADIC_VELOCITY_COEFFCIENT`), so it means "closing faster than a
/// just-explosive ert wanders on its own" and tracks the motion constants if they are
/// retuned. Replace with a literal if you want it decoupled.
const MIN_EXPLOSION_VELOCITY: f32 = T_EXPLODE * SPORADIC_VELOCITY_COEFFCIENT * 3.0;

/// The `1/3` from the explosion equations, pulled out so blast size and blast force can
/// be tuned independently of each other. Left at `1/3` each, they reproduce the equations
/// as written; raise `RADIUS` for a wider blast, `INTENSITY` for a harder shove.
///
/// `INTENSITY` scales `v^r`, which already grows steeply — treat it as a fine adjustment
/// and expect `MAX_EXPLOSION_INTENSITY` to start clamping sooner as you raise it.
const EXPLOSION_RADIUS_COEFFCIENT: f32 = 1.0 / 100.0;
const EXPLOSION_INTENSITY_COEFFCIENT: f32 = 1.0 / 100.0;

/// Ceilings on the explosion equations. `e_r = 1/3vt` and `e_i = 1/3v^r` are both
/// unbounded — `e_i` especially, since `r` is an exponent — so a hot fast pile-up can
/// produce arbitrarily large numbers. These bound the blast to something the sim can
/// survive without needing the equations themselves changed.
const MAX_EXPLOSION_RADIUS: f32 = ERT_LENGTH * 20.0;
const MAX_EXPLOSION_INTENSITY: f32 = 50.0;
/// How long a spawned explosion effect entity lives. `explosion.ron`'s emitters are all
/// `one_shot`, and its longest runs `delay 0.3 + lifetime 0.6`, so the visual is done at
/// ~0.9s — but nothing owns the entity afterwards, so without this they accumulate for
/// the rest of the session. The margin lets the last particles finish fading.
const EXPLOSION_EFFECT_SECS: f32 = 1.2;

/// Marker + self-destruct timer on a spawned explosion effect.
#[derive(Component)]
struct ExplosionEffect(Timer);

/// One detonation, resolved from a core-on-core hit. Carries everything the visuals need
/// so the effect can be dressed from the same numbers that drove the physics.
struct Blast {
    center: Vec3,
    radius: f32,
    intensity: f32,
    temperature: f32,
}

/// A flame ert's state at the instant `explode` runs, snapshotted so the blast search can
/// read every ert while the impulse pass writes them.
struct ErtState {
    position: Vec3,
    velocity: Vec3,
    temperature: f32,
    convergance_radius: f32,
}

/// Applies the explosion equations, or `None` if they produce nothing usable.
///
///   e_r = EXPLOSION_RADIUS_COEFFCIENT * v * t     — blast radius
///   e_i = EXPLOSION_INTENSITY_COEFFCIENT * v^r    — blast intensity
///
/// `v^r` raises a possibly-greater-than-one velocity to a power that grows with
/// neighbour count, so it diverges fast; the clamps keep a dense pile-up from throwing
/// every other ert into the NaN territory this file has already fought once.
fn resolve_blast(center: Vec3, v: f32, t: f32, r: f32) -> Option<Blast> {
    let e_r = EXPLOSION_RADIUS_COEFFCIENT * v * t;
    let e_i = EXPLOSION_INTENSITY_COEFFCIENT * v.powf(r);

    if !e_r.is_finite() || !e_i.is_finite() || e_r <= 0.0 {
        return None;
    }

    Some(Blast {
        center,
        radius: e_r.min(MAX_EXPLOSION_RADIUS),
        intensity: e_i.min(MAX_EXPLOSION_INTENSITY),
        temperature: t,
    })
}

pub struct FlameErtPlugin;

impl Plugin for FlameErtPlugin {
    fn build(&self, app: &mut App) {
        // Chained: `explode` needs the temperature and convergance radius that
        // `process_physics` computes, and `flame_cosmetics` should not bother dressing
        // erts that `explode` just consumed.
        app.add_systems(Update, (process_physics, explode, flame_cosmetics).chain());
        // Unordered: it only ever touches effect entities, which nothing above reads.
        app.add_systems(Update, despawn_explosion_effects);
    }
}

pub fn spawn(position: Vec3, mut commands: Commands, asset_server: Res<AssetServer>) {
    let effect = asset_server.load("fire.ron");

    spawn_ert(
        &mut commands,
        &effect,
        position,
        CORE_RADIUS,
        RANGE_RADIUS,
        Flame::default(),
    );
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
    let mut convergance_radii: HashMap<Entity, f32> = HashMap::new();

    // Temperature = sum of 13/d^2 over the cores this ert's sensor currently detects.
    for (child_of, colliding, mut collider) in &mut sensors {
        let me_entity = child_of.parent();
        let Some(&me) = pos.get(&me_entity) else {
            continue;
        };

        for &core in colliding.iter() {
            if let Some(&other) = pos.get(&core) {
                let delta = other - me;
                *temperatures.entry(me_entity).or_insert(0.0) += TEMPERATURE_COEFFCIENT
                    / delta
                        .length()
                        .clamp(MIN_TEMPERATURE_DISTANCE, 100.0)
                        .squared();
            }
        }

        // n is a natural number, and radius counting always counts self, so if colllding
        // count is 0, the ert is always in range of itself so 0+1. therefore log10 can
        // never be fed 0.

        // Only count flame erts by filtering by the flame ert list
        let n = colliding.iter().filter(|e| pos.contains_key(e)).count() + 1;
        let convergance_radius = CONVERGANCE_RADIUS_COEFFCIENT * (n as f32).log10() + 1.0;
        convergance_radii.insert(me_entity, convergance_radius);
        let scale: Vec3 = Vec3::new(1.0, 1.0, 1.0) * convergance_radius;
        collider.set_scale(scale, 1);
    }

    let now = time.elapsed_secs();
    let mut rng = rand::rng();
    for (entity, mut velocity, mut flame) in &mut bodies {
        // Temperature is instantaneous: 0 when no neighbours are in range this frame.
        flame.temperature = temperatures.get(&entity).copied().unwrap_or(0.0);
        flame.convergance_radius = convergance_radii.get(&entity).copied().unwrap_or(1.0);

        // Re-roll the sporadic direction once per interval, in a fresh random direction.
        if now - flame.last_reroll >= SPORADIC_REROLL_SECS {
            flame.sporadic_velocity = get_sv_slice(flame.temperature, &mut rng);
            flame.last_reroll = now;
        }
        velocity.0 += flame.sporadic_velocity * time.delta_secs();
    }
}

/// Explosion, per the equations at the top of this file: flame erts explode when their
/// cores collide.
///
///   e_r = 1/3 * v * t     — blast radius
///   e_i = 1/3 * v^r       — blast intensity
///
/// where `v` is the total velocity between the two erts, `t` the temperature, and `r`
/// the convergance radius. The two erts that collided are consumed; every other ert
/// inside `e_r` is thrown outward by `e_i`, falling off linearly with distance.
fn explode(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut flames: Query<(
        Entity,
        &Transform,
        &mut LinearVelocity,
        &Flame,
        &CollidingEntities,
    )>,
) {
    // Snapshot first: finding the blasts needs to read every ert, applying them needs to
    // write every ert, and the two can't overlap on one query.
    let snapshot: HashMap<Entity, ErtState> = flames
        .iter()
        .map(|(e, transform, velocity, flame, _)| {
            (
                e,
                ErtState {
                    position: transform.translation,
                    velocity: velocity.0,
                    temperature: flame.temperature,
                    convergance_radius: flame.convergance_radius,
                },
            )
        })
        .collect();

    let mut blasts: Vec<Blast> = Vec::new();
    let mut consumed: HashSet<Entity> = HashSet::new();
    // Detonations waiting to be resolved. Seeded by core-on-core hits, then grown by the
    // chain reaction below.
    let mut pending: Vec<Blast> = Vec::new();

    for (entity, _, _, _, colliding) in flames.iter() {
        for &other in colliding.iter() {
            // A core's contacts include range sensors and glacial cores; only flame
            // cores are in the snapshot. Each pair also surfaces twice (A sees B, B sees
            // A), so the ordering test keeps exactly one of them.
            if other <= entity {
                continue;
            }
            let (Some(a), Some(b)) = (snapshot.get(&entity), snapshot.get(&other)) else {
                continue;
            };

            let t = a.temperature.max(b.temperature);
            let v = (a.velocity - b.velocity).length();
            // Cold erts just bump into each other, and so do slow ones: a detonation
            // needs both the heat AND the closing speed. Without the velocity gate two
            // hot erts drifting together at a crawl still detonate, because `t` alone
            // says nothing about how hard they met.
            if t < T_EXPLODE || v < MIN_EXPLOSION_VELOCITY {
                continue;
            }

            let Some(blast) = resolve_blast(
                (a.position + b.position) * 0.5,
                v,
                t,
                a.convergance_radius.max(b.convergance_radius),
            ) else {
                continue;
            };

            pending.push(blast);
            consumed.insert(entity);
            consumed.insert(other);
        }
    }

    // Chain reaction: a blast detonates any ert inside it that is itself above the
    // threshold, and those detonations can reach further erts in turn. This terminates
    // because `consumed` only grows and an ert can detonate at most once.
    while let Some(blast) = pending.pop() {
        for (&entity, state) in &snapshot {
            if consumed.contains(&entity) || state.temperature < T_EXPLODE {
                continue;
            }
            // A chained ert has no partner to be "the total velocity between erts", so
            // its own speed stands in — and it faces the same velocity gate as a seed.
            // A hot but near-stationary ert therefore rides out the blast and is merely
            // thrown by it, rather than being quietly consumed for a detonation that
            // `resolve_blast` would have rejected as zero-radius anyway.
            let v = state.velocity.length();
            if v * 2.5 < MIN_EXPLOSION_VELOCITY {
                continue;
            }
            if state.position.distance(blast.center) >= blast.radius {
                continue;
            }
            consumed.insert(entity);
            if let Some(chained) = resolve_blast(
                state.position,
                v,
                state.temperature,
                state.convergance_radius,
            ) {
                pending.push(chained);
            }
        }
        blasts.push(blast);
    }

    if blasts.is_empty() {
        return;
    }

    for (entity, transform, mut velocity, _, _) in flames.iter_mut() {
        if consumed.contains(&entity) {
            continue; // about to be despawned, so pushing it is wasted work
        }
        for blast in &blasts {
            let delta = transform.translation - blast.center;
            let distance = delta.length();
            if distance >= blast.radius || distance <= f32::EPSILON {
                continue;
            }
            velocity.0 += (delta / distance) * blast.intensity * (1.0 - distance / blast.radius);
        }
    }

    for entity in consumed {
        commands.entity(entity).despawn();
    }

    // One effect per blast, sized by the blast itself so a big detonation reads big.
    // `AssetServer::load` is cached by path, so calling it here rather than preloading
    // costs a hash lookup and keeps this consistent with `spawn` above.
    let effect = asset_server.load("explosion.ron");
    for blast in &blasts {
        commands.spawn((
            ExplosionEffect(Timer::from_seconds(EXPLOSION_EFFECT_SECS, TimerMode::Once)),
            Particles3d(effect.clone()),
            explosion_overrides(blast.temperature, blast.intensity),
            // Floored at ERT_LENGTH: a low-energy pair can produce an `e_r` small enough
            // that the effect would be invisible, which reads as the explosion silently
            // failing rather than as a small explosion.
            Transform::from_translation(blast.center)
                .with_scale(Vec3::splat(blast.radius.max(ERT_LENGTH))),
            // Particle children need this to inherit visibility (same reason the ert body
            // carries one).
            Visibility::default(),
        ));
    }
}

/// Explosion effects own nothing else, so they self-destruct once their emitters have
/// finished; see `EXPLOSION_EFFECT_SECS`.
fn despawn_explosion_effects(
    mut commands: Commands,
    time: Res<Time>,
    mut effects: Query<(Entity, &mut ExplosionEffect)>,
) {
    for (entity, mut effect) in &mut effects {
        if effect.0.tick(time.delta()).is_finished() {
            commands.entity(entity).despawn();
        }
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
    // The clamp is what keeps an infinite temperature (two cores at d = 0) from ever
    // reaching LinearVelocity. It uses its OWN ceiling rather than the visual `T_HOT`,
    // so retuning the look never silently retunes how fast erts fly apart.
    dir.normalize_or_zero() * temperature.clamp(0.0, T_SPORADIC_MAX) * SPORADIC_VELOCITY_COEFFCIENT
}

/// Normalised 0..1 heat for COSMETICS, in log space between `T_COLD` and `T_HOT`.
/// Temperature spans orders of magnitude, so a linear map would spend virtually all
/// its range pinned at 1.0.
fn heat_of(temperature: f32) -> f32 {
    if temperature <= T_COLD {
        return 0.0;
    }
    ((temperature / T_COLD).log10() / (T_HOT / T_COLD).log10()).clamp(0.0, 1.0)
}

/// Colour-over-lifetime stops, in **sRGB, clamped to 0..1**: `color_keys` is baked
/// straight into an `Rgba8UnormSrgb` texture with no linear conversion, so values above
/// 1.0 are simply cut off. Each pair is birth → death of a single particle.
///
/// These are deliberately DIM, because the emitter blends additively (`alpha_mode: Add`)
/// — what you see is the SUM of every overlapping quad, not one particle's colour. The
/// flame's core is ~10 quads deep, so these are authored at roughly a tenth of the
/// intended on-screen colour and reach it by stacking. Authoring them at full strength
/// is what made the centre clip to white on all three channels while only the 1-2 quad
/// deep rim kept any hue.
///
/// Green and blue are held especially low: red is *meant* to clip in the core, but the
/// moment green and blue clip alongside it the flame goes white regardless of hue.
const COLD_BIRTH: [f32; 4] = [0.115, 0.050, 0.018, 1.0];
const COLD_DEATH: [f32; 4] = [0.070, 0.023, 0.007, 1.0];
const HOT_BIRTH: [f32; 4] = [0.130, 0.025, 0.007, 1.0];
const HOT_DEATH: [f32; 4] = [0.060, 0.007, 0.002, 1.0];

/// Brightness endpoints, ramped by `heat`. The stops above carry the HUE; these carry
/// how hard it adds — so a cold flame stays a dim ember and only a hot one burns bright.
///
/// Raise them if flames read too dim, lower them if the dense core washes back toward
/// white — hue is preserved either way, since every channel scales together. Green and
/// blue sit low enough in the stops that red saturates well before they do, which is
/// what keeps a bright core red rather than white.
const INTENSITY_COLD: f32 = 1.5;
const INTENSITY_HOT: f32 = 4.2;

/// Quantisation applied to `heat` before it reaches the gradient. `color_keys` re-bakes
/// a 256px texture whenever the stops hash differently, so a continuously-jittering
/// temperature would rebake every ert every frame. 32 steps is far finer than the eye
/// can resolve on a flame this size.
const HEAT_STEPS: f32 = 32.0;

fn lerp_stop(a: [f32; 4], b: [f32; 4], t: f32, intensity: f32) -> [f32; 4] {
    [
        (a[0] + (b[0] - a[0]) * t) * intensity,
        (a[1] + (b[1] - a[1]) * t) * intensity,
        (a[2] + (b[2] - a[2]) * t) * intensity,
        // Alpha is deliberately NOT scaled: it gates how much of each quad reaches the
        // additive blend, so scaling it would change coverage rather than brightness.
        a[3] + (b[3] - a[3]) * t,
    ]
}

/// The heat-driven colour-over-lifetime gradient, shared by the steady flame and by the
/// explosion's additive "Fire" emitter so a detonation looks like the flames that caused
/// it. Both callers blend additively, so both want the same dim, low-green stops.
fn heat_color_keys(heat: f32) -> bevy_sprinkles::prelude::ParticleGradient {
    use bevy_sprinkles::prelude::{GradientInterpolation, GradientStop, ParticleGradient};

    let gradient_heat = (heat * HEAT_STEPS).round() / HEAT_STEPS;
    // Intensity rides the QUANTISED heat, same as the stops it scales — using raw `heat`
    // here would change the baked stop values every frame and defeat `HEAT_STEPS`.
    let intensity = INTENSITY_COLD + (INTENSITY_HOT - INTENSITY_COLD) * gradient_heat;

    ParticleGradient {
        stops: vec![
            GradientStop {
                color: lerp_stop(COLD_BIRTH, HOT_BIRTH, gradient_heat, intensity),
                position: 0.0,
            },
            GradientStop {
                color: lerp_stop(COLD_DEATH, HOT_DEATH, gradient_heat, intensity),
                position: 1.0,
            },
        ],
        interpolation: GradientInterpolation::Linear,
    }
}

/// Per-emitter overrides for `explosion.ron`, driven by the blast's temperature (colour)
/// and intensity (scale, speed, duration).
///
/// The keys must match the asset's emitter names — "Windup", "Fire", "Shockwave". A key
/// that doesn't match simply leaves that emitter rendering its authored values, so a
/// typo here fails silently rather than loudly.
///
/// Note the two blend modes are calibrated differently: "Fire" is `alpha_mode: Add`, so
/// it uses the same dim stacking-aware gradient as the flames, while "Windup" and
/// "Shockwave" are `Blend` and don't accumulate — they can carry full-strength tints.
fn explosion_overrides(temperature: f32, intensity: f32) -> ParticleEmitterOverrides {
    use bevy::prelude::LinearRgba;
    use bevy_sprinkles::prelude::ParticleOverride;

    let heat = heat_of(temperature);
    let force = (intensity / MAX_EXPLOSION_INTENSITY).clamp(0.0, 1.0);

    // Full-strength tints for the non-additive emitters: soft orange → furious red.
    let tint = LinearRgba::new(
        1.0,
        0.55 + (0.20 - 0.55) * heat,
        0.22 + (0.06 - 0.22) * heat,
        1.0,
    );

    let mut map = HashMap::new();

    // The fireball. `size_mul` stays near 1.0 on purpose: the whole effect entity is
    // already scaled by the blast radius, so scaling per-particle size by force again
    // would compound into something far larger than `e_r`.
    map.insert(
        "Fire".to_string(),
        ParticleOverride {
            color_keys: Some(heat_color_keys(heat)),
            emissive: Some(LinearRgba::new(
                0.60 + 3.00 * heat,
                0.16 - 0.10 * heat,
                0.06 - 0.04 * heat,
                1.0,
            )),
            size_mul: Some(0.8 + 0.4 * force),
            speed_mul: Some(1.0 + 1.5 * force),
            lifetime_mul: Some(0.8 + 0.6 * force),
            ..Default::default()
        },
    );

    ParticleEmitterOverrides(map)
}

/// Maps an instantaneous flame temperature to the "fire" emitter's per-instance
/// particle look: soft orange when cold, ramping to a furious red as it heats —
/// growing and flaring faster along the way.
///
/// The colour is authored by REPLACING the emitter's colour-over-lifetime gradient
/// (`color_keys`) rather than by multiplying `tint` over the asset's amber gradient.
/// Tinting could only ever scale what the asset already had, and with 36 additively
/// blended particles stacking up that reads as "brighter", not "redder".
fn fire_override(temperature: f32) -> bevy_sprinkles::prelude::ParticleOverride {
    use bevy::prelude::LinearRgba;
    use bevy_sprinkles::prelude::ParticleOverride;

    let heat = heat_of(temperature);
    let color_keys = heat_color_keys(heat);

    ParticleOverride {
        // No `tint`: the gradient above already IS the colour, so leaving tint at
        // identity keeps one source of truth instead of compounding two.
        color_keys: Some(color_keys),
        // Sprite size is world-absolute in bevy_sprinkles' world mode (the emitter's
        // transform scale doesn't reach it), so fold in ERT_LENGTH here to keep the
        // hot core sized in ert-units alongside the rest of the effect.
        size_mul: Some((0.1 + 4.0 * heat) * ERT_LENGTH),
        // Emissive stacks additively per overlapping quad too, so it is kept small for
        // the same reason the gradient is: the old `9.0` red / `0.9` green drove the
        // ~10-deep core to a summed ~90 / ~9, clipping every channel to white. These
        // values still bloom once stacked, but only red is allowed to run away.
        emissive: Some(LinearRgba::new(
            0.60 + 2.40 * heat,
            0.16 - 0.10 * heat,
            0.06 - 0.04 * heat,
            1.0,
        )),
        speed_mul: Some(1.0 + 1.0 * heat),
        // A hot flame throws longer-lived particles, so it reads as taller rather than
        // just brighter. Spawn-time, so particles already in flight keep the lifetime
        // they were born with and nothing pops when temperature swings.
        //
        // NOTE: concurrent particles ≈ spawn rate × lifetime, so this also deepens the
        // additive stack — at full heat the core is ~1.5x more quads deep than at rest.
        // That compounds with `INTENSITY_HOT`: both scale brightness with heat, so the
        // hot end climbs faster than either constant suggests on its own. If the hot
        // core washes toward white, lower `INTENSITY_HOT` rather than the stops.
        lifetime_mul: Some(0.65 + 0.85 * heat),
        ..Default::default()
    }
}
