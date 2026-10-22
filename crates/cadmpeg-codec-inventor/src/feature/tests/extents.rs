// SPDX-License-Identifier: Apache-2.0
//! Extrusion extents and the records their targets use.

use super::*;

fn work_plane(ordinal: u32) -> PmDcWorkPlane {
    Located::new(
        PmDcWorkPlanePayload {
            save_version_major: 19,
            header: test_header(),
            entity_flags: 0x80_8000,
            values: vec![0; 3],
            marker: false,
            origin: [cadmpeg_ir::scalar::FiniteReal::ZERO; 3],
            u_axis: [
                cadmpeg_ir::scalar::FiniteReal::ONE,
                cadmpeg_ir::scalar::FiniteReal::ZERO,
                cadmpeg_ir::scalar::FiniteReal::ZERO,
            ],
            v_axis: [
                cadmpeg_ir::scalar::FiniteReal::ZERO,
                cadmpeg_ir::scalar::FiniteReal::ONE,
                cadmpeg_ir::scalar::FiniteReal::ZERO,
            ],
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
    let raw_parameters = vec![raw_parameter(70), raw_parameter(71), raw_parameter(72)];
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
    project_extrusion(&ctx, &feature, &label, &index).map(|projected| {
        (
            projected.expect("extrusion projection").0,
            neutral_parameters,
        )
    })
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
            origin: [cadmpeg_ir::scalar::FiniteReal::ZERO; 3],
            u_axis: [
                cadmpeg_ir::scalar::FiniteReal::ONE,
                cadmpeg_ir::scalar::FiniteReal::ZERO,
                cadmpeg_ir::scalar::FiniteReal::ZERO,
            ],
            v_axis: [
                cadmpeg_ir::scalar::FiniteReal::ZERO,
                cadmpeg_ir::scalar::FiniteReal::ONE,
                cadmpeg_ir::scalar::FiniteReal::ZERO,
            ],
        },
    )
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
    assert!(matches!(
        extrude_definition(&to_plane).0,
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::ToFace {
                    face: cadmpeg_ir::features::FaceSelection::Native(native),
                    offset: None,
                },
                ..
            }
        } if native == &work_plane(40).id()
    ));

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
    assert!(matches!(
        extrude_definition(&to_face).0,
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::ToFace {
                    face: cadmpeg_ir::features::FaceSelection::Native(native),
                    ..
                },
                ..
            }
        } if native == &plane_surface(21).id()
    ));
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
    assert!(matches!(
        start,
        cadmpeg_ir::features::ExtrudeStart::FromFace {
            face: cadmpeg_ir::features::FaceSelection::Native(native),
            offset: None,
        } if native == &work_plane(41).id()
    ));
    assert!(matches!(
        extent,
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::ToFace { .. },
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
