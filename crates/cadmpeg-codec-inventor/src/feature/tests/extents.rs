// SPDX-License-Identifier: Apache-2.0
//! Extrusion extents and the records their targets use.

use super::*;

const EPS_SAME_COORDINATE: f64 = 1e-12;

fn reals(values: [f64; 3]) -> [cadmpeg_ir::scalar::FiniteReal; 3] {
    values.map(|value| cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite fixture value"))
}

/// Assert a to-plane termination: origin in millimetres, normal, in-plane
/// axis, and native record.
fn assert_to_plane(
    termination: &LinearTermination,
    origin: [f64; 3],
    normal: [f64; 3],
    u_axis: [f64; 3],
    native: &str,
) {
    let LinearTermination::ToPlane { plane, native: id } = termination else {
        panic!("not a to-plane termination: {termination:?}");
    };
    let close = |actual: cadmpeg_ir::math::Vector3, expected: [f64; 3]| {
        (actual.x - expected[0]).abs() < EPS_SAME_COORDINATE
            && (actual.y - expected[1]).abs() < EPS_SAME_COORDINATE
            && (actual.z - expected[2]).abs() < EPS_SAME_COORDINATE
    };
    let point = plane.origin().get();
    assert!(close(
        cadmpeg_ir::math::Vector3::new(point.x, point.y, point.z),
        origin
    ));
    assert!(close(plane.normal().get(), normal));
    assert!(close(plane.u_axis().get(), u_axis));
    assert_eq!(id.as_str(), native);
}

fn work_plane(ordinal: u32) -> PmDcWorkPlane {
    Located::new(
        PmDcWorkPlanePayload {
            save_version_major: 19,
            header: test_header(),
            entity_flags: 0x80_8000,
            values: vec![0; 3],
            marker: false,
            origin: reals([1.0, -2.0, 0.5]),
            u_axis: reals([1.0, 0.0, 0.0]),
            v_axis: reals([0.0, 1.0, 0.0]),
        },
        crate::record_identity::RecordTypeId::from_bytes(WORK_PLANE_TYPE),
        segment(),
        ordinal,
    )
}

/// Extrusion fixture with a kernel-entity profile link, a configurable
/// extent value and symmetric Boolean, and additional slots.
fn extent_extrusion(
    extent: u16,
    symmetric: bool,
    slots: &[(usize, u32)],
    extra: Vec<PmDcFeatureProperty>,
) -> Option<(Feature, Vec<DesignParameter>)> {
    configured_extent_extrusion(extent, symmetric, slots, extra, |_| {}, |_, _, _, _| {})
}

fn configured_extent_extrusion(
    extent: u16,
    symmetric: bool,
    slots: &[(usize, u32)],
    extra: Vec<PmDcFeatureProperty>,
    configure: impl FnOnce(&mut crate::feature::ProjectionIndex<'_>),
    check: impl FnOnce(
        &PmDcFeature,
        &PmDcFeatureLabel,
        &crate::feature::ProjectionIndex<'_>,
        Option<&Feature>,
    ),
) -> Option<(Feature, Vec<DesignParameter>)> {
    let raw_parameters = vec![
        raw_parameter(70),
        raw_parameter(71),
        raw_parameter(72),
        raw_parameter(73),
        raw_parameter(74),
        raw_parameter(75),
    ];
    let neutral_parameters = vec![
        neutral_parameter(
            &raw_parameters[0],
            ParameterValue::Length(Length::new(12.0).expect("finite length fixture")),
        ),
        neutral_parameter(
            &raw_parameters[1],
            ParameterValue::Angle(Angle::new(0.0).expect("finite angle fixture")),
        ),
        neutral_parameter(
            &raw_parameters[2],
            ParameterValue::Length(Length::new(3.0).expect("finite length fixture")),
        ),
        neutral_parameter(
            &raw_parameters[3],
            ParameterValue::Angle(Angle::new(0.2).expect("finite angle fixture")),
        ),
        neutral_parameter(
            &raw_parameters[4],
            ParameterValue::Length(Length::new(0.0).expect("finite length fixture")),
        ),
        neutral_parameter(
            &raw_parameters[5],
            ParameterValue::Angle(Angle::new(-0.2).expect("finite angle fixture")),
        ),
    ];
    let raw_sketch = Located::new(
        crate::sketch::PmDcSketchPayload {
            save_version_major: 19,
            header: test_header(),
            state: 0,
            count_value: 0,
            entities: PmDcReferenceList::new(8, None, Vec::new()).expect("empty entity list"),
            transform: reference(0),
            direction: reference(0),
            values: [0; 2],
            auxiliary: None,
        },
        crate::record_identity::RecordTypeId::try_from(
            "114d8790d011f8d10008cabc0663dc09".to_owned(),
        )
        .expect("test GUID"),
        segment(),
        50,
    );
    let neutral_sketch = Sketch {
        id: SketchId::mint(format!("inventor:design:sketch#{}-50", segment()))
            .expect("valid test fixture"),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid test fixture"),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: Some(raw_sketch.id()),
    };
    let direction = Located::new(
        crate::sketch::PmDcDirectionPayload {
            save_version_major: 19,
            header: test_header(),
            entity_flags: 0,
            parameter: cadmpeg_ir::scalar::FiniteReal::ZERO,
            extension: None,
            direction: [
                cadmpeg_ir::scalar::FiniteReal::ZERO,
                cadmpeg_ir::scalar::FiniteReal::ZERO,
                cadmpeg_ir::scalar::FiniteReal::ONE,
            ],
        },
        crate::record_identity::RecordTypeId::try_from(
            "40df52ced011d0d20008ccbc0663dc09".to_owned(),
        )
        .expect("test GUID"),
        segment(),
        60,
    );
    let mut properties = vec![
        test_property(
            1,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::PartOperation,
                type_value: 5,
                value: 3,
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BoundaryPatch,
                items: reference_list(&[4]),
            },
        ),
        test_property(
            3,
            PmDcFeaturePropertyKind::ProfileSelection {
                entity_link: reference(52),
                value: 0,
            },
        ),
        test_property(
            4,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: false,
            },
        ),
        test_property(
            5,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Extent,
                type_value: 11,
                value: extent,
            },
        ),
        test_property(
            6,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: symmetric,
            },
        ),
        test_property(
            7,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[9]),
            },
        ),
        test_property(
            8,
            PmDcFeaturePropertyKind::SurfaceBody {
                body: reference(30),
            },
        ),
    ];
    properties.extend(extra);
    for property in &mut properties {
        let type_id = match property.kind {
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BoundaryPatch,
                ..
            } => Some(crate::feature::BOUNDARY_PATCH_TYPE),
            PmDcFeaturePropertyKind::ProfileSelection { .. } => {
                Some(crate::feature::PROFILE_SELECTION_TYPE)
            }
            _ => None,
        };
        if let Some(type_id) = type_id {
            property.identity.type_id = crate::record_identity::RecordTypeId::from_bytes(type_id);
        }
    }
    let mut all_slots = vec![
        (0, 1),
        (1, 2),
        (2, 60),
        (3, 4),
        (4, 70),
        (5, 71),
        (6, 5),
        (7, 6),
        (23, 2),
        (26, 7),
    ];
    all_slots.extend_from_slice(slots);
    let feature = test_feature(100, 30, &all_slots);
    let label = test_label(100, 5, EXTRUSION_CLASS_ID, &[51]);
    let planes = vec![work_plane(40), work_plane(41)];
    let mut index = test_projection_index(
        &properties,
        &raw_parameters,
        &neutral_parameters,
        std::slice::from_ref(&raw_sketch),
        std::slice::from_ref(&neutral_sketch),
        std::slice::from_ref(&direction),
        &[],
        &[],
    );
    index.entity_style_links.insert(("generated", 51));
    for plane in &planes {
        index.work_planes.insert(
            (
                plane.identity.segment_token.as_str(),
                plane.identity.record_ordinal,
            ),
            Some(plane),
        );
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    configure(&mut index);
    let mut losses = Vec::new();
    let projected = crate::feature::extrusion::project(&ctx, &feature, &label, &index, &mut losses)
        .map(|projected| projected.expect("extrusion projection").0);
    if projected.as_ref().is_some_and(|feature| {
        crate::feature::extrusion::has_native_profile(feature.evaluation.definition())
    }) {
        assert!(losses.iter().any(|note| note.code.local_code()
            == crate::loss::InventorLossCode::FeatureExtrusionProfileNative.code()));
    } else if projected.is_none() {
        assert!(losses.iter().any(|note| note.code.local_code()
            == crate::loss::InventorLossCode::FeatureExtrusionOperandsNative.code()));
    } else {
        assert!(losses.is_empty());
    }
    check(&feature, &label, &index, projected.as_ref());
    projected.map(|projected| (projected, neutral_parameters))
}

