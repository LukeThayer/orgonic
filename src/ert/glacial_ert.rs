use bevy::prelude::*;

use super::{spawn_ert, ERT_LENGTH};

#[derive(Component)]
pub struct Glacial;

const COUNT: usize = 0;
const ATTRACTION: f32 = ERT_LENGTH * 3.0;
const CORE_RADIUS: f32 = ERT_LENGTH * 0.5;
const RANGE_RADIUS: f32 = ERT_LENGTH * 3.0;

pub struct GlacialErtPlugin;

impl Plugin for GlacialErtPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn);
    }
}

fn spawn(mut commands: Commands, asset_server: Res<AssetServer>) {
    let effect = asset_server.load("magic-puff.ron");

    for i in 0..COUNT {
        let fi = (i + 1) as f32;
        let x = (fi * 12.9898).sin() * ERT_LENGTH * 1.0;
        let y = (fi * 78.233).sin() * ERT_LENGTH * 2.0;
        let z = (fi * 37.719).sin() * ERT_LENGTH * 4.0;
        spawn_ert(
            &mut commands,
            &effect,
            Vec3::new(x, y, z),
            CORE_RADIUS,
            RANGE_RADIUS,
            Glacial,
        );
    }
}
