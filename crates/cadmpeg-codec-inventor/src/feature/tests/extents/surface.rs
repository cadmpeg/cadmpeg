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
            solid_input: SolidInput::None,
        },
        slots,
        extra,
        IDENTITY_MATRIX,
        |_| {},
        |_, _, _, _| {},
    )
}

/// A surface extrusion whose slot-14 wrapper is the result wrapper of an
/// earlier new-body solid extrusion.
fn surface_extrusion_over_solid(
    extent: u16,
    slots: &[(usize, u32)],
    extra: Vec<PmDcFeatureProperty>,
) -> Option<Feature> {
    surface_extrusion_with_input(SolidInput::Earlier, extent, slots, extra)
}

fn surface_extrusion_with_input(
    solid_input: SolidInput,
    extent: u16,
    slots: &[(usize, u32)],
    extra: Vec<PmDcFeatureProperty>,
) -> Option<Feature> {
    build_extent_extrusion(
        ExtentCase {
            output: Output::Surface,
            operation: 5,
            extent,
            symmetric: false,
            solid_input,
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

/// Ordinal of the true Boolean that replaces slot 3.
const TRUE_SLOT_3: u32 = 23;

#[test]
fn a_surface_extrusion_states_the_assumed_polarity_for_both_slot_3_values() {
    // `build_extent_extrusion` requires the note on every projected case.
    let reversed = surface_extrusion(
        5,
        5,
        &[(3, TRUE_SLOT_3)],
        vec![test_property(
            TRUE_SLOT_3,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: true,
            },
        )],
    )
    .expect("reversed surface");
    let direction = |feature: &Feature| match feature.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::Extrude { direction, .. }) => {
            direction.clone()
        }
        _ => panic!("not an extrusion"),
    };
    let plain = surface_extrusion(5, 5, &[], Vec::new()).expect("plain surface");
    assert_ne!(direction(&reversed), direction(&plain));
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
    // Slot 14 selects a surface body that no solid extrusion created.
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
            solid_input: SolidInput::None,
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

#[test]
fn through_all_surface_extrusion_over_a_solid_body_is_a_new_sheet() {
    let feature = surface_extrusion_over_solid(5, &[(14, 22)], vec![surface_body(22)])
        .expect("through-all surface over a solid");
    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        extent,
        op,
        solid: Some(false),
        ..
    }) = feature.evaluation.definition()
    else {
        panic!("not a surface extrusion");
    };
    assert_eq!(*op, BooleanOp::NewBody);
    assert!(matches!(
        extent,
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::ThroughAll {},
                draft: None,
            }
        }
    ));
}

#[test]
fn surface_extrusion_over_a_solid_body_refuses_other_slot_14_forms() {
    // The slot-14 wrapper has to be a result wrapper of a solid extrusion.
    assert!(surface_extrusion(5, 5, &[(14, 22)], vec![surface_body(22)]).is_none());
    // Only a through-all extent reads the solid body.
    assert!(surface_extrusion_over_solid(
        7,
        &[(13, 20), (18, 21), (14, 22)],
        vec![
            test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
            plane_surface(21),
            surface_body(22),
        ],
    )
    .is_none());
    // The solid extrusion that stores the wrapper precedes the surface
    // extrusion in the history.
    assert!(surface_extrusion_with_input(
        SolidInput::Later,
        5,
        &[(14, 22)],
        vec![surface_body(22)]
    )
    .is_none());
    // The input wrapper differs from the result wrapper of slot 16.
    assert!(surface_extrusion_over_solid(5, &[(14, 8)], Vec::new()).is_none());
    // The slot-14 record is a surface-body wrapper.
    assert!(surface_extrusion_over_solid(
        5,
        &[(14, 22)],
        vec![test_property(
            22,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: false,
            },
        )],
    )
    .is_none());
}
