use crate::ert::ErtRange;
use avian3d::prelude::*;
use bevy::prelude::*;
use rand::RngExt;
use std::collections::HashMap;

use super::{spawn_ert, ErtStats, ERT_LENGTH};

// Equations:
//
// Convergance Radius - The radius in which Flame erts turn into a flame and start accumulating
// temperature
//  c_r = 3.34log(n), where n is the number of erts in range
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
const ATTRACTION: f32 = ERT_LENGTH * 10.0;
const CORE_RADIUS: f32 = ERT_LENGTH * 0.25;
const RANGE_RADIUS: f32 = ERT_LENGTH * 2.0;
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
        app.add_systems(Update, process);
    }
}

fn spawn(mut commands: Commands, asset_server: Res<AssetServer>) {
    let effect = asset_server.load("flame-core.ron");

    for i in 0..COUNT {
        let fi = (i + 1) as f32;
        let x = (fi * 10.9898).sin() * ERT_LENGTH * 1.0;
        let y = (fi * 38.233).sin() * ERT_LENGTH * 1.0;
        let z = (fi * 37.719).sin() * ERT_LENGTH * 1.0;
        spawn_ert(
            &mut commands,
            &effect,
            Vec3::new(x, y, z),
            ErtStats {
                attraction: ATTRACTION,
            },
            CORE_RADIUS,
            RANGE_RADIUS,
            Flame::default(),
        );
    }
}

fn process(
    positions: Query<(Entity, &Transform), With<Flame>>,
    sensors: Query<(&ChildOf, &CollidingEntities), With<ErtRange>>,
    mut bodies: Query<(
        Entity,
        &mut LinearVelocity,
        &mut Flame,
        &mut bevy_sprinkles::prelude::ParticleOverride,
    )>,
    time: Res<Time>,
) {
    // Snapshot every ert core's position, keyed by entity.
    let pos: HashMap<Entity, Vec3> = positions.iter().map(|(e, t)| (e, t.translation)).collect();
    let mut temperatures: HashMap<Entity, f32> = HashMap::new();

    // Temperature = sum of 13/d^2 over the cores this ert's sensor currently detects.
    for (child_of, colliding) in &sensors {
        let me_entity = child_of.parent();
        let Some(&me) = pos.get(&me_entity) else {
            continue;
        };

        for &core in colliding.iter() {
            if let Some(&other) = pos.get(&core) {
                let delta = other - me;
                let dist = delta.length();
                *temperatures.entry(me_entity).or_insert(0.0) +=
                    TEMPERATURE_COEFFCIENT / (dist * dist);
            }
        }
    }

    let now = time.elapsed_secs();
    let mut rng = rand::rng();
    for (entity, mut velocity, mut flame, mut ovr) in &mut bodies {
        // Temperature is instantaneous: 0 when no neighbours are in range this frame.
        flame.temperature = temperatures.get(&entity).copied().unwrap_or(0.0);
        *ovr = flame_override(flame.temperature);

        // Re-roll the sporadic direction once per interval, in a fresh random direction.
        if now - flame.last_reroll >= SPORADIC_REROLL_SECS {
            flame.sporadic_velocity = get_sv_slice(flame.temperature, &mut rng);
            flame.last_reroll = now;
        }
        velocity.0 += flame.sporadic_velocity * time.delta_secs();
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

/// Maps an instantaneous flame temperature to its per-instance particle look.
/// Saturates at `T_HOT` so hotter-than-max flames don't produce runaway values.
fn flame_override(temperature: f32) -> bevy_sprinkles::prelude::ParticleOverride {
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
        size_mul: Some(0.5 + 1.0 * heat),
        // emissive scales from a dim ember to a bright hot core (HDR values drive bloom).
        emissive: Some(LinearRgba::new(
            0.4 + 5.0 * heat,
            0.05 + 2.0 * heat,
            0.02 + 0.5 * heat,
            1.0,
        )),
        // author flame-core.ron amount at max density; modulate DOWN (stays <= 1.0).
        emission_rate_mul: Some(0.3 + 0.7 * heat),
        speed_mul: Some(1.0 + 1.0 * heat),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};

    /// The regression that matters: successive rolls point in different directions.
    /// The old sin(time) seeding pinned every roll to a near-constant direction.
    #[test]
    fn sporadic_direction_varies() {
        let mut rng = StdRng::seed_from_u64(42);
        let a = get_sv_slice(10.0, &mut rng).normalize_or_zero();
        let b = get_sv_slice(10.0, &mut rng).normalize_or_zero();
        assert!(
            (a - b).length() > 0.1,
            "consecutive rolls should differ: {a:?} vs {b:?}"
        );
    }

    /// Speed follows the temperature (the `t / 577` in the equation).
    #[test]
    fn sporadic_speed_tracks_temperature() {
        let mut rng = StdRng::seed_from_u64(1);
        let hot = get_sv_slice(20.0, &mut rng).length();
        let cool = get_sv_slice(5.0, &mut rng).length();
        assert!(hot > cool, "hotter ert must move faster: {hot} !> {cool}");
        assert!(
            (hot - 20.0 * SPORADIC_VELOCITY_COEFFCIENT).abs() < 1e-5,
            "speed should equal temperature * coefficient"
        );
    }

    /// A cold ert (no neighbours) must not move.
    #[test]
    fn cold_ert_is_still() {
        let mut rng = StdRng::seed_from_u64(1);
        assert!(get_sv_slice(0.0, &mut rng).length() < 1e-6);
    }

    /// Velocity must always be finite.
    #[test]
    fn sporadic_velocity_is_finite() {
        let mut rng = StdRng::seed_from_u64(1);
        assert!(get_sv_slice(10.0, &mut rng).is_finite());
    }

    #[test]
    fn hot_flame_is_bigger_brighter_than_cool() {
        let cool = flame_override(0.0);
        let hot = flame_override(T_HOT);
        assert!(
            hot.size_mul.unwrap() > cool.size_mul.unwrap(),
            "hot must be bigger"
        );
        // emissive luminance rises with temperature
        let lum = |o: &bevy_sprinkles::prelude::ParticleOverride| {
            let e = o.emissive.unwrap();
            e.red + e.green + e.blue
        };
        assert!(lum(&hot) > lum(&cool), "hot must glow brighter");
    }

    #[test]
    fn override_is_clamped_at_and_above_t_hot() {
        // Above T_HOT the mapping must saturate (no runaway values).
        let hot = flame_override(T_HOT);
        let hotter = flame_override(T_HOT * 4.0);
        assert_eq!(hot.size_mul, hotter.size_mul);
        assert_eq!(hot.emissive.map(|e| e.red), hotter.emissive.map(|e| e.red));
    }
}