fn extrude_definition(
    projected: &(Feature, Vec<DesignParameter>),
) -> (&ExtrudeExtent, &cadmpeg_ir::features::ExtrudeStart) {
    match projected.0.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::Extrude { extent, start, .. }) => {
            (extent, start)
        }
        other => panic!("not an extrusion: {other:?}"),
    }
}

fn enum_property(ordinal: u32, family: PmDcFeatureEnumFamily, value: u16) -> PmDcFeatureProperty {
    test_property(
        ordinal,
        PmDcFeaturePropertyKind::Enumeration {
            family,
            type_value: 11,
            value,
        },
    )
}

fn plane_surface(ordinal: u32) -> PmDcFeatureProperty {
    test_property(
        ordinal,
        PmDcFeaturePropertyKind::PlaneSurface {
            references: [reference(0); 3],
            state: 1,
            surface_form: PLANE_SURFACE_FORM,
            value: 0,
            origin: reals([0.0, 0.0, 4.0]),
            u_axis: reals([0.0, 1.0, 0.0]),
            v_axis: reals([0.0, 0.0, 1.0]),
        },
    )
}

fn typed_extent_extrusion(version: u8, type_value: i16) -> Option<(Feature, Vec<DesignParameter>)> {
    let property = Located::new(
        PmDcFeaturePropertyPayload {
            save_version_major: version,
            header: test_header(),
            kind: PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Extent,
                type_value,
                value: 1,
            },
        },
        crate::record_identity::RecordTypeId::from_bytes(crate::feature::EXTENT_TYPE),
        segment(),
        22,
    );
    extent_extrusion(1, false, &[(6, 22)], vec![property])
}

