// SPDX-License-Identifier: Apache-2.0
//! Hand of a modeled Thread, read from the final helical faces.

use cadmpeg_ir::design::options::Hand;
use cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
use cadmpeg_ir::geometry::ProceduralSurface;
use cadmpeg_ir::math::Vector3;

use crate::records::feature::thread::DesignThreadConstruction;

/// Relative tolerance between a helix pitch and the stored thread pitch.
const EPS_PITCH_RELATIVE: f64 = 1.0e-6;

const MILLIMETRES_PER_CENTIMETRE: f64 = 10.0;

/// Path radius, pitch length and turning sense of one helical surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct HelicalPath {
    radius: f64,
    pitch: f64,
    hand: Hand,
}

/// The helical paths of the procedural surfaces of the final model.
///
/// Rotation from the major to the minor vector turns about `major × minor`;
/// the pitch vector points along that direction for a right-handed helix and
/// against it for a left-handed helix.
pub(super) fn paths<'a>(
    surfaces: impl IntoIterator<Item = &'a ProceduralSurface>,
) -> Vec<HelicalPath> {
    surfaces
        .into_iter()
        .filter_map(|surface| {
            let ProceduralSurfaceDefinition::Helix { construction } = surface.definition() else {
                return None;
            };
            let path = construction.path();
            let (major, minor, pitch): (Vector3, Vector3, Vector3) =
                (path.major().get(), path.minor().get(), path.pitch().get());
            let rise = major.cross(minor).dot(pitch);
            let pitch_length = pitch.norm();
            let hand = if rise > 0.0 {
                Hand::Right
            } else if rise < 0.0 {
                Hand::Left
            } else {
                return None;
            };
            (pitch_length > 0.0).then_some(HelicalPath {
                radius: major.norm(),
                pitch: pitch_length,
                hand,
            })
        })
        .collect()
}

/// The hand that every helical surface of the thread's pitch and radius band
/// states, when at least one exists and all agree.
///
/// A thread face has a path radius between the minor radius less one pitch and
/// the major radius plus one pitch.
pub(super) fn hand(paths: &[HelicalPath], construction: &DesignThreadConstruction) -> Option<Hand> {
    if !construction.modeled {
        return None;
    }
    let pitch = construction.pitch.get() * MILLIMETRES_PER_CENTIMETRE;
    let minor = construction.diameters.minor() * MILLIMETRES_PER_CENTIMETRE / 2.0;
    let major = construction.diameters.major() * MILLIMETRES_PER_CENTIMETRE / 2.0;
    let mut matching = paths.iter().filter(|path| {
        (path.pitch - pitch).abs() <= EPS_PITCH_RELATIVE * pitch
            && path.radius >= minor - pitch
            && path.radius <= major + pitch
    });
    let first = matching.next()?.hand;
    matching.all(|path| path.hand == first).then_some(first)
}

#[cfg(test)]
mod tests;
