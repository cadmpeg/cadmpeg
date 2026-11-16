// SPDX-License-Identifier: Apache-2.0
//! Extrusion extents and the records their targets use.

use super::*;
use crate::feature::extrusion::Output;

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
    let LinearTermination::ToPlane {
        plane, native: id, ..
    } = termination
    else {
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
) -> Option<Feature> {
    configured_extent_extrusion(extent, symmetric, slots, extra, |_| {}, |_, _, _, _| {})
}

const IDENTITY_MATRIX: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

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
) -> Option<Feature> {
    placed_extent_extrusion(
        extent,
        symmetric,
        slots,
        extra,
        IDENTITY_MATRIX,
        configure,
        check,
    )
}

/// An extent fixture whose consumed sketch is placed by `matrix`, in
/// centimetres. The sketch direction record and the feature direction record
/// are the same +z direction unless the slots say otherwise.
fn placed_extent_extrusion(
    extent: u16,
    symmetric: bool,
    slots: &[(usize, u32)],
    extra: Vec<PmDcFeatureProperty>,
    matrix: [[f64; 4]; 4],
    configure: impl FnOnce(&mut crate::feature::ProjectionIndex<'_>),
    check: impl FnOnce(
        &PmDcFeature,
        &PmDcFeatureLabel,
        &crate::feature::ProjectionIndex<'_>,
        Option<&Feature>,
    ),
) -> Option<Feature> {
    build_extent_extrusion(
        ExtentCase {
            output: Output::Solid,
            operation: 3,
            extent,
            symmetric,
            solid_input: SolidInput::None,
        },
        slots,
        extra,
        matrix,
        configure,
        check,
    )
}

/// The body kind, part operation, extent, and symmetric Boolean of an extent
/// fixture.
#[derive(Clone, Copy)]
struct ExtentCase {
    output: Output,
    operation: u16,
    extent: u16,
    symmetric: bool,
    /// A new-body solid extrusion stores the slot-14 wrapper as its result.
    solid_input: SolidInput,
}

/// Where the solid extrusion that stores the slot-14 wrapper stands in the
/// history relative to the extrusion under test.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SolidInput {
    None,
    Earlier,
    Later,
}

/// History positions of the extrusion under test and of the solid extrusion
/// that precedes or follows it.
fn test_positions<'a>(
    feature: &'a PmDcFeature,
    prior: Option<&'a PmDcFeature>,
    input: SolidInput,
) -> std::collections::HashMap<(&'a str, u32), u64> {
    let key = |feature: &'a PmDcFeature| {
        (
            feature.identity.segment_token.as_str(),
            feature.identity.record_ordinal,
        )
    };
    let mut positions = std::collections::HashMap::new();
    positions.insert(key(feature), 1);
    if let Some(prior) = prior {
        positions.insert(key(prior), if input == SolidInput::Later { 2 } else { 0 });
    }
    positions
}