#[test]
fn versioned_extent_types_project_the_same_one_sided_extrusion() {
    let expected = typed_extent_extrusion(21, 11).expect("type-11 distance extrusion");
    assert!(matches!(
        extrude_definition(&expected).0,
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind { length },
                draft: None,
            }
        } if length.value_from(&expected.1).expect("bound length").get() == 12.0
    ));
    for version in [22, 23, 25, 28, 29, 30, 31] {
        let actual = typed_extent_extrusion(version, 12).expect("type-12 distance extrusion");
        assert_eq!(
            actual.0.evaluation.definition(),
            expected.0.evaluation.definition()
        );
    }
}

#[test]
fn unrelated_and_wrong_version_extent_types_are_rejected() {
    for version in [21, 22, 29] {
        assert!(typed_extent_extrusion(version, 13).is_none());
    }
    assert!(typed_extent_extrusion(21, 12).is_none());
    assert!(typed_extent_extrusion(22, 11).is_none());
}

#[test]
fn extrusion_extent_branches_project_exactly() {
    let symmetric = extent_extrusion(2, true, &[], Vec::new()).expect("symmetric distance");
    assert!(matches!(
        extrude_definition(&symmetric).0,
        ExtrudeExtent::Symmetric {
            side: ExtrudeSide {
                termination: LinearTermination::Blind { length },
                draft: None,
            }
        } if length.value_from(&symmetric.1).expect("bound symmetric length").get() == 12.0
    ));
    assert!(extent_extrusion(2, false, &[], Vec::new()).is_none());
    assert!(extent_extrusion(7, true, &[(11, 40)], Vec::new()).is_none());

    let to_plane = extent_extrusion(7, false, &[(11, 40)], Vec::new()).expect("to work plane");
    let ExtrudeExtent::OneSided { side } = extrude_definition(&to_plane).0 else {
        panic!("to-plane extrusion is one-sided");
    };
    assert_to_plane(
        &side.termination,
        [10.0, -20.0, 5.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        &work_plane(40).id(),
    );
    assert!(extent_extrusion(7, false, &[(11, 40), (13, 20)], Vec::new()).is_none());

    let to_face = extent_extrusion(
        7,
        false,
        &[(13, 20), (18, 21)],
        vec![
            test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
            plane_surface(21),
        ],
    )
    .expect("to face");
    let ExtrudeExtent::OneSided { side } = extrude_definition(&to_face).0 else {
        panic!("to-face extrusion is one-sided");
    };
    assert_to_plane(
        &side.termination,
        [0.0, 0.0, 40.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        &plane_surface(21).id(),
    );
    assert!(extent_extrusion(
        7,
        false,
        &[(13, 20), (18, 21)],
        vec![
            test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 0 }),
            plane_surface(21),
        ],
    )
    .is_none());

    let from_to = extent_extrusion(6, false, &[(8, 41), (11, 40)], Vec::new()).expect("from-to");
    let (extent, start) = extrude_definition(&from_to);
    let cadmpeg_ir::features::ExtrudeStart::FromPlane { plane, native } = start else {
        panic!("from-to extrusion starts at a plane: {start:?}");
    };
    assert_eq!(native.as_str(), work_plane(41).id());
    assert_eq!(plane.origin().get(), Point3::new(10.0, -20.0, 5.0));
    assert_eq!(plane.normal().get(), Vector3::new(0.0, 0.0, 1.0));
    assert!(matches!(
        extent,
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::ToPlane { .. },
                ..
            }
        }
    ));

    let two_sided = extent_extrusion(
        1,
        false,
        &[(27, 72), (28, 71), (29, 22)],
        vec![enum_property(22, PmDcFeatureEnumFamily::Extent, 1)],
    )
    .expect("two-sided");
    assert!(matches!(
        extrude_definition(&two_sided).0,
        ExtrudeExtent::TwoSided {
            first: ExtrudeSide {
                termination: LinearTermination::Blind { length: first },
                ..
            },
            second: ExtrudeSide {
                termination: LinearTermination::Blind { length: second },
                ..
            },
        } if first.value_from(&two_sided.1).expect("bound first length").get() == 12.0 && second.value_from(&two_sided.1).expect("bound second length").get() == 3.0
    ));
    assert!(extent_extrusion(
        1,
        true,
        &[(27, 72), (28, 71), (29, 22)],
        vec![enum_property(22, PmDcFeatureEnumFamily::Extent, 1)],
    )
    .is_none());
}

