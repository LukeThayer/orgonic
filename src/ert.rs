use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_sprinkles::prelude::{ParticleEmitterOverrides, Particles3d, ParticlesAsset};

pub mod flame_ert;
mod glacial_ert;

// ---------------------------------------------------------------------------
// Global ert values  shared by every kind of ert.
// ---------------------------------------------------------------------------
pub const ERT_LENGTH: f32 = 0.25;
/// Velocity damping
pub const DAMPING: f32 = 1.0;
/// Density used to derive each ert core's mass properties from its collider shape.
/// Roughly water-like, so a flame core (radius `ERT_LENGTH * 0.25`) lands near unit mass.
pub const ERT_DENSITY: f32 = 1000.0;

/// Marker: every ert (of any kind) has this.
#[derive(Component)]
pub struct Ert;

/// Marker on the child sensor entity, so `attract` can find range sensors.
#[derive(Component)]
struct ErtRange;

/// Marker on the child particle-emitter entity. The emitter lives on its own
/// collider-free child so its `Transform` scale (`ERT_LENGTH`) reaches bevy_sprinkles
/// — which scales the whole effect from the emitter's world transform — without avian
/// ever applying that scale to a collider.
#[derive(Component)]
struct ErtParticles;

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
    core_radius: f32,
    range_radius: f32,
    kind: impl Bundle,
) {
    let core = Collider::sphere(core_radius);

    commands
        .spawn((
            Ert,
            kind,
            RigidBody::Dynamic,
            // Avian derives a body's mass from its colliders, but its mass query filters
            // `Without<Sensor>` — and the core below is a `Sensor`, so it contributes
            // nothing. Without this the body is left with zero mass and inertia, which
            // avian warns about and which can feed NaN into the solver. Derive the mass
            // properties explicitly from the very shape the collider uses.
            MassPropertiesBundle::from_shape(&core, ERT_DENSITY),
            core,
            CollisionLayers::new(ErtLayer::Core, [ErtLayer::Core, ErtLayer::Range]),
            // Cores report their own contacts so core-on-core hits can be detected
            // (flame erts explode on one). Includes range sensors as well as other
            // cores, so readers must filter to the entities they care about.
            CollidingEntities::default(),
            Sensor,
            GravityScale(0.0),
            LinearDamping(DAMPING),
            LinearVelocity::default(),
            Transform::from_translation(position),
            // The body has no mesh of its own, but the particle-emitter child does, so
            // the body must be a node in the visibility hierarchy for visibility to
            // propagate to it (otherwise Bevy warns B0004). `Particles3d` used to supply
            // this implicitly when it lived here; now it lives on the child.
            Visibility::default(),
        ))
        .with_children(|parent| {
            parent.spawn((
                ErtRange,
                Collider::sphere(range_radius),
                Sensor,
                CollisionLayers::new(ErtLayer::Range, [ErtLayer::Core]),
                CollidingEntities::default(),
                Transform::default(),
            ));
            // The particle emitter lives on its own collider-free child, scaled by
            // ERT_LENGTH. bevy_sprinkles reads the emitter's world transform and scales
            // the whole effect (emission volume, velocities, gravity) by that factor,
            // while avian only ever sees the un-scaled physics body and its sensor.
            parent.spawn((
                ErtParticles,
                Particles3d(effect.clone()),
                ParticleEmitterOverrides::default(),
                Transform::from_scale(Vec3::splat(ERT_LENGTH)),
            ));
        });
}
