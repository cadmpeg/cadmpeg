// SPDX-License-Identifier: Apache-2.0
//! Writer-domain synthetic tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::F3dCodec;

#[test]
fn generated_f3d_rewrites_native_sketch_point_coordinates() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = update_f3d_native(&mut edited, |native| {
        let point = &mut native.sketch_points[0];
        point.coordinates.u += 12.5;
        point.coordinates.v -= 7.5;
        point.coordinates
    });

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("native sketch-point regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        f3d_native(round_trip.ir()).sketch_points[0].coordinates,
        expected
    );
}

#[test]
fn generated_f3d_rewrites_native_sketch_arc_geometry() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = update_f3d_native(&mut edited, |native| {
        let curve = &mut native.sketch_curve_identities[0];
        let Some(crate::records::SketchCurveGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            ..
        }) = &mut curve.geometry
        else {
            panic!("generated sketch curve must be an arc")
        };
        center.x += 20.0;
        *radius = 35.0;
        *start_angle = 0.25;
        *end_angle = 2.75;
        curve.geometry.clone()
    });

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("native sketch-arc regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        f3d_native(round_trip.ir()).sketch_curve_identities[0].geometry,
        expected
    );
}

#[test]
fn generated_f3d_rewrites_native_sketch_constraint_mask() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected_references = update_f3d_native(&mut edited, |native| {
        let relation = &mut native.sketch_relations[0];
        relation.state = 0x40;
        relation.members.reverse();
        for reference in relation.auxiliary_references.values_mut() {
            *reference = reference.saturating_add(1);
        }
        relation.return_members.reverse();
        (
            relation.members.clone(),
            relation.auxiliary_references.clone(),
            relation.owner_reference,
            relation.return_members.clone(),
        )
    });

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("native sketch-constraint regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    let native = f3d_native(round_trip.ir());
    let relation = &native.sketch_relations[0];
    assert_eq!(relation.state, 0x40);
    assert_eq!(
        relation.constraint_kinds(),
        [crate::records::SketchConstraintKind::Horizontal]
    );
    assert_eq!(relation.unknown_constraint_bits(), 0);
    assert_eq!(relation.members, expected_references.0);
    assert_eq!(relation.auxiliary_references, expected_references.1);
    assert_eq!(relation.owner_reference, expected_references.2);
    assert_eq!(relation.return_members, expected_references.3);
}

#[test]
fn generated_f3d_rewrites_native_sketch_nurbs_values() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = update_f3d_native(&mut edited, |native| {
        let curve = &mut native.sketch_curve_identities[1];
        let Some(crate::records::SketchCurveGeometry::Nurbs {
            fit_tolerance,
            poles,
            ..
        }) = &mut curve.geometry
        else {
            panic!("generated sketch curve must be NURBS")
        };
        *fit_tolerance = 0.125;
        let point = poles.points_mut().nth(1).expect("second spline control point");
        point.x += 15.0;
        point.y -= 5.0;
        curve.geometry.clone()
    });

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("native sketch-NURBS regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        f3d_native(round_trip.ir()).sketch_curve_identities[1].geometry,
        expected
    );
}