fn push_f64s(bytes: &mut Vec<u8>, values: &[f64]) {
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

#[test]
fn parses_kernel_entity_link_work_plane_termination_flag_and_surface() {
    let mut link = Vec::new();
    link.extend_from_slice(&0u32.to_le_bytes());
    link.extend_from_slice(&0x1a6u16.to_le_bytes());
    link.extend_from_slice(&107u32.to_le_bytes());
    link.push(4);
    link.extend_from_slice(&0x8000_013du32.to_le_bytes());
    link.extend_from_slice(&3u32.to_le_bytes());
    link.extend_from_slice(&38u32.to_le_bytes());
    let parsed = parse(&link, |_, source| {
        crate::feature::parse_kernel_entity_link(source, 19).expect("kernel entity link")
    });
    assert_eq!(parsed.selection_source, 107);
    assert_eq!(parsed.body.index(), 0x13d);
    assert_eq!(
        (parsed.entity_id, parsed.entity_type, parsed.state),
        (3, 38, 4)
    );

    let frame = [1.5, 1.2, 0.3, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    for (version, marker, extended) in [(19, false, false), (19, true, false), (22, true, true)] {
        let mut plane = content(9);
        plane.extend_from_slice(&0x80_8000u32.to_le_bytes());
        plane.extend_from_slice(&[0; 8]);
        if extended {
            plane.extend_from_slice(&0u32.to_le_bytes());
        }
        if marker {
            plane.extend_from_slice(&crate::feature::WORK_PLANE_FRAME_MARKER.to_le_bytes());
        }
        plane.extend_from_slice(&0u32.to_le_bytes());
        push_f64s(&mut plane, &frame);
        let parsed = parse(&plane, |_, source| {
            crate::feature::parse_work_plane(source, version).expect("work plane")
        });
        assert_eq!(parsed.marker, marker);
        assert_eq!(parsed.values.len(), if extended { 4 } else { 3 });
        assert_eq!(parsed.origin[0].get(), 1.5);
        assert_eq!(parsed.u_axis[0].get(), -1.0);
        assert_eq!(parsed.v_axis[2].get(), 1.0);
    }

    let mut flag = content(10);
    flag.push(1);
    let parsed = parse(&flag, |ctx, source| {
        crate::feature::parse_termination_flag(ctx, source, 19).expect("termination flag")
    });
    assert_eq!(
        parsed.kind,
        PmDcFeaturePropertyKind::TerminationFlag { value: 1 }
    );

    let mut surface = content(11);
    surface.extend_from_slice(&[0; 12]);
    surface.push(1);
    surface.extend_from_slice(&crate::feature::PLANE_SURFACE_FORM.to_le_bytes());
    surface.extend_from_slice(&0u32.to_le_bytes());
    push_f64s(&mut surface, &frame);
    let parsed = parse(&surface, |_, source| {
        crate::feature::parse_surface(source, 19).expect("surface")
    })
    .expect("plane surface");
    assert!(matches!(
        parsed.kind,
        PmDcFeaturePropertyKind::PlaneSurface {
            state: 1,
            surface_form: 25,
            ..
        }
    ));
    let mut cylinder = content(12);
    cylinder.extend_from_slice(&[0; 12]);
    cylinder.push(1);
    cylinder.extend_from_slice(&29u32.to_le_bytes());
    cylinder.extend_from_slice(&1u32.to_le_bytes());
    push_f64s(&mut cylinder, &frame);
    assert!(parse(&cylinder, |_, source| crate::feature::parse_surface(
        source, 19
    ))
    .expect("surface form")
    .is_none());
}

fn assert_native_profile_validator(
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &crate::feature::ProjectionIndex<'_>,
    feature: Option<&Feature>,
) {
    use crate::feature::extrusion;
    let feature = feature.expect("extrusion with native profile");
    let definition = feature.evaluation.definition();
    assert!(extrusion::has_native_profile(definition));
    assert!(extrusion::matches(index, source, label, definition));
    let mut changed = definition.clone();
    let FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) = &mut changed
    else {
        panic!("extrusion fixture");
    };
    *profile =
        cadmpeg_ir::features::ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::Native(
            "inventor:pmdc:feature-property#generated-999".into(),
        ));
    assert!(!extrusion::matches(index, source, label, &changed));
    let mut changed = definition.clone();
    let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = &mut changed else {
        panic!("extrusion fixture");
    };
    *op = if *op == BooleanOp::Cut {
        BooleanOp::Join
    } else {
        BooleanOp::Cut
    };
    assert!(!extrusion::matches(index, source, label, &changed));
    let mut changed = definition.clone();
    let FeatureDefinition::Operation(FeatureOperation::Extrude { direction, .. }) = &mut changed
    else {
        panic!("extrusion fixture");
    };
    *direction = ExtrudeDirection::default();
    assert!(!extrusion::matches(index, source, label, &changed));
    let mut changed = definition.clone();
    let FeatureDefinition::Operation(FeatureOperation::Extrude { start, .. }) = &mut changed else {
        panic!("extrusion fixture");
    };
    *start = cadmpeg_ir::features::ExtrudeStart::ProfilePlane {};
    if changed != *definition {
        assert!(!extrusion::matches(index, source, label, &changed));
    }
    let mut changed = definition.clone();
    let FeatureDefinition::Operation(FeatureOperation::Extrude { extent, .. }) = &mut changed
    else {
        panic!("extrusion fixture");
    };
    *extent = ExtrudeExtent::OneSided {
        side: ExtrudeSide {
            termination: LinearTermination::ThroughAll {},
            draft: None,
        },
    };
    if changed != *definition {
        assert!(!extrusion::matches(index, source, label, &changed));
    }
    assert_eq!(
        extrusion::profile_loss().code.local_code(),
        "feature.extrusion-profile-native"
    );
    assert_eq!(
        extrusion::operand_loss().code.local_code(),
        "feature.extrusion-operands-native"
    );
}

