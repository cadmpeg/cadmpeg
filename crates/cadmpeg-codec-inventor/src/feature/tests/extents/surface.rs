// SPDX-License-Identifier: Apache-2.0
//! Surface extrusion operands, profile and result.

use super::*;

fn surface_extrusion(
    operation: u16,
    extent: u16,
    slots: &[(usize, u32)],
    extra: Vec<PmDcFeatureProperty>,
) -> Option<Feature> {
    build_extent_extrusion(
        ExtentCase {
            output: Output::Surface,
            operation,
            extent,
            symmetric: false,
        },
        slots,
        extra,
        IDENTITY_MATRIX,
        |_| {},
        |_, _, _, _| {},
    )
}

fn surface_body(ordinal: u32) -> PmDcFeatureProperty {
    test_property(
        ordinal,
        PmDcFeaturePropertyKind::SurfaceBody {
            body: reference(31),
        },
    )
}

#[test]
fn through_all_surface_extrusion_is_a_new_sheet_with_a_selected_profile() {
    let feature = surface_extrusion(5, 5, &[], Vec::new()).expect("through-all surface");
    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        profile,
        op,
        solid,
        extent,
        start,
        ..
    }) = feature.evaluation.definition()
    else {
        panic!("not an extrusion");
    };
    assert_eq!(*op, BooleanOp::NewBody);
    assert_eq!(*solid, Some(false));
    assert!(matches!(
        start,
        cadmpeg_ir::features::ExtrudeStart::ProfilePlane {}
    ));
    assert!(matches!(
        extent,
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::ThroughAll {},
                draft: None,
            }
        }
    ));
    assert!(matches!(
        profile,
        cadmpeg_ir::features::ProfileRef::Planar(
            cadmpeg_ir::features::PlanarProfileRef::SketchSelection { .. }
        )
    ));
}

#[test]
fn to_face_surface_extrusion_ends_at_the_stored_face_plane() {
    let feature = surface_extrusion(
        5,
        7,
        &[(13, 20), (18, 21)],
        vec![
            test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
            plane_surface(21),
        ],
    )
    .expect("to-face surface");
    let ExtrudeExtent::OneSided { side } = extrude_definition(&feature).0 else {
        panic!("to-face surface extrusion is one-sided");
    };
    assert_to_plane(
        &side.termination,
        [0.0, 0.0, 40.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        &plane_surface(21).id(),
    );
}

#[test]
fn surface_extrusion_refuses_operands_it_does_not_define() {
    // Solid part operations select the solid decoder.
    for operation in [1, 2, 3, 4, 6] {
        assert!(surface_extrusion(operation, 5, &[], Vec::new()).is_none());
    }
    // Slot 14 holds a surface body whose role is open.
    assert!(surface_extrusion(5, 5, &[(14, 22)], vec![surface_body(22)]).is_none());
    // Slot 26 is the solid result collection.
    assert!(surface_extrusion(5, 5, &[(26, 22)], vec![surface_body(22)]).is_none());
    // A start target belongs to the from-to extent.
    assert!(surface_extrusion(5, 5, &[(8, 40)], Vec::new()).is_none());
    // Extents that the solid form refuses stay refused.
    assert!(surface_extrusion(5, 2, &[], Vec::new()).is_none());
    assert!(surface_extrusion(5, 7, &[], Vec::new()).is_none());
    // The solid form projects a distance and a work-plane target; the
    // surface form defines neither.
    assert!(extent_extrusion(1, false, &[], Vec::new()).is_some());
    assert!(surface_extrusion(5, 1, &[], Vec::new()).is_none());
    assert!(extent_extrusion(7, false, &[(11, 40)], Vec::new()).is_some());
    assert!(surface_extrusion(5, 7, &[(11, 40)], Vec::new()).is_none());
}

#[test]
fn the_solid_decoder_refuses_the_surface_operation() {
    let solid = build_extent_extrusion(
        ExtentCase {
            output: Output::Solid,
            operation: 5,
            extent: 5,
            symmetric: false,
        },
        &[],
        Vec::new(),
        IDENTITY_MATRIX,
        |_| {},
        |_, _, _, _| {},
    );
    assert!(solid.is_none());
}

#[test]
fn extrusion_families_admit_only_their_own_body_kind() {
    use crate::feature::FeatureFamily;
    let surface = surface_extrusion(5, 5, &[], Vec::new()).expect("through-all surface");
    let solid = extent_extrusion(5, false, &[], Vec::new()).expect("through-all solid");
    let (surface, solid) = (
        surface.evaluation.definition(),
        solid.evaluation.definition(),
    );
    assert!(FeatureFamily::SurfaceExtrusion.admits_definition(surface));
    assert!(!FeatureFamily::Extrusion.admits_definition(surface));
    assert!(FeatureFamily::Extrusion.admits_definition(solid));
    assert!(!FeatureFamily::SurfaceExtrusion.admits_definition(solid));
    assert!(matches!(
        FeatureFamily::from_definition(surface),
        Some(FeatureFamily::SurfaceExtrusion)
    ));
    assert!(matches!(
        FeatureFamily::from_definition(solid),
        Some(FeatureFamily::Extrusion)
    ));
}
