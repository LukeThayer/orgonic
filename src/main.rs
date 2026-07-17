mod camera;

#[cfg(feature = "dev")]
use bevy::dev_tools::fps_overlay::FpsOverlayPlugin;
use bevy::prelude::*;
use camera::FlyCameraPlugin;

fn main() {
    let mut app = App::new();
    // DefaultPlugins wires up windowing, rendering, input, audio, and more.
    app.add_plugins(DefaultPlugins)
        // Our fly camera lives in its own plugin (see camera.rs).
        .add_plugins(FlyCameraPlugin)
        .add_systems(Startup, setup);
    #[cfg(feature = "dev")]
    app.add_plugins(FpsOverlayPlugin::default());
    app.run();
}

/// Build the world: a cube and a light. The camera belongs to FlyCameraPlugin.
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // A shaded cube at the origin. In Bevy's current API a renderable entity is a
    // Mesh3d + a MeshMaterial3d + a Transform (required components fill in the rest).
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.8, 0.7, 0.6))),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));

    // A directional light (like the sun) so the cube is actually shaded.
    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