#[test]
fn native_profile_extrusion_forms_and_validator_operands() {
    let forms = [
        (1, false, Vec::new(), Vec::new()),
        (2, true, Vec::new(), Vec::new()),
        (4, false, Vec::new(), Vec::new()),
        (5, false, Vec::new(), Vec::new()),
        (5, true, Vec::new(), Vec::new()),
        (7, false, vec![(11, 40)], Vec::new()),
        (6, false, vec![(8, 41), (11, 40)], Vec::new()),
        (
            7,
            false,
            vec![(13, 20), (18, 21)],
            vec![
                test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
                plane_surface(21),
            ],
        ),
        (
            6,
            false,
            vec![(10, 20), (17, 21), (11, 40)],
            vec![
                test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
                plane_surface(21),
            ],
        ),
        (1, false, vec![(5, 73)], Vec::new()),
        (
            1,
            false,
            vec![(3, 20)],
            vec![test_property(
                20,
                PmDcFeaturePropertyKind::Boolean {
                    name: String::new(),
                    name_value: 0,
                    value: true,
                },
            )],
        ),
        (
            1,
            false,
            vec![(27, 72), (28, 71), (29, 22)],
            vec![enum_property(22, PmDcFeatureEnumFamily::Extent, 1)],
        ),
    ];
    for second_extent in [4, 5] {
        configured_extent_extrusion(
            1,
            false,
            &[(27, 72), (28, 71), (29, 22)],
            vec![enum_property(
                22,
                PmDcFeatureEnumFamily::Extent,
                second_extent,
            )],
            |index| index.sketch_ids.clear(),
            assert_native_profile_validator,
        )
        .expect("opposite-side termination");
    }
    for operation in [1, 2, 4] {
        configured_extent_extrusion(
            1,
            false,
            &[(0, 20)],
            vec![test_property(
                20,
                PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::PartOperation,
                    type_value: 5,
                    value: operation,
                },
            )],
            |index| index.sketch_ids.clear(),
            assert_native_profile_validator,
        )
        .expect("operation with native profile");
    }
    for (extent, symmetric, slots, extra) in forms {
        let projected = configured_extent_extrusion(
            extent,
            symmetric,
            &slots,
            extra,
            |index| index.sketch_ids.clear(),
            assert_native_profile_validator,
        );
        let FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) = projected
            .expect("native profile form")
            .0
            .evaluation
            .definition()
            .clone()
        else {
            panic!("extrusion fixture");
        };
        assert_eq!(
            profile,
            cadmpeg_ir::features::ProfileRef::Planar(
                cadmpeg_ir::features::PlanarProfileRef::Native(
                    "inventor:pmdc:feature-property#generated-2".into()
                )
            )
        );
    }
}