/// The extent fixture of either body kind. A surface extrusion has the
/// surface part operation, its result in slot 16, and a null slot 26.
fn build_extent_extrusion(
    case: ExtentCase,
    slots: &[(usize, u32)],
    extra: Vec<PmDcFeatureProperty>,
    matrix: [[f64; 4]; 4],
    configure: impl FnOnce(&mut crate::feature::ProjectionIndex<'_>),
    check: impl FnOnce(
        &PmDcFeature,
        &PmDcFeatureLabel,
        &crate::feature::ProjectionIndex<'_>,
        Option<&Feature>,
    ),
) -> Option<Feature> {
    let ExtentCase {
        output,
        operation,
        extent,
        symmetric,
        solid_input,
    } = case;
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
            transform: reference(91),
            direction: reference(61),
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
    let sideways = Located::new(
        crate::sketch::PmDcDirectionPayload {
            save_version_major: 19,
            header: test_header(),
            entity_flags: 0,
            parameter: cadmpeg_ir::scalar::FiniteReal::ZERO,
            extension: None,
            direction: [
                cadmpeg_ir::scalar::FiniteReal::ONE,
                cadmpeg_ir::scalar::FiniteReal::ZERO,
                cadmpeg_ir::scalar::FiniteReal::ZERO,
            ],
        },
        crate::record_identity::RecordTypeId::try_from(
            "40df52ced011d0d20008ccbc0663dc09".to_owned(),
        )
        .expect("test GUID"),
        segment(),
        62,
    );
    let mut directions = vec![direction, sideways];
    for (ordinal, flags, parameter, extension, vector) in [
        (63, 1, 0.0, None, [0.0, 0.0, 1.0]),
        (64, 0, 1.0, None, [0.0, 0.0, 1.0]),
        (65, 0, 0.0, Some(1), [0.0, 0.0, 1.0]),
        (66, 0, 0.0, Some(0), [0.0, 0.0, 1.0]),
        (67, 0, 0.0, None, [1.0, 0.0, 1.0]),
    ] {
        let mut payload = (*directions[0]).clone();
        payload.entity_flags = flags;
        payload.parameter = cadmpeg_ir::scalar::FiniteReal::new(parameter).expect("finite");
        payload.extension = extension;
        payload.direction = reals(vector);
        directions.push(Located::new(
            payload,
            directions[0].identity.type_id.clone(),
            segment(),
            ordinal,
        ));
    }
    let transform = Located::new(
        crate::sketch::PmDcTransformPayload {
            save_version_major: 19,
            header: test_header(),
            prefix_present: false,
            matrix: crate::compact_matrix::CompactMatrix::try_from_rows(0, 0, matrix)
                .expect("finite test matrix"),
        },
        crate::record_identity::RecordTypeId::try_from(
            "184d8790d011f8d10008cabc0663dc09".to_owned(),
        )
        .expect("test GUID"),
        segment(),
        90,
    );
    let mut properties = vec![
        test_property(
            1,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::PartOperation,
                type_value: 5,
                value: operation,
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
        if output == Output::Surface {
            (16, 8)
        } else {
            (26, 7)
        },
    ];
    all_slots.extend_from_slice(slots);
    let feature = test_feature(100, 30, &all_slots);
    let label = test_label(
        100,
        5,
        if output == Output::Surface {
            SURFACE_EXTRUSION_CLASS_ID
        } else {
            EXTRUSION_CLASS_ID
        },
        &[51],
    );
    // An earlier new-body solid extrusion whose result collection holds the
    // slot-14 wrapper.
    let wrapper = all_slots
        .iter()
        .find_map(|(slot, ordinal)| (*slot == 14).then_some(*ordinal));
    let prior = (solid_input != SolidInput::None).then(|| {
        let wrapper = wrapper.expect("solid input names slot 14");
        let mut operation = test_property(
            80,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::PartOperation,
                type_value: 5,
                value: 1,
            },
        );
        operation.identity.type_id =
            crate::record_identity::RecordTypeId::from_bytes(crate::feature::PART_OPERATION_TYPE);
        properties.push(operation);
        properties.push(test_property(
            81,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[wrapper + 1]),
            },
        ));
        (
            test_feature(101, 30, &[(0, 80), (26, 81)]),
            test_label(101, 6, EXTRUSION_CLASS_ID, &[51]),
        )
    });
    let planes = vec![work_plane(40), work_plane(41)];
    let mut index = test_projection_index(
        &properties,
        &raw_parameters,
        &neutral_parameters,
        std::slice::from_ref(&raw_sketch),
        std::slice::from_ref(&neutral_sketch),
        &directions,
        std::slice::from_ref(&transform),
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
    if let Some((prior_feature, _)) = &prior {
        index.positions = test_positions(&feature, Some(prior_feature), solid_input);
        index.solid_wrappers = crate::feature::extrusion::solid_result_wrappers(
            std::slice::from_ref(prior_feature),
            |_| true,
            &index.properties,
            &index.positions,
        );
    }
    configure(&mut index);
    let mut losses = Vec::new();
    let projected = match output {
        Output::Solid => {
            crate::feature::extrusion::project(&ctx, &feature, &label, &index, &mut losses)
        }
        Output::Surface => {
            crate::feature::extrusion::project_surface(&ctx, &feature, &label, &index, &mut losses)
        }
    }
    .map(|projected| projected.expect("extrusion projection"))
    .map(|(feature, result)| {
        if output == Output::Surface {
            let wrapper = test_property(
                8,
                PmDcFeaturePropertyKind::SurfaceBody {
                    body: reference(30),
                },
            )
            .id();
            assert_eq!(
                result
                    .bodies()
                    .iter()
                    .map(cadmpeg_core::text::NonBlankString::as_str)
                    .collect::<Vec<_>>(),
                [wrapper.as_str()]
            );
        }
        feature
    });
    let face_targets = if projected.is_some() && matches!(extent, 6 | 7) {
        let slots = feature.properties.references();
        usize::from(slots[13].index() != 0 && slots[18].index() != 0)
            + usize::from(extent == 6 && slots[10].index() != 0 && slots[17].index() != 0)
    } else {
        0
    };
    assert_eq!(
        losses
            .iter()
            .filter(|note| note.code.local_code()
                == crate::loss::InventorLossCode::FeatureExtrusionTargetFaceNative.code())
            .count(),
        face_targets
    );
    losses.retain(|note| {
        note.code.local_code()
            != crate::loss::InventorLossCode::FeatureExtrusionTargetFaceNative.code()
    });
    if output == Output::Surface {
        let profile_losses = losses
            .iter()
            .filter(|note| {
                note.code.local_code()
                    == crate::loss::InventorLossCode::FeatureExtrusionProfileNative.code()
            })
            .count();
        let polarity_losses = losses
            .iter()
            .filter(|note| {
                note.code.local_code()
                    == crate::loss::InventorLossCode::FeatureSurfaceExtrusionPolarityAssumed.code()
            })
            .count();
        let bound_losses = losses
            .iter()
            .filter(|note| {
                note.code.local_code()
                    == crate::loss::InventorLossCode::FeatureSurfaceExtrusionSolidBoundAssumed
                        .code()
            })
            .count();
        assert_eq!(
            losses.len(),
            profile_losses + polarity_losses + bound_losses
        );
        assert_eq!(
            bound_losses,
            usize::from(projected.is_some() && solid_input != SolidInput::None)
        );
        assert_eq!(profile_losses, usize::from(projected.is_some()));
        assert_eq!(polarity_losses, usize::from(projected.is_some()));
    } else if projected.as_ref().is_some_and(|feature| {
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
    if let Some(projected) = projected.as_ref().filter(|feature| {
        output == Output::Surface
            || !crate::feature::extrusion::has_native_profile(feature.evaluation.definition())
    }) {
        let mut model = cadmpeg_ir::document::Model::default();
        model.parameters.extend(neutral_parameters.iter().cloned());
        model.sketches.push(neutral_sketch.clone());
        let link = entity_link_record();
        let mut validation_features = vec![feature.clone()];
        let mut validation_labels = vec![label.clone()];
        if let Some((prior_feature, prior_label)) = &prior {
            validation_features.push(prior_feature.clone());
            validation_labels.push(prior_label.clone());
        }
        let validation = crate::feature::extrusion::ValidationIndex::new(
            &crate::feature::extrusion::ValidationRecords {
                features: &validation_features,
                labels: &validation_labels,
                properties: &properties,
                parameters: &raw_parameters,
                sketches: std::slice::from_ref(&raw_sketch),
                directions: &directions,
                transforms: std::slice::from_ref(&transform),
                planes: &planes,
                entity_links: std::slice::from_ref(&link),
                kernel_links: &[],
                model: &model,
                positions: &index.positions,
            },
        );
        assert!(validation.matches(&feature, &label, projected.evaluation.definition()));
    }
    check(&feature, &label, &index, projected.as_ref());
    projected
}

/// The entity link that the profile selection of the extent fixture names.
fn entity_link_record() -> PmDcEntityStyleLink {
    Located::new(
        PmDcEntityStyleLinkPayload {
            save_version_major: 19,
            header: PmDcLinkedHeader {
                header_value: 0,
                header_id: 0,
                values: [0; 2],
                flags_extension: None,
                owner: reference(0),
                parent: reference(0),
                next: reference(0),
            },
            value: 0,
            associative_id: 1,
            entity_type: 1,
        },
        crate::record_identity::RecordTypeId::from_bytes(ENTITY_STYLE_LINK_TYPE),
        segment(),
        51,
    )
}

fn extrude_definition(feature: &Feature) -> (&ExtrudeExtent, &cadmpeg_ir::features::ExtrudeStart) {
    match feature.evaluation.definition() {
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
    plane_surface_with(ordinal, None)
}

fn plane_surface_with(ordinal: u32, extension: Option<u32>) -> PmDcFeatureProperty {
    let mut surface = test_property(
        ordinal,
        PmDcFeaturePropertyKind::PlaneSurface {
            references: [reference(0); 3],
            extension,
            state: 1,
            surface_form: PLANE_SURFACE_FORM,
            value: 0,
            origin: reals([0.0, 0.0, 4.0]),
            u_axis: reals([0.0, 1.0, 0.0]),
            v_axis: reals([0.0, 0.0, 1.0]),
        },
    );
    surface.edit(|payload| payload.save_version_major = if extension.is_some() { 22 } else { 19 });
    surface
}

fn typed_extent_extrusion(version: u8, type_value: i16) -> Option<Feature> {
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
        } if length.get() == 12.0
    ));
    for version in [22, 23, 25, 28, 29, 30, 31] {
        let actual = typed_extent_extrusion(version, 12).expect("type-12 distance extrusion");
        assert_eq!(
            actual.evaluation.definition(),
            expected.evaluation.definition()
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
        } if length.get() == 12.0
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
    let cadmpeg_ir::features::ExtrudeStart::FromPlane { plane, native, .. } = start else {
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
        } if first.get() == 12.0 && second.get() == 3.0
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
    assert_eq!(first.draft.expect("first draft").get(), 0.2);
    assert_eq!(second.draft.expect("second draft").get(), -0.2);
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
            feature.evaluation.definition()
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

#[test]
fn face_target_requires_a_zero_surface_extension() {
    let face = |extension: Option<u32>| {
        extent_extrusion(
            7,
            false,
            &[(13, 20), (18, 21)],
            vec![
                test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
                plane_surface_with(21, extension),
            ],
        )
    };
    assert!(face(None).is_some());
    assert!(face(Some(0)).is_some());
    assert!(face(Some(1)).is_none());
}

#[test]
fn parses_plane_surface_extension_from_segment_version_22() {
    let frame = [1.5, 1.2, 0.3, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    for (version, stamps, extended) in [
        (19, &[][..], false),
        (23, &[-1][..], true),
        (25, &[-1, -1][..], true),
    ] {
        let mut surface = crate::test_support::test_fixtures::content_at(11, version, stamps);
        surface.extend_from_slice(&[0; 12]);
        if extended {
            surface.extend_from_slice(&0u32.to_le_bytes());
        }
        surface.push(1);
        surface.extend_from_slice(&crate::feature::PLANE_SURFACE_FORM.to_le_bytes());
        surface.extend_from_slice(&0u32.to_le_bytes());
        push_f64s(&mut surface, &frame);
        let parsed = parse(&surface, |_, source| {
            crate::feature::parse_surface(source, version).expect("surface")
        })
        .expect("plane surface");
        let PmDcFeaturePropertyKind::PlaneSurface {
            extension,
            surface_form,
            origin,
            ..
        } = parsed.kind
        else {
            panic!("plane surface kind");
        };
        assert_eq!(extension, extended.then_some(0));
        assert_eq!(surface_form, crate::feature::PLANE_SURFACE_FORM);
        assert_eq!(origin[0].get(), 1.5);

        // The same bytes read without the extension are not a plane.
        let mut cylinder = crate::test_support::test_fixtures::content_at(12, version, stamps);
        cylinder.extend_from_slice(&[0; 12]);
        if extended {
            cylinder.extend_from_slice(&0u32.to_le_bytes());
        }
        cylinder.push(1);
        cylinder.extend_from_slice(&29u32.to_le_bytes());
        cylinder.extend_from_slice(&1u32.to_le_bytes());
        push_f64s(&mut cylinder, &frame);
        assert!(parse(&cylinder, |_, source| crate::feature::parse_surface(
            source, version
        ))
        .expect("surface form")
        .is_none());
    }
}

fn work_point(ordinal: u32, origin: [f64; 3]) -> PmDcFeatureProperty {
    work_point_with(ordinal, 0, vec![0, 0], origin, [&[], &[]])
}

fn work_point_with(
    ordinal: u32,
    entity_flags: u32,
    values: Vec<u32>,
    origin: [f64; 3],
    lists: [&[u32]; 2],
) -> PmDcFeatureProperty {
    let mut point = test_property(
        ordinal,
        PmDcFeaturePropertyKind::WorkPoint {
            entity_flags,
            values,
            origin: reals(origin),
            references: lists.map(reference_list),
        },
    );
    point.identity.type_id =
        crate::record_identity::RecordTypeId::from_bytes(crate::feature::WORK_POINT_TYPE);
    point
}

/// The slot of a work point in the point fixtures: record 24.
const POINT_SLOTS: [(usize, u32); 1] = [(25, 24)];

fn point_extrusion(point: PmDcFeatureProperty, slots: &[(usize, u32)]) -> Option<Feature> {
    let mut all = POINT_SLOTS.to_vec();
    all.extend_from_slice(slots);
    let expected = extent_extrusion(
        7,
        false,
        &POINT_SLOTS,
        vec![work_point(24, [0.0, 0.0, 2.5])],
    )
    .expect("canonical point extrusion");
    configured_extent_extrusion(
        7,
        false,
        &all,
        vec![point],
        |_| {},
        |source, label, index, projected| {
            if projected.is_none() {
                assert!(!crate::feature::extrusion::matches(
                    index,
                    source,
                    label,
                    expected.evaluation.definition()
                ));
            }
        },
    )
}

#[test]
fn work_point_extent_ends_at_a_resolved_plane() {
    // The point lies 2.5 cm ahead of the profile plane along +z.
    let point = work_point(24, [0.0, 0.0, 2.5]);
    let native = point.id();
    let feature = configured_extent_extrusion(
        7,
        false,
        &POINT_SLOTS,
        vec![point],
        |_| {},
        |source, label, index, projected| {
            let definition = projected.expect("projected").evaluation.definition();
            assert!(crate::feature::extrusion::matches(
                index, source, label, definition
            ));
            let mut changed = definition.clone();
            let FeatureDefinition::Operation(FeatureOperation::Extrude { extent, .. }) =
                &mut changed
            else {
                panic!("extrusion fixture");
            };
            *extent = ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::ThroughAll {},
                    draft: None,
                },
            };
            assert!(!crate::feature::extrusion::matches(
                index, source, label, &changed
            ));
            let FeatureDefinition::Operation(FeatureOperation::Extrude { extent, .. }) =
                &mut changed
            else {
                panic!("extrusion fixture");
            };
            *extent = ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::ToVertex {
                        vertex: cadmpeg_ir::features::VertexSelection::Native(
                            cadmpeg_ir::features::SelectionReference::new(native.clone())
                                .expect("native point reference"),
                        ),
                    },
                    draft: None,
                },
            };
            assert!(!crate::feature::extrusion::matches(
                index, source, label, &changed
            ));
        },
    )
    .expect("to work point");
    let ExtrudeExtent::OneSided { side } = extrude_definition(&feature).0 else {
        panic!("to-point extrusion is one-sided");
    };
    let LinearTermination::ToPlane {
        plane,
        native: actual,
        ..
    } = &side.termination
    else {
        panic!("resolved plane termination");
    };
    assert_eq!(actual.as_str(), native);
    assert_eq!(plane.origin().get(), Point3::new(0.0, 0.0, 25.0));
    assert_eq!(plane.normal().get(), Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(plane.u_axis().get(), Vector3::new(1.0, 0.0, 0.0));
    assert!(matches!(
        extrude_definition(&feature).1,
        cadmpeg_ir::features::ExtrudeStart::ProfilePlane {}
    ));
}

