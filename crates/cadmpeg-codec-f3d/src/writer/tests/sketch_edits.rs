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

use crate::test_support::native_test::{f3d_native, update_f3d_native};
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_smbh_and_protein;
use crate::F3dCodec;

#[test]
fn generated_f3d_reports_native_sketch_point_coordinates() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let _expected = update_f3d_native(&mut edited, |native| {
        let point = &mut native.sketch_points[0];
        let mut coordinates = point.coordinates();
        coordinates.u += 12.5;
        coordinates.v -= 7.5;
        point.try_set_coordinates(coordinates).unwrap();
        point.coordinates()
    });

    let (output, report) =
        super::source_less_analytic::synthesize_document(&edited, Some(&fidelity));
    super::source_less_analytic::assert_native_omission(&report);
    let native = f3d_native(output.ir());
    assert!(native.sketch_points.is_empty());
    assert!(native.sketch_curve_identities.is_empty());
    assert!(native.sketch_relations.is_empty());
    assert_eq!(output.ir().model.bodies.len(), edited.model.bodies.len());
    if !edited.model.sketches.is_empty() {
        assert!(report
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::WriterDesignRecordsOmitted.kind()));
    }
}

#[test]
fn generated_f3d_reports_native_sketch_arc_geometry() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let _expected = update_f3d_native(&mut edited, |native| {
        let curve = &mut native.sketch_curve_identities[0];
        let Some(crate::records::sketch_geometry::SketchCurveGeometry::Arc {
            center,
            normal,
            reference_direction,
            ..
        }) = &curve.geometry
        else {
            panic!("generated sketch curve must be an arc")
        };
        let center = center.get();
        curve.geometry = Some(
            crate::records::sketch_geometry::SketchCurveGeometry::arc(
                cadmpeg_ir::math::Point3::new(center.x + 20.0, center.y, center.z),
                *normal.as_raw(),
                *reference_direction.as_raw(),
                35.0,
                0.25,
                2.75,
            )
            .expect("edited arc"),
        );
        curve.geometry.clone()
    });

    let (output, report) =
        super::source_less_analytic::synthesize_document(&edited, Some(&fidelity));
    super::source_less_analytic::assert_native_omission(&report);
    let native = f3d_native(output.ir());
    assert!(native.sketch_points.is_empty());
    assert!(native.sketch_curve_identities.is_empty());
    assert!(native.sketch_relations.is_empty());
    assert_eq!(output.ir().model.bodies.len(), edited.model.bodies.len());
    if !edited.model.sketches.is_empty() {
        assert!(report
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::WriterDesignRecordsOmitted.kind()));
    }
}

#[test]
fn generated_f3d_reports_native_sketch_constraint_mask() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let _expected_references = update_f3d_native(&mut edited, |native| {
        let relation = &mut native.sketch_relations[0];
        relation.definition = crate::records::sketch_relations::SketchRelationDefinition::new(
            0x40,
            relation.definition.pattern().cloned(),
        )
        .expect("valid relation definition");
        relation
            .try_edit(|draft| {
                draft.members = draft
                    .members
                    .iter()
                    .zip(draft.members.iter().rev())
                    .map(|(position, value)| {
                        crate::records::sketch_relations::SketchRelationMember {
                            reference: value.reference.clone(),
                            offset: position.offset,
                            relation_ordinal: position.relation_ordinal,
                        }
                    })
                    .collect::<Vec<_>>()
                    .try_into()
                    .expect("uniform member resolution")
            })
            .unwrap();
        relation
            .try_edit(|draft| {
                for reference in draft.auxiliary_references.values_mut() {
                    *reference = reference.saturating_add(1);
                }
            })
            .unwrap();
        relation
            .try_edit(|draft| {
                draft.return_members = draft
                    .return_members
                    .iter()
                    .zip(draft.return_members.iter().rev())
                    .map(|(position, value)| {
                        crate::records::sketch_relations::SketchRelationReturnMember {
                            reference: value.reference.clone(),
                            offset: position.offset,
                        }
                    })
                    .collect::<Vec<_>>()
                    .try_into()
                    .expect("uniform member resolution")
            })
            .unwrap();
        (
            relation.members().clone(),
            relation.auxiliary_references().clone(),
            relation.owner_reference,
            relation.return_members().clone(),
        )
    });

    let (output, report) =
        super::source_less_analytic::synthesize_document(&edited, Some(&fidelity));
    super::source_less_analytic::assert_native_omission(&report);
    let native = f3d_native(output.ir());
    assert!(native.sketch_points.is_empty());
    assert!(native.sketch_curve_identities.is_empty());
    assert!(native.sketch_relations.is_empty());
    assert_eq!(output.ir().model.bodies.len(), edited.model.bodies.len());
    if !edited.model.sketches.is_empty() {
        assert!(report
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::WriterDesignRecordsOmitted.kind()));
    }
}

#[test]
fn generated_f3d_reports_native_sketch_nurbs_values() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let _expected = update_f3d_native(&mut edited, |native| {
        let curve = &mut native.sketch_curve_identities[1];
        let Some(crate::records::sketch_geometry::SketchCurveGeometry::Nurbs {
            carrier_reference,
            subtype_class_tag,
            subtype_record_index,
            geometry,
        }) = &curve.geometry
        else {
            panic!("generated sketch curve must be NURBS")
        };
        let mut points = geometry.poles().points().copied().collect::<Vec<_>>();
        let point = points.get_mut(1).expect("second spline control point");
        point.x += 15.0;
        point.y -= 5.0;
        let poles = crate::records::sketch_geometry::SketchNurbsPoles::from_wire(
            points,
            geometry.poles().weights().collect(),
        )
        .expect("edited poles");
        curve.geometry = Some(
            crate::records::sketch_geometry::SketchCurveGeometry::nurbs_from_parts(
                *carrier_reference,
                subtype_class_tag.clone(),
                *subtype_record_index,
                crate::records::sketch_geometry::SketchNurbsGeometry::from_parts(
                    geometry.degree(),
                    0.125,
                    8,
                    geometry.knots().to_vec(),
                    poles,
                )
                .expect("edited NURBS"),
            ),
        );
        curve.geometry.clone()
    });

    let (output, report) =
        super::source_less_analytic::synthesize_document(&edited, Some(&fidelity));
    super::source_less_analytic::assert_native_omission(&report);
    let native = f3d_native(output.ir());
    assert!(native.sketch_points.is_empty());
    assert!(native.sketch_curve_identities.is_empty());
    assert!(native.sketch_relations.is_empty());
    assert_eq!(output.ir().model.bodies.len(), edited.model.bodies.len());
    if !edited.model.sketches.is_empty() {
        assert!(report
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::WriterDesignRecordsOmitted.kind()));
    }
}