#[test]
fn native_profile_extrusion_refuses_unresolved_participants_and_targets() {
    for slots in [&[(13, 20), (18, 21)][..], &[(11, 40), (13, 20)][..]] {
        assert!(configured_extent_extrusion(
            7,
            false,
            slots,
            vec![
                test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 0 }),
                plane_surface(21)
            ],
            |index| index.sketch_ids.clear(),
            |_, _, _, feature| assert!(feature.is_none())
        )
        .is_none());
    }
    assert!(configured_extent_extrusion(
        1,
        false,
        &[],
        Vec::new(),
        |index| {
            index.sketch_ids.clear();
            index.sketches.clear();
        },
        |_, _, _, feature| assert!(feature.is_none())
    )
    .is_none());
    assert!(configured_extent_extrusion(
        1,
        false,
        &[],
        Vec::new(),
        |index| {
            index.sketch_ids.clear();
            index.entity_style_links.clear();
        },
        |_, _, _, feature| assert!(feature.is_none())
    )
    .is_none());
}

#[test]
fn native_profile_requires_the_zero_selection_control() {
    assert!(configured_extent_extrusion(
        1,
        false,
        &[(1, 22), (23, 22)],
        vec![
            test_property(
                22,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::BoundaryPatch,
                    items: reference_list(&[24])
                }
            ),
            test_property(
                23,
                PmDcFeaturePropertyKind::ProfileSelection {
                    entity_link: reference(52),
                    value: 1
                }
            ),
        ],
        |index| index.sketch_ids.clear(),
        |_, _, _, feature| assert!(feature.is_none())
    )
    .is_none());
    assert!(configured_extent_extrusion(
        1,
        false,
        &[(1, 22), (23, 22)],
        vec![
            test_property(
                22,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::BoundaryPatch,
                    items: reference_list(&[4, 24])
                }
            ),
            test_property(
                23,
                PmDcFeaturePropertyKind::ProfileSelection {
                    entity_link: reference(52),
                    value: 1
                }
            ),
        ],
        |index| index.sketch_ids.clear(),
        |_, _, _, feature| assert!(feature.is_none())
    )
    .is_none());
}

