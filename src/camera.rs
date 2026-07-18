use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::ert;

/// Marker + state for the fly camera. Storing yaw/pitch ourselves keeps mouse-look
/// stable (we rebuild the rotation from them each frame instead of accumulating
/// floating-point drift on the Quat).
#[derive(Component)]
pub struct FlyCam {
    pub yaw: f32,
    pub pitch: f32,
    pub speed: f32,
    pub sensitivity: f32,
}

impl Default for FlyCam {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
            speed: 6.0,
            sensitivity: 0.002,
        }
    }
}

/// Bundles the camera's spawn + behavior into one reusable unit.
pub struct FlyCameraPlugin;

impl Plugin for FlyCameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera)
            .add_systems(Update, (toggle_cursor_grab, camera_look, camera_move));
    }
}

/// Spawn the camera at (0,0,6). With yaw=pitch=0 the identity rotation looks down
/// -Z, straight at the cube at the origin.
fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 0.0, 6.0),
        FlyCam::default(),
    ));
}

/// Left-click captures the cursor (locks + hides it); Esc releases it.
fn toggle_cursor_grab(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
) {
    if mouse.just_pressed(MouseButton::Left) {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
    if keys.just_pressed(KeyCode::Escape) {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
}

/// Mouse-look: only while the cursor is captured. Yaw around world Y, pitch around
/// local X, clamped so you can't flip over the poles.
fn camera_look(
    mouse_motion: Res<AccumulatedMouseMotion>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    cam: Single<(&mut Transform, &mut FlyCam)>,
) {
    if cursor.grab_mode == CursorGrabMode::None {
        return;
    }
    let (mut transform, mut fly) = cam.into_inner();
    let delta = mouse_motion.delta;
    fly.yaw -= delta.x * fly.sensitivity;
    fly.pitch -= delta.y * fly.sensitivity;
    fly.pitch = fly.pitch.clamp(-1.54, 1.54); // ~±88°
    transform.rotation = Quat::from_euler(EulerRot::YXZ, fly.yaw, fly.pitch, 0.0);
}

/// WASD/QE movement in the camera's local frame, framerate-independent via Time.
fn camera_move(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    cam: Single<(&mut Transform, &FlyCam)>,
    commands: Commands,
    asset_server: Res<AssetServer>,
) {
    if cursor.grab_mode == CursorGrabMode::None {
        return;
    }
    let (mut transform, fly) = cam.into_inner();

    let forward = *transform.forward();
    let right = *transform.right();
    let up = Vec3::Y;

    let mut direction = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        direction += forward;
    }
    if keys.pressed(KeyCode::KeyS) {
        direction -= forward;
    }
    if keys.pressed(KeyCode::KeyD) {
        direction += right;
    }
    if keys.pressed(KeyCode::KeyA) {
        direction -= right;
    }
    if keys.pressed(KeyCode::KeyE) {
        direction += up;
    }
    if keys.pressed(KeyCode::KeyQ) {
        direction -= up;
    }
    if keys.just_released(KeyCode::KeyF) {
        ert::flame_ert::spawn(
            transform.translation + forward * 2.0,
            commands,
            asset_server,
        );
    }

    let boost = if keys.pressed(KeyCode::ShiftLeft) {
        3.0
    } else {
        1.0
    };

    if direction != Vec3::ZERO {
        transform.translation += direction.normalize() * fly.speed * boost * time.delta_secs();
    }
}