#[test]
fn work_point_extent_requires_a_point_ahead_of_the_profile_plane() {
    assert!(point_extrusion(work_point(24, [0.0, 0.0, 2.5]), &[]).is_some());
    // On the profile plane and behind it.
    assert!(point_extrusion(work_point(24, [1.0, 2.0, 0.0]), &[]).is_none());
    assert!(point_extrusion(work_point(24, [0.0, 0.0, -2.5]), &[]).is_none());
    // A true reverse Boolean turns the travel direction toward -z.
    let reverse = || {
        vec![test_property(
            20,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: true,
            },
        )]
    };
    for (z, projects) in [(2.5, false), (-2.5, true)] {
        let mut extra = reverse();
        extra.push(work_point(24, [0.0, 0.0, z]));
        assert_eq!(
            extent_extrusion(7, false, &[(25, 24), (3, 20)], extra).is_some(),
            projects,
            "reverse travel and point z {z}"
        );
    }
    // A travel direction that is not perpendicular to the profile plane.
    assert!(point_extrusion(work_point(24, [2.5, 0.0, 0.0]), &[(2, 62)]).is_none());
    // A sketch placed with a normal that disagrees with its direction record.
    assert!(placed_extent_extrusion(
        7,
        false,
        &POINT_SLOTS,
        vec![work_point(24, [0.0, 0.0, 2.5])],
        [
            [0.0, 0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        |_| {},
        |_, _, _, _| {},
    )
    .is_none());
    // The profile plane sits at z 1 cm, so the point at z 0.5 cm is behind it.
    let raised = |z: f64| {
        placed_extent_extrusion(
            7,
            false,
            &POINT_SLOTS,
            vec![work_point(24, [0.0, 0.0, z])],
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 1.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            |_| {},
            |_, _, _, _| {},
        )
    };
    assert!(raised(0.5).is_none());
    assert!(raised(1.5).is_some());
}

#[test]
fn work_point_extent_pins_every_slot_it_reads() {
    // Exactly one target form is present.
    for slot in [11, 13, 18] {
        assert!(point_extrusion(work_point(24, [0.0, 0.0, 2.5]), &[(slot, 40)]).is_none());
    }
    assert!(extent_extrusion(
        7,
        false,
        &[(25, 24), (13, 20), (18, 21)],
        vec![
            work_point(24, [0.0, 0.0, 2.5]),
            test_property(20, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
            plane_surface(21),
        ],
    )
    .is_none());
    // A plane or face target with a point slot is the mixed form.
    assert!(extent_extrusion(
        7,
        false,
        &[(25, 24), (11, 40)],
        vec![work_point(24, [0.0, 0.0, 2.5])]
    )
    .is_none());
    // The point record: zero flags, zero values, empty lists, plain record type.
    assert!(point_extrusion(
        work_point_with(24, 8192, vec![0, 0], [0.0, 0.0, 2.5], [&[], &[]]),
        &[]
    )
    .is_none());
    assert!(point_extrusion(
        work_point_with(24, 0, vec![0, 1], [0.0, 0.0, 2.5], [&[], &[]]),
        &[]
    )
    .is_none());
    assert!(point_extrusion(
        work_point_with(24, 0, vec![0, 0], [0.0, 0.0, 2.5], [&[7], &[]]),
        &[]
    )
    .is_none());
    assert!(point_extrusion(
        work_point_with(24, 0, vec![0, 0], [0.0, 0.0, 2.5], [&[], &[7]]),
        &[]
    )
    .is_none());
    assert!(point_extrusion(plane_surface(24), &[]).is_none());
    // A null point slot with a point record present is a missing target.
    assert!(extent_extrusion(7, false, &[], vec![work_point(24, [0.0, 0.0, 2.5])]).is_none());
    // The from-to extent has no point form.
    assert!(extent_extrusion(
        6,
        false,
        &[(25, 24), (8, 41)],
        vec![work_point(24, [0.0, 0.0, 2.5])]
    )
    .is_none());
}

#[test]
fn parses_work_point_across_header_and_value_count_forms() {
    let point = [0.5, -0.25, 1.75];
    let empty_list = [2u8, 0, 0, 0x30, 0, 0, 0, 0];
    for (version, stamps, extended) in [
        (19, &[][..], false),
        (21, &[][..], false),
        (22, &[][..], true),
        (23, &[-1][..], true),
        (25, &[-1, -1][..], true),
        (29, &[-1, -1][..], true),
    ] {
        let mut record = crate::test_support::test_fixtures::content_at(13, version, stamps);
        record.extend_from_slice(&0u32.to_le_bytes());
        record.extend_from_slice(&[0; 8]);
        if extended {
            record.extend_from_slice(&0u32.to_le_bytes());
        }
        push_f64s(&mut record, &point);
        record.extend_from_slice(&empty_list);
        record.extend_from_slice(&empty_list);
        let header_len = crate::test_support::test_fixtures::content_at(13, version, stamps).len();
        let parsed = parse(&record, |ctx, source| {
            crate::feature::parse_work_point(ctx, source, version).expect("work point")
        });
        let PmDcFeaturePropertyKind::WorkPoint {
            entity_flags,
            values,
            origin,
            references,
        } = parsed.kind
        else {
            panic!("work point kind");
        };
        assert_eq!(entity_flags, 0);
        assert_eq!(values.len(), if extended { 3 } else { 2 });
        assert_eq!(origin.map(cadmpeg_ir::scalar::FiniteReal::get), point);
        assert!(references.iter().all(|list| list.references().is_empty()));
        assert_eq!(
            record.len() - header_len,
            crate::layout::pm_dc_work_point_fields::LEN
                + if extended { 4 } else { 0 }
                + 2 * empty_list.len()
        );

        // A list that holds one reference parses and keeps it; the extrusion
        // target refuses it.
        let list = [
            2u8, 0, 0, 0x30, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0x80,
        ];
        let mut listed = record[..record.len() - 2 * empty_list.len()].to_vec();
        listed.extend_from_slice(&list);
        listed.extend_from_slice(&empty_list);
        let parsed = parse(&listed, |ctx, source| {
            crate::feature::parse_work_point(ctx, source, version).expect("work point with a list")
        });
        let PmDcFeaturePropertyKind::WorkPoint { references, .. } = parsed.kind else {
            panic!("work point kind");
        };
        assert_eq!(references[0].references().len(), 1);
        assert_eq!(references[0].references()[0].index(), 5);
        assert!(references[0].references()[0].qualified());
        assert!(references[1].references().is_empty());

        // A byte more or fewer is not a work point.
        for variant in [
            record[..record.len() - 1].to_vec(),
            [record.clone(), vec![0]].concat(),
        ] {
            assert!(parse(&variant, |ctx, source| {
                crate::feature::parse_work_point(ctx, source, version)
            })
            .is_err());
        }
    }
}

mod plane_controls;
mod surface;
mod work_point;