#[test]
fn native_profile_direction_controls_are_pinned() {
    use crate::feature::extrusion::native_direction_controls;
    assert!(native_direction_controls(0, 0.0, None));
    assert!(native_direction_controls(0, 0.0, Some(0)));
    assert!(!native_direction_controls(1, 0.0, None));
    assert!(!native_direction_controls(0, 1.0, None));
    assert!(!native_direction_controls(0, 0.0, Some(1)));
}

#[test]
fn native_profile_extrusion_refuses_a_nonplane_target() {
    assert!(configured_extent_extrusion(
        7,
        false,
        &[(13, 20), (18, 21)],
        vec![
            test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
            test_property(
                21,
                PmDcFeaturePropertyKind::SurfaceBody {
                    body: reference(30)
                }
            ),
        ],
        |index| index.sketch_ids.clear(),
        |_, _, _, feature| assert!(feature.is_none())
    )
    .is_none());
}

#[test]
fn native_profile_keeps_the_complete_boundary_patch() {
    configured_extent_extrusion(
        1,
        false,
        &[(1, 22), (23, 22)],
        vec![
            test_property(
                22,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::BoundaryPatch,
                    items: reference_list(&[4, 25]),
                },
            ),
            test_property(
                24,
                PmDcFeaturePropertyKind::ProfileSelection {
                    entity_link: reference(52),
                    value: 0,
                },
            ),
        ],
        |index| index.sketch_ids.clear(),
        |source, label, index, feature| {
            assert_native_profile_validator(source, label, index, feature);
            let FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) = feature
                .as_ref()
                .expect("multiple selections")
                .evaluation
                .definition()
            else {
                panic!("extrusion fixture");
            };
            assert_eq!(
                *profile,
                cadmpeg_ir::features::ProfileRef::Planar(
                    cadmpeg_ir::features::PlanarProfileRef::Native(
                        "inventor:pmdc:feature-property#generated-22".into()
                    )
                )
            );
        },
    )
    .expect("complete native patch");
}

#[test]
fn native_profile_refuses_empty_and_duplicate_selections() {
    for members in [&[][..], &[4, 4][..]] {
        assert!(configured_extent_extrusion(
            1,
            false,
            &[(1, 22), (23, 22)],
            vec![test_property(
                22,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::BoundaryPatch,
                    items: reference_list(members)
                }
            ),],
            |index| index.sketch_ids.clear(),
            |_, _, _, feature| assert!(feature.is_none())
        )
        .is_none());
    }
}

