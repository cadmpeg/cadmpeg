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
        cadmpeg_ir::features::extents::ExtrudeStart::ProfilePlane {}
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
        cadmpeg_ir::features::profiles::ProfileRef::Planar(
            cadmpeg_ir::features::profiles::PlanarProfileRef::SketchSelection { .. }
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

/// A plane face perpendicular to the +z travel direction at `z` centimetres.
fn plane_face_at(ordinal: u32, z: f64) -> PmDcFeatureProperty {
    let mut surface = test_property(
        ordinal,
        PmDcFeaturePropertyKind::PlaneSurface {
            references: [reference(0); 3],
            extension: None,
            state: 1,
            surface_form: PLANE_SURFACE_FORM,
            value: 0,
            origin: reals([0.0, 0.0, z]),
            u_axis: reals([1.0, 0.0, 0.0]),
            v_axis: reals([0.0, 1.0, 0.0]),
        },
    );
    surface.edit(|payload| payload.save_version_major = 19);
    surface
}

/// A plane face tilted about the x axis, with unit normal (0, -0.8, 0.6),
/// through (0, 0, `z`) centimetres.
fn tilted_plane_face(ordinal: u32, z: f64, v_axis: [f64; 3]) -> PmDcFeatureProperty {
    let mut surface = plane_face_at(ordinal, z);
    surface.edit(|payload| {
        if let PmDcFeaturePropertyKind::PlaneSurface { v_axis: v, .. } = &mut payload.kind {
            *v = reals(v_axis);
        }
    });
    surface
}

const FROM_TO_SLOTS: [(usize, u32); 4] = [(10, 20), (17, 21), (13, 22), (18, 23)];

fn from_to_surface(
    start_z: f64,
    end_z: f64,
    slots: &[(usize, u32)],
    start_flag: u8,
) -> Option<Feature> {
    surface_extrusion(
        5,
        6,
        slots,
        vec![
            test_property(
                20,
                PmDcFeaturePropertyKind::TerminationFlag { value: start_flag },
            ),
            plane_face_at(21, start_z),
            test_property(22, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
            plane_face_at(23, end_z),
        ],
    )
}

#[test]
fn from_to_surface_extrusion_spans_the_start_face_to_the_end_face() {
    use cadmpeg_ir::features::extents::{ExtrudeStart, PlaneTargetSource};
    let feature = from_to_surface(1.0, 4.0, &FROM_TO_SLOTS, 1).expect("from-to surface");
    let FeatureDefinition::Operation(FeatureOperation::Extrude { solid, .. }) =
        feature.evaluation.definition()
    else {
        panic!("not an extrusion");
    };
    assert_eq!(*solid, Some(false));
    let (extent, start) = extrude_definition(&feature);
    let ExtrudeStart::FromPlane {
        plane,
        native,
        source,
    } = start
    else {
        panic!("from-to surface starts at a plane");
    };
    assert_eq!(*source, Some(PlaneTargetSource::FaceSurface));
    assert_eq!(native.as_str(), plane_face_at(21, 1.0).id());
    assert!((plane.origin().get().z - 10.0).abs() < EPS_SAME_COORDINATE);
    let ExtrudeExtent::OneSided { side } = extent else {
        panic!("from-to surface extrusion is one-sided");
    };
    assert!(matches!(
        side.termination,
        LinearTermination::ToPlane {
            source: Some(PlaneTargetSource::FaceSurface),
            ..
        }
    ));
    assert_to_plane(
        &side.termination,
        [0.0, 0.0, 40.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        &plane_face_at(23, 4.0).id(),
    );
    // One target-face note for the start face and one for the end face.
    assert_eq!(
        crate::feature::extrusion::native_face_targets(feature.evaluation.definition()),
        2
    );
}

fn from_to_tilted(start: (f64, [f64; 3]), end: (f64, [f64; 3])) -> Option<Feature> {
    surface_extrusion(
        5,
        6,
        &FROM_TO_SLOTS,
        vec![
            test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
            tilted_plane_face(21, start.0, start.1),
            test_property(22, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
            tilted_plane_face(23, end.0, end.1),
        ],
    )
}

#[test]
fn from_to_surface_extrusion_orders_tilted_faces_along_the_travel() {
    let tilt = [0.0, 0.6, 0.8];
    assert!(from_to_tilted((1.0, tilt), (4.0, tilt)).is_some());
    // The end plane lies behind the start plane along the travel direction.
    assert!(from_to_tilted((4.0, tilt), (1.0, tilt)).is_none());
    // Planes that are not parallel meet the travel line in an order that
    // depends on the profile point; a sketch without curves has none, so
    // both orders of origins stay native.
    let other = [0.0, 0.8, 0.6];
    assert!(from_to_tilted((1.0, tilt), (4.0, other)).is_none());
    assert!(from_to_tilted((4.0, tilt), (1.0, other)).is_none());
    // The travel direction lies in the face planes.
    assert!(from_to_tilted((1.0, [0.0, 0.0, 1.0]), (4.0, [0.0, 0.0, 1.0])).is_none());
}

#[test]
fn from_to_surface_extrusion_refuses_operands_it_does_not_define() {
    // A work-plane start stays native.
    assert!(from_to_surface(1.0, 4.0, &[(8, 41), (13, 22), (18, 23)], 1).is_none());
    // A start termination flag other than 1.
    assert!(from_to_surface(1.0, 4.0, &FROM_TO_SLOTS, 0).is_none());
    // A start face without its surface.
    assert!(from_to_surface(1.0, 4.0, &[(10, 20), (13, 22), (18, 23)], 1).is_none());
    // A non-null slot 14 requires the through-all extent.
    assert!(surface_extrusion_over_solid(
        6,
        &[(10, 20), (17, 21), (13, 22), (18, 23), (14, 24)],
        vec![
            test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
            plane_face_at(21, 1.0),
            test_property(22, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
            plane_face_at(23, 4.0),
            surface_body(24),
        ],
    )
    .is_none());
    // The start plane lies behind the end plane along the travel direction.
    assert!(from_to_surface(4.0, 1.0, &FROM_TO_SLOTS, 1).is_none());
    assert!(from_to_surface(2.0, 2.0, &FROM_TO_SLOTS, 1).is_none());
    // Start-face slots belong to the from-to extent only.
    assert!(surface_extrusion(
        5,
        7,
        &[(10, 20), (17, 21), (13, 22), (18, 23)],
        vec![
            test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
            plane_face_at(21, 1.0),
            test_property(22, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
            plane_face_at(23, 4.0),
        ],
    )
    .is_none());
}

/// A plane face through `origin` centimetres with the given axes.
fn framed_plane_face(
    ordinal: u32,
    origin: [f64; 3],
    u: [f64; 3],
    v: [f64; 3],
) -> PmDcFeatureProperty {
    let mut surface = plane_face_at(ordinal, 0.0);
    surface.edit(|payload| {
        if let PmDcFeaturePropertyKind::PlaneSurface {
            origin: o,
            u_axis,
            v_axis,
            ..
        } = &mut payload.kind
        {
            *o = reals(origin);
            *u_axis = reals(u);
            *v_axis = reals(v);
        }
    });
    surface
}

/// The four from-to properties with the given end flag and faces.
fn from_to_properties(
    end_flag: u8,
    start: PmDcFeatureProperty,
    end: PmDcFeatureProperty,
) -> Vec<PmDcFeatureProperty> {
    vec![
        test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
        start,
        test_property(
            22,
            PmDcFeaturePropertyKind::TerminationFlag { value: end_flag },
        ),
        end,
    ]
}

fn true_boolean(ordinal: u32) -> PmDcFeatureProperty {
    test_property(
        ordinal,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value: true,
        },
    )
}

/// A from-to surface extrusion between two tilted faces whose profile sketch
/// holds the given non-construction lines, in millimetres of the sketch plane.
/// The sketch lies in the plane z = 0 with u = +x and v = +y.
fn from_to_over_profile(
    start: PmDcFeatureProperty,
    end: PmDcFeatureProperty,
    lines: &[([f64; 2], [f64; 2])],
) -> Option<Feature> {
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometryDefinition as Curve};
    let lines = lines.to_vec();
    build_extent_extrusion(
        ExtentCase {
            output: Output::Surface,
            operation: 5,
            extent: 6,
            symmetric: false,
            solid_input: SolidInput::None,
        },
        &FROM_TO_SLOTS,
        from_to_properties(1, start, end),
        IDENTITY_MATRIX,
        |index| {
            let sketch = index
                .sketch_ids
                .values()
                .next()
                .expect("neutral sketch")
                .clone();
            for (ordinal, (a, b)) in lines.into_iter().enumerate() {
                let id = SketchEntityId::mint(format!("test:design:entity#{ordinal}"))
                    .expect("entity id");
                let line = SketchEntity::new(
                    id.clone(),
                    sketch.clone(),
                    Curve::Line {
                        start: Point2::new(a[0], a[1]),
                        end: Point2::new(b[0], b[1]),
                    }
                    .try_into()
                    .expect("line geometry"),
                );
                let line: &'static SketchEntity = Box::leak(Box::new(line));
                index
                    .neutral_entities
                    .insert(Box::leak(id.as_str().to_owned().into_boxed_str()), line);
            }
        },
        |_, _, _, _| {},
    )
}

#[test]
fn from_to_surface_extrusion_orders_tilted_faces_at_the_profile() {
    const NEAR: [([f64; 2], [f64; 2]); 1] = [([0.0, 0.0], [10.0, 0.0])];
    const FAR: [([f64; 2], [f64; 2]); 1] = [([0.0, 100.0], [10.0, 100.0])];
    const SPAN: [([f64; 2], [f64; 2]); 1] = [([0.0, 0.0], [10.0, 100.0])];
    // Travel +z. Face A has normal (0, -0.8, 0.6) through z = 10 mm and face
    // B has normal (0, -0.6, 0.8) through z = 40 mm. The line through a
    // profile point (0, y, 0) meets A at z = (10 + 0.8 y) / 0.6 and B at
    // z = (40 + 0.6 y) / 0.8, so A comes first for y < 66.7 mm and B first
    // beyond: y = 0 gives 16.7 and 50; y = 100 gives 143.3 and 115.
    let a = || tilted_plane_face(21, 1.0, [0.0, 0.6, 0.8]);
    let b = || tilted_plane_face(21, 4.0, [0.0, 0.8, 0.6]);
    let b_end = || tilted_plane_face(23, 4.0, [0.0, 0.8, 0.6]);
    let a_end = || tilted_plane_face(23, 1.0, [0.0, 0.6, 0.8]);
    // Start A, end B: ordered at y = 0, reversed at y = 100.
    assert!(from_to_over_profile(a(), b_end(), &NEAR).is_some());
    assert!(from_to_over_profile(a(), b_end(), &FAR).is_none());
    // Start B, end A: ordered at y = 100, reversed at y = 0.
    assert!(from_to_over_profile(b(), a_end(), &FAR).is_some());
    assert!(from_to_over_profile(b(), a_end(), &NEAR).is_none());
    // A profile that spans y = 0 to 100 meets the faces in both orders.
    assert!(from_to_over_profile(a(), b_end(), &SPAN).is_none());
    assert!(from_to_over_profile(b(), a_end(), &SPAN).is_none());
    // Without a profile curve the order is not defined for tilted planes.
    assert!(from_to_over_profile(a(), b_end(), &[]).is_none());
}

#[test]
fn from_to_surface_extrusion_follows_the_slot_3_travel() {
    // Slot 3 true reverses the travel to -z. The travel line meets the plane
    // at z = 40 mm before the plane at z = 10 mm, so the start plane is the
    // upper one.
    let travel = vec![true_boolean(24)];
    let slots = [(3, 24), (10, 20), (17, 21), (13, 22), (18, 23)];
    let build = |start_z: f64, end_z: f64| {
        let mut extra = from_to_properties(1, plane_face_at(21, start_z), plane_face_at(23, end_z));
        extra.extend(travel.clone());
        surface_extrusion(5, 6, &slots, extra)
    };
    assert!(build(4.0, 1.0).is_some());
    assert!(build(1.0, 4.0).is_none());
}

#[test]
fn from_to_surface_extrusion_follows_a_travel_other_than_z() {
    // Direction record 62 is +x. Planes with normal u x v = (0,1,0) x (0,0,1)
    // = +x at x = 10 mm and x = 40 mm: the line meets the start plane first.
    let slots = [(2, 62), (10, 20), (17, 21), (13, 22), (18, 23)];
    let face =
        |ordinal, x| framed_plane_face(ordinal, [x, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let build = |start_x: f64, end_x: f64| {
        surface_extrusion(
            5,
            6,
            &slots,
            from_to_properties(1, face(21, start_x), face(23, end_x)),
        )
    };
    assert!(build(1.0, 4.0).is_some());
    assert!(build(4.0, 1.0).is_none());
}

#[test]
fn from_to_surface_extrusion_requires_the_operands_of_its_form() {
    let faces =
        |end_flag| from_to_properties(end_flag, plane_face_at(21, 1.0), plane_face_at(23, 4.0));
    // An end termination flag other than 1.
    assert!(surface_extrusion(5, 6, &FROM_TO_SLOTS, faces(0)).is_none());
    assert!(surface_extrusion(5, 6, &FROM_TO_SLOTS, faces(1)).is_some());
    // Slot 8 holds no operand beside a start face.
    let with_work_plane = faces(1);
    assert!(surface_extrusion(
        5,
        6,
        &[(8, 41), (10, 20), (17, 21), (13, 22), (18, 23)],
        with_work_plane,
    )
    .is_none());
    // Slot 29 holds a second extent: the two-sided form is not a from-to.
    let mut two_sided = faces(1);
    two_sided.push(enum_property(25, PmDcFeatureEnumFamily::Extent, 1));
    let mut slots = FROM_TO_SLOTS.to_vec();
    slots.push((29, 25));
    assert!(surface_extrusion(5, 6, &slots, two_sided).is_none());
    // The symmetric flag has no from-to meaning.
    assert!(build_extent_extrusion(
        ExtentCase {
            output: Output::Surface,
            operation: 5,
            extent: 6,
            symmetric: true,
            solid_input: SolidInput::None,
        },
        &FROM_TO_SLOTS,
        faces(1),
        IDENTITY_MATRIX,
        |_| {},
        |_, _, _, _| {},
    )
    .is_none());
    // Start-face slots with the through-all extent.
    assert!(surface_extrusion(5, 5, &FROM_TO_SLOTS, faces(1)).is_none());
}

#[test]
fn from_to_surface_extrusion_bounds_the_travel_crossing_by_a_cosine() {
    // Both planes share the normal (s, 0, c), the cosine with the +z travel
    // being c: u x v = (0,1,0) x (-c,0,s) = (s, 0, c). The planes are 30 mm
    // apart along z, so the gap along the travel is 30 for every c.
    let build = |c: f64| {
        let s = (1.0 - c * c).sqrt();
        let face =
            |ordinal, z| framed_plane_face(ordinal, [0.0, 0.0, z], [0.0, 1.0, 0.0], [-c, 0.0, s]);
        surface_extrusion(
            5,
            6,
            &FROM_TO_SLOTS,
            from_to_properties(1, face(21, 1.0), face(23, 4.0)),
        )
    };
    // The bound is 1e-6: a cosine below it does not cross, above it does.
    assert!(build(0.5e-6).is_none());
    assert!(build(2.0e-6).is_some());
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
