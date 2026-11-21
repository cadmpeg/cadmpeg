// SPDX-License-Identifier: Apache-2.0

use super::*;

/// Face 12 persists in the fillet output of `output()` on surface 212 in place
/// of surface 112, as a plane through `origin` with `normal`; the input holds
/// surface 112 as the plane x = 0.
fn recarried(origin: Point3, normal: Vector3) -> (AsmHistoricalTopology, AsmHistoricalTopology) {
    let mut before = input(z());
    plane(&mut before, 112, Point3::new(0.0, 0.0, 0.0), x());
    let mut after = output();
    after.surfaces.retain(|surface| *surface != 112);
    after.surfaces.push(212);
    for binding in &mut after.face_surfaces {
        if binding.entity == 12 {
            binding.carrier = 212;
        }
    }
    plane(&mut after, 212, origin, normal);
    (before, after)
}

#[test]
fn a_face_recorded_again_on_its_own_plane_is_no_blend() {
    let (before, after) = recarried(Point3::new(0.0, 4.0, -2.0), x());
    assert_eq!(blends(&before, &after), Some(vec![proven(3.0, 17)]));
}

#[test]
fn a_face_on_a_new_carrier_of_other_geometry_stays_a_possible_blend() {
    let (before, after) = recarried(Point3::new(0.5, 4.0, -2.0), x());
    assert_eq!(blends(&before, &after), None);
    let (before, after) = recarried(Point3::new(0.0, 4.0, -2.0), y());
    assert_eq!(blends(&before, &after), None);
}
