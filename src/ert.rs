use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_sprinkles::prelude::{ParticleOverride, Particles3d, ParticlesAsset};
use std::collections::HashMap;

mod flame_ert;
mod glacial_ert;

// ---------------------------------------------------------------------------
// Global ert values — shared by every kind of ert.
// ---------------------------------------------------------------------------
/// Radius of an ert's solid core (also the size of its visible sphere).
pub const ERT_LENGTH: f32 = 1.0;
/// Velocity damping, so erts settle instead of oscillating forever.
pub const DAMPING: f32 = 1.0;
/// Attraction cuts off closer than this, avoiding a divide-by-zero blow-up.
pub const DEAD_ZONE: f32 = 0.5 * ERT_LENGTH;

/// Marker: every ert (of any kind) has this.
#[derive(Component)]
pub struct Ert;

/// Per-ert tunables. Each kind spawns with its own values.
#[derive(Component, Clone, Copy)]
pub struct ErtStats {
    /// Pull strength toward in-range neighbours. Negative = repel.
    pub attraction: f32,
}

/// Marker on the child sensor entity, so `attract` can find range sensors.
#[derive(Component)]
struct ErtRange;

/// Physics layers so a range sensor detects cores but NOT other range sensors.
#[derive(PhysicsLayer, Default, Clone, Copy, Debug)]
enum ErtLayer {
    #[default]
    Default,
    Core,
    Range,
}

/// The top-level ert plugin. `main` adds only this; it wires up every kind of
/// ert (via sub-plugins) plus the shared behaviour they all obey.
pub struct ErtPlugin;

impl Plugin for ErtPlugin {
    fn build(&self, app: &mut App) {
        // Each kind is its own sub-plugin.
        app.add_plugins((flame_ert::FlameErtPlugin, glacial_ert::GlacialErtPlugin));
    }
}

/// Shared spawn helper: builds one ert (solid core + child range sensor) of the
/// given `kind`. Sub-plugins call this so all the physics/collider wiring lives
/// in exactly one place.
pub fn spawn_ert(
    commands: &mut Commands,
    effect: &Handle<ParticlesAsset>,
    position: Vec3,
    stats: ErtStats,
    core_radius: f32,
    range_radius: f32,
    kind: impl Bundle,
) {
    commands
        .spawn((
            Ert,
            kind,
            stats,
            RigidBody::Dynamic,
            Collider::sphere(core_radius),
            CollisionLayers::new(ErtLayer::Core, [ErtLayer::Range]),
            GravityScale(0.0),
            LinearDamping(DAMPING),
            LinearVelocity::default(),
            Particles3d(effect.clone()),
            ParticleOverride::default(),
            Transform::from_translation(position),
        ))
        .with_child((
            ErtRange,
            Collider::sphere(range_radius),
            Sensor,
            CollisionLayers::new(ErtLayer::Range, [ErtLayer::Core]),
            CollidingEntities::default(),
            Transform::default(),
        ));
}

/// Shared behaviour: pull every ert toward the cores inside its range sensor,
/// scaled by that ert's own `attraction`.
fn attract(
    positions: Query<(Entity, &Transform), With<Ert>>,
    stats: Query<&ErtStats>,
    sensors: Query<(&ChildOf, &CollidingEntities), With<ErtRange>>,
    mut bodies: Query<(Entity, &mut LinearVelocity), With<Ert>>,
    time: Res<Time>,
) {
    // Snapshot every ert core's position, keyed by entity.
    let pos: HashMap<Entity, Vec3> = positions.iter().map(|(e, t)| (e, t.translation)).collect();

    // Sum, per ert, the pull from the cores its sensor currently detects.
    let mut pulls: HashMap<Entity, Vec3> = HashMap::new();
    for (child_of, colliding) in &sensors {
        let me_entity = child_of.parent();
        let Some(&me) = pos.get(&me_entity) else {
            continue;
        };

        let mut pull = Vec3::ZERO;
        for &core in colliding.iter() {
            if let Some(&other) = pos.get(&core) {
                let delta = other - me;
                let dist = delta.length();
                if dist > DEAD_ZONE {
                    pull += delta / dist;
                }
            }
        }
        *pulls.entry(me_entity).or_insert(Vec3::ZERO) += pull;
    }

    // Apply each ert's pull, scaled by its own attraction stat.
    for (entity, mut velocity) in &mut bodies {
        if let Some(&pull) = pulls.get(&entity) {
            let attraction = stats.get(entity).map(|s| s.attraction).unwrap_or(0.0);
            velocity.0 += pull * attraction * time.delta_secs();
        }
    }
}