#[test]
fn native_profile_refuses_unequal_patch_references_and_zero_blind_length() {
    assert!(configured_extent_extrusion(
        1,
        false,
        &[(23, 22)],
        vec![test_property(
            22,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BoundaryPatch,
                items: reference_list(&[4])
            }
        ),],
        |index| index.sketch_ids.clear(),
        |_, _, _, feature| assert!(feature.is_none())
    )
    .is_none());
    assert!(configured_extent_extrusion(
        1,
        false,
        &[(4, 74)],
        Vec::new(),
        |index| index.sketch_ids.clear(),
        |_, _, _, feature| assert!(feature.is_none())
    )
    .is_none());
}

#[test]
fn native_profile_retains_signed_taper_on_each_side() {
    let projected = configured_extent_extrusion(
        1,
        false,
        &[(5, 73), (27, 72), (28, 75), (29, 22)],
        vec![enum_property(22, PmDcFeatureEnumFamily::Extent, 1)],
        |index| index.sketch_ids.clear(),
        assert_native_profile_validator,
    )
    .expect("two tapered sides");
    let ExtrudeExtent::TwoSided { first, second } = extrude_definition(&projected).0 else {
        panic!("two-sided fixture");
    };
    assert_eq!(
        first
            .draft
            .as_ref()
            .expect("first draft")
            .value_from(&projected.1)
            .expect("bound first draft")
            .get(),
        0.2
    );
    assert_eq!(
        second
            .draft
            .as_ref()
            .expect("second draft")
            .value_from(&projected.1)
            .expect("bound second draft")
            .get(),
        -0.2
    );
}

#[test]
fn native_profile_direction_and_operation_values_are_exact() {
    for (value, expected) in [
        (1, BooleanOp::NewBody),
        (2, BooleanOp::Cut),
        (3, BooleanOp::Join),
        (4, BooleanOp::Intersect),
    ] {
        let feature = configured_extent_extrusion(
            1,
            false,
            &[(0, 22), (3, 23)],
            vec![
                test_property(
                    22,
                    PmDcFeaturePropertyKind::Enumeration {
                        family: PmDcFeatureEnumFamily::PartOperation,
                        type_value: 5,
                        value,
                    },
                ),
                test_property(
                    23,
                    PmDcFeaturePropertyKind::Boolean {
                        name: String::new(),
                        name_value: 0,
                        value: true,
                    },
                ),
            ],
            |index| index.sketch_ids.clear(),
            assert_native_profile_validator,
        )
        .expect("reversed extrusion");
        let FeatureDefinition::Operation(FeatureOperation::Extrude { direction, op, .. }) =
            feature.0.evaluation.definition()
        else {
            panic!("extrusion fixture");
        };
        assert_eq!(*op, expected);
        assert_eq!(
            *direction,
            ExtrudeDirection::Explicit {
                vector: cadmpeg_ir::features::FeatureDirection3::Z_AXIS.reversed(),
                source: Some(cadmpeg_ir::features::ExtrusionDirectionSource::Custom {}),
            }
        );
    }
}

#[test]
fn native_profile_validator_requires_the_patch_and_selection_record_types() {
    configured_extent_extrusion(
        1,
        false,
        &[],
        Vec::new(),
        |index| index.sketch_ids.clear(),
        |source, label, index, feature| {
            assert_native_profile_validator(source, label, index, feature);
            for ordinal in [2, 3] {
                let mut wrong = (*index
                    .properties
                    .get(&("generated", ordinal))
                    .expect("property key")
                    .expect("unique property"))
                .clone();
                wrong.identity.type_id =
                    crate::record_identity::RecordTypeId::from_bytes(crate::feature::BOOLEAN_TYPE);
                let mut changed = crate::feature::ProjectionIndex {
                    properties: index.properties.clone(),
                    sketches: index.sketches.clone(),
                    sketch_ids: index.sketch_ids.clone(),
                    directions: index.directions.clone(),
                    entity_style_links: index.entity_style_links.clone(),
                    ..crate::feature::ProjectionIndex::default()
                };
                assert!(
                    crate::feature::extrusion::native_profile(source, label, &changed).is_some()
                );
                changed
                    .properties
                    .insert(("generated", ordinal), Some(&wrong));
                assert!(
                    crate::feature::extrusion::native_profile(source, label, &changed).is_none()
                );
            }
        },
    )
    .expect("native profile fixture");
}
