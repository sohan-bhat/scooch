use crate::shape;
use crate::{Options, Scene, Scenery};

pub fn active(scene: &Scene, options: &Options) -> bool {
    options.scenery != Scenery::Off && shape::ground(scene).is_some()
}
