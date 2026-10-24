// SPDX-License-Identifier: Apache-2.0
//! Hole forms, center selections, and extent projection.

use crate::feature::hole;
use crate::feature::tests::{
    inventory, neutral_parameter, raw_parameter, reference, reference_list, references, segment,
    test_feature, test_header, test_label, test_projection_index, test_property, with_record,
};
use crate::feature::{
    project_hole, PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureLabel, PmDcFeatureProperty,
    PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily, PmDcWorkPlane, PmDcWorkPlanePayload,
    HOLE_CLASS_ID, WORK_PLANE_TYPE,
};
use crate::pmdc::{PmDcReferenceList, PmDcU32List};
use crate::record_identity::Located;
use crate::test_support::test_fixtures::content;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    holes::{HoleKind, HolePlacement},
    DesignParameter, Feature, FeatureDefinition, FeatureOperation, LinearTermination,
    ParameterValue,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, Length};

const EPS_COORDINATE: f64 = 1e-12;
const POINT_TYPE: [u8; 16] = [
    0x35, 0xdf, 0x52, 0xce, 0xd0, 0x11, 0xd0, 0xd2, 0x00, 0x08, 0xcc, 0xbc, 0x06, 0x63, 0xdc, 0x09,
];
const DIRECTION_TYPE: [u8; 16] = [
    0x40, 0xdf, 0x52, 0xce, 0xd0, 0x11, 0xd0, 0xd2, 0x00, 0x08, 0xcc, 0xbc, 0x06, 0x63, 0xdc, 0x09,
];

struct HoleFixture {
    parameters: Vec<crate::design::PmDcParameter>,
    values: Vec<DesignParameter>,
    properties: Vec<PmDcFeatureProperty>,
    transform: crate::sketch::PmDcTransform,
    direction: crate::sketch::PmDcDirection,
    sketch: crate::sketch::PmDcSketch,
    points: Vec<crate::sketch::PmDcSketchEntity>,
    centers: PmDcFeatureProperty,
    plane: PmDcWorkPlane,
    faces: PmDcFeatureProperty,
    feature: PmDcFeature,
    label: PmDcFeatureLabel,
}

impl HoleFixture {
    fn new(form: u16, extent_type: i16, extent: u16) -> Self {
        let parameters = (70..76).map(raw_parameter).collect::<Vec<_>>();
        let values = [
            ParameterValue::Length(Length::new(2.0).expect("finite test length")),
            ParameterValue::Length(Length::new(15.0).expect("finite test length")),
            ParameterValue::Length(Length::new(6.0).expect("finite test length")),
            ParameterValue::Length(Length::new(1.0).expect("finite test length")),
            ParameterValue::Angle(Angle::new(1.0).expect("finite test angle")),
            ParameterValue::Angle(Angle::new(0.0).expect("finite test angle")),
        ]
        .into_iter()
        .zip(&parameters)
        .map(|(v, p)| neutral_parameter(p, v))
        .collect();
        let properties = vec![
            test_property(
                1,
                PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::Hole,
                    type_value: 3,
                    value: form,
                },
            ),
            test_property(
                2,
                PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::Extent,
                    type_value: extent_type,
                    value: extent,
                },
            ),
            test_property(
                3,
                PmDcFeaturePropertyKind::Boolean {
                    name: String::new(),
                    name_value: 0,
                    value: false,
                },
            ),
            test_property(
                4,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::ObjectCollection,
                    items: reference_list(&[6]),
                },
            ),
            test_property(
                5,
                PmDcFeaturePropertyKind::SurfaceBody {
                    body: reference(30),
                },
            ),
            test_property(6, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
        ];
        let transform = Located::new(
            crate::sketch::PmDcTransformPayload {
                save_version_major: 16,
                header: test_header(),
                prefix_present: false,
                matrix: crate::compact_matrix::CompactMatrix::try_from_rows(
                    0,
                    0,
                    [
                        [0.0, -1.0, 0.0, 1.0],
                        [1.0, 0.0, 0.0, 2.0],
                        [0.0, 0.0, 1.0, 3.0],
                        [0.0, 0.0, 0.0, 1.0],
                    ],
                )
                .expect("finite test matrix"),
            },
            crate::record_identity::RecordTypeId::from_bytes(crate::pmdc::inventor_id(0x9087_4d18)),
            segment(),
            60,
        );
        let direction = Located::new(
            crate::sketch::PmDcDirectionPayload {
                save_version_major: 16,
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
            crate::record_identity::RecordTypeId::from_bytes(DIRECTION_TYPE),
            segment(),
            61,
        );
        let sketch = Located::new(
            crate::sketch::PmDcSketchPayload {
                save_version_major: 16,
                header: test_header(),
                state: 0,
                count_value: 2,
                entities: PmDcReferenceList::new(
                    8,
                    Some(crate::pmdc::PmDcListMetadata::U16([2, 0])),
                    vec![reference(81), reference(82)],
                )
                .expect("sketch entity-array metadata"),
                transform: reference(61),
                direction: reference(62),
                values: [0; 2],
                auxiliary: None,
            },
            crate::record_identity::RecordTypeId::from_bytes(crate::pmdc::inventor_id(0x9087_4d11)),
            segment(),
            50,
        );
        let points = [[2.0_f64, -1.0_f64], [-3.0_f64, 4.0_f64]]
            .into_iter()
            .enumerate()
            .map(|(i, position)| {
                let mut bytes = content(0);
                bytes.extend(0_u32.to_le_bytes());
                bytes.extend(51_u32.to_le_bytes());
                for x in position {
                    bytes.extend(x.to_le_bytes());
                }
                for _ in 0..2 {
                    bytes.extend(2_u16.to_le_bytes());
                    bytes.extend(0x3000_u16.to_le_bytes());
                    bytes.extend(0_u32.to_le_bytes());
                }
                let parsed = with_record(
                    POINT_TYPE,
                    &bytes,
                    DecodePolicy::service(),
                    crate::sketch::inventory,
                )
                .expect("point inventory");
                let point = parsed
                    .entities
                    .into_iter()
                    .next()
                    .expect("parsed test point");
                Located::new(
                    (*point).clone(),
                    crate::record_identity::RecordTypeId::from_bytes(POINT_TYPE),
                    segment(),
                    80 + u32::try_from(i).expect("small test ordinal"),
                )
            })
            .collect();
        let centers = center_property(reference_list(&[81, 82]));
        let faces = test_property(
            41,
            PmDcFeaturePropertyKind::FaceItem {
                index_references: PmDcU32List::new(
                    2,
                    Some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                    vec![0x8000_0007],
                )
                .expect("face-list metadata"),
            },
        );
        let plane = Located::new(
            PmDcWorkPlanePayload {
                save_version_major: 16,
                header: test_header(),
                entity_flags: 0,
                values: vec![0; 3],
                marker: false,
                origin: [1.0, 2.0, 0.0].map(|v| {
                    cadmpeg_ir::scalar::FiniteReal::new(v).expect("finite plane coordinate")
                }),
                u_axis: [1.0, 0.0, 0.0]
                    .map(|v| cadmpeg_ir::scalar::FiniteReal::new(v).expect("finite plane axis")),
                v_axis: [0.0, 1.0, 0.0]
                    .map(|v| cadmpeg_ir::scalar::FiniteReal::new(v).expect("finite plane axis")),
            },
            crate::record_identity::RecordTypeId::from_bytes(WORK_PLANE_TYPE),
            segment(),
            42,
        );
        let feature = test_feature(
            100,
            30,
            &[
                (0, 1),
                (1, 70),
                (2, 71),
                (3, 72),
                (4, 73),
                (5, 74),
                (6, 75),
                (7, 40),
                (8, 60),
                (9, 2),
                (15, 6),
                (16, 61),
                (17, 3),
                (24, 4),
            ],
        );
        let label = test_label(100, 9, HOLE_CLASS_ID, &[51]);
        Self {
            parameters,
            values,
            properties,
            transform,
            direction,
            sketch,
            points,
            centers,
            plane,
            faces,
            feature,
            label,
        }
    }

    fn project(&self, slots: &[(usize, u32)]) -> Option<Feature> {
        let mut payload = (*self.feature).clone();
        let mut refs = payload.properties.references().to_vec();
        for &(slot, reference_index) in slots {
            refs[slot] = reference(reference_index);
        }
        payload.properties = PmDcReferenceList::new(
            2,
            Some(crate::pmdc::PmDcListMetadata::U32([
                u32::try_from(refs.len()).expect("small property list"),
                0,
            ])),
            refs,
        )
        .expect("property-list metadata");
        let feature = Located::new(
            payload,
            self.feature.identity.type_id.clone(),
            segment(),
            100,
        );
        let mut index = test_projection_index(
            &self.properties,
            &self.parameters,
            &self.values,
            std::slice::from_ref(&self.sketch),
            &[],
            std::slice::from_ref(&self.direction),
            std::slice::from_ref(&self.transform),
            &[],
        );
        let token = segment();
        index
            .properties
            .insert((token.as_str(), 40), Some(&self.centers));
        index
            .properties
            .insert((token.as_str(), 41), Some(&self.faces));
        index
            .work_planes
            .insert((token.as_str(), 42), Some(&self.plane));
        for point in &self.points {
            index
                .sketch_entities
                .insert((token.as_str(), point.identity.record_ordinal), Some(point));
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("decode context");
        project_hole(&ctx, &feature, &self.label, &index).map(|v| v.expect("hole projection").0)
    }
}

#[test]
fn sketch_centers_become_directed_model_space_placements() {
    let fixture = HoleFixture::new(0, 11, 5);
    let projected = fixture.project(&[]).expect("center placement projection");
    let FeatureDefinition::Operation(FeatureOperation::Hole {
        placements: Some(placements),
        ..
    }) = projected.evaluation.definition()
    else {
        panic!("hole");
    };
    assert_eq!(placements.len(), 2);
    for (placement, expected) in placements
        .iter()
        .zip([[20.0, 40.0, 30.0], [-30.0, -10.0, 30.0]])
    {
        let HolePlacement::Directed {
            position,
            direction,
        } = placement
        else {
            panic!("directed");
        };
        let p = position.get();
        for (actual, expected) in [p.x, p.y, p.z].into_iter().zip(expected) {
            assert!((actual - expected).abs() < EPS_COORDINATE);
        }
        assert_eq!(direction.get(), Vector3::new(0.0, 0.0, 1.0));
    }
}

#[test]
fn spotface_uses_a_flat_recess_and_retains_its_form() {
    let projected = HoleFixture::new(3, 12, 5)
        .project(&[])
        .expect("spotface projection");
    let FeatureDefinition::Operation(FeatureOperation::Hole { shape, .. }) =
        projected.evaluation.definition()
    else {
        panic!("hole");
    };
    assert!(matches!(
        shape.construction(),
        cadmpeg_ir::features::holes::HoleConstruction::Form {
            kind: HoleKind::Counterbore { diameter, depth },
            ..
        } if diameter.get() == 6.0 && depth.get() == 1.0
    ));
    assert_eq!(
        projected
            .source_properties
            .get("inventor.hole_form")
            .map(String::as_str),
        Some("spotface")
    );
}

#[test]
fn spotface_drill_point_angles_require_an_interior_angle() {
    let mut fixture = HoleFixture::new(3, 11, 1);
    fixture.values[5].value = Some(ParameterValue::Angle(
        Angle::new(1.0).expect("finite test angle"),
    ));
    let projected = fixture.project(&[]).expect("drilled spotface projection");
    let FeatureDefinition::Operation(FeatureOperation::Hole { shape, .. }) =
        projected.evaluation.definition()
    else {
        panic!("hole");
    };
    assert!(matches!(
        shape.construction(),
        cadmpeg_ir::features::holes::HoleConstruction::Form {
            kind: HoleKind::CounterboreDrilled { drill_point_angle, .. },
            ..
        } if drill_point_angle.get() == 1.0
    ));
    for angle in [-1.0, std::f64::consts::PI] {
        fixture.values[5].value = Some(ParameterValue::Angle(
            Angle::new(angle).expect("finite test angle"),
        ));
        assert!(fixture.project(&[]).is_none());
    }
}

#[test]
fn hole_extents_keep_their_termination_kind() {
    for t in [11, 12] {
        for (value, expected) in [(1, "blind"), (4, "through_next"), (5, "through_all")] {
            let projected = HoleFixture::new(0, t, value)
                .project(&[])
                .expect("hole extent projection");
            let FeatureDefinition::Operation(FeatureOperation::Hole {
                extent: Some(extent),
                ..
            }) = projected.evaluation.definition()
            else {
                panic!("hole extent");
            };
            assert_eq!(
                serde_json::to_value(extent).expect("termination JSON")["kind"],
                expected
            );
            if let LinearTermination::Blind { length } = extent {
                assert_eq!(length.get(), 15.0);
            }
        }
    }
    assert!(HoleFixture::new(0, 13, 5).project(&[]).is_none());
    assert!(HoleFixture::new(0, 11, 6).project(&[]).is_none());
}

#[test]
fn hole_targets_use_plane_frames_or_native_face_selections() {
    let mut fixture = HoleFixture::new(0, 11, 7);
    let projected = fixture.project(&[(13, 43)]).expect("work-plane target");
    let FeatureDefinition::Operation(FeatureOperation::Hole {
        extent: Some(LinearTermination::ToPlane { plane, native }),
        ..
    }) = projected.evaluation.definition()
    else {
        panic!("plane termination");
    };
    assert_eq!(plane.origin().get(), Point3::new(10.0, 20.0, 0.0));
    assert_eq!(plane.normal().get(), Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(native.as_str(), fixture.plane.id());
    assert!(fixture.project(&[(14, 42)]).is_none());
    fixture.properties[5] = test_property(6, PmDcFeaturePropertyKind::TerminationFlag { value: 0 });
    assert!(fixture.project(&[(13, 43)]).is_none());
    let projected = fixture.project(&[(14, 42)]).expect("native face target");
    let FeatureDefinition::Operation(FeatureOperation::Hole {
        extent:
            Some(LinearTermination::ToFace {
                face: cadmpeg_ir::features::FaceSelection::Native(face),
                offset: None,
            }),
        ..
    }) = projected.evaluation.definition()
    else {
        panic!("face termination");
    };
    assert_eq!(face, &fixture.faces.id());
    assert!(fixture.project(&[(13, 43), (14, 42)]).is_none());
    assert!(fixture.project(&[]).is_none());
    assert!(fixture.project(&[(13, 43), (15, 0)]).is_none());
}

#[test]
fn invalid_center_graphs_stay_native() {
    for centers in [&[][..], &[81, 81], &[0], &[83]] {
        let mut fixture = HoleFixture::new(0, 11, 5);
        fixture.centers = center_property(reference_list(centers));
        assert!(fixture.project(&[]).is_none());
    }
    let fixture = HoleFixture::new(0, 11, 5);
    assert!(fixture.project(&[(7, 0)]).is_none());
    assert!(fixture.project(&[(8, 62)]).is_none());
    let mut fixture = HoleFixture::new(0, 11, 5);
    let mut point = (*fixture.points[0]).clone();
    point.sketch = reference(52);
    fixture.points[0] = Located::new(
        point,
        fixture.points[0].identity.type_id.clone(),
        segment(),
        80,
    );
    assert!(fixture.project(&[]).is_none());
}

#[test]
fn invalid_recess_dimensions_and_unresolved_boolean_stay_native() {
    for (parameter, value) in [(2, 2.0), (3, 0.0), (3, -1.0)] {
        let mut fixture = HoleFixture::new(3, 11, 5);
        fixture.values[parameter].value = Some(ParameterValue::Length(
            Length::new(value).expect("finite test length"),
        ));
        assert!(fixture.project(&[]).is_none());
    }
    let mut fixture = HoleFixture::new(0, 11, 5);
    fixture.properties[2] = test_property(
        3,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value: true,
        },
    );
    assert!(fixture.project(&[]).is_none());
}

#[test]
fn incomplete_or_ambiguous_face_targets_stay_native() {
    for values in [vec![], vec![0], vec![7, 8]] {
        let mut fixture = HoleFixture::new(0, 11, 7);
        fixture.properties[5] =
            test_property(6, PmDcFeaturePropertyKind::TerminationFlag { value: 0 });
        let metadata = (!values.is_empty()).then_some(crate::pmdc::PmDcListMetadata::U32([1, 0]));
        fixture.faces = test_property(
            41,
            PmDcFeaturePropertyKind::FaceItem {
                index_references: PmDcU32List::new(2, metadata, values)
                    .expect("face-list metadata"),
            },
        );
        assert!(fixture.project(&[(14, 42)]).is_none());
    }
}

#[test]
fn center_membership_uses_the_record_index() {
    let mut fixture = HoleFixture::new(0, 11, 5);
    fixture.centers = center_property(
        PmDcReferenceList::new(
            2,
            Some(crate::pmdc::PmDcListMetadata::U32([2, 0])),
            vec![
                crate::pmdc::PmDcReference::from_packed(81),
                crate::pmdc::PmDcReference::from_packed(82),
            ],
        )
        .expect("center-list metadata"),
    );
    assert!(fixture.project(&[]).is_some());
}

#[test]
fn unsupported_hole_boolean_options_stay_native() {
    let mut fixture = HoleFixture::new(0, 11, 5);
    fixture.properties.push(test_property(
        7,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value: true,
        },
    ));
    fixture.properties.push(test_property(
        8,
        PmDcFeaturePropertyKind::Placement {
            transform: reference(61),
            point: reference(81),
            value: reference(82),
        },
    ));
    for placement in [0, 9] {
        assert!(fixture.project(&[(21, placement)]).is_some());
        for slot in [26, 27] {
            for option in [8, 3, 999] {
                assert!(fixture
                    .project(&[(21, placement), (slot, option)])
                    .is_none());
            }
            assert!(fixture.project(&[(21, placement), (slot, 4)]).is_some());
        }
    }
}

#[test]
fn sketch_center_placement_requires_a_valid_sketch_frame() {
    let fixture = HoleFixture::new(0, 11, 5);
    assert!(fixture.project(&[]).is_some());
    for column in [[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0]] {
        let mut fixture = HoleFixture::new(0, 11, 5);
        let mut transform = (*fixture.transform).clone();
        let mut rows = transform.matrix.rows();
        for (row, value) in rows.iter_mut().zip(column) {
            row[1] = value;
        }
        transform.matrix = crate::compact_matrix::CompactMatrix::try_from_rows(0, 0, rows)
            .expect("finite test matrix");
        fixture.transform = Located::new(
            transform,
            fixture.transform.identity.type_id.clone(),
            segment(),
            60,
        );
        assert!(fixture.project(&[]).is_none());
    }
    let mut fixture = HoleFixture::new(0, 11, 5);
    let mut direction = (*fixture.direction).clone();
    direction.direction[2] = cadmpeg_ir::scalar::FiniteReal::ONE.negated();
    fixture.direction = Located::new(
        direction,
        fixture.direction.identity.type_id.clone(),
        segment(),
        61,
    );
    assert!(fixture.project(&[]).is_none());
}

#[test]
fn fixed_depth_holes_require_positive_depth() {
    for extent_type in [11, 12] {
        let mut fixture = HoleFixture::new(0, extent_type, 1);
        assert!(fixture.project(&[]).is_some());
        for depth in [0.0, -1.0] {
            fixture.values[1].value = Some(ParameterValue::Length(
                Length::new(depth).expect("finite test length"),
            ));
            assert!(fixture.project(&[]).is_none());
        }
    }
}

#[test]
fn hole_inventory_reads_only_referenced_selection_records() {
    let slot = 7;
    let type_id = hole::CENTERS_TYPE;
    let mut bytes = content(0);
    bytes.extend(references(&[81]));
    for selected in [false, true] {
        for malformed in [false, true] {
            let payload = if malformed {
                &bytes[..bytes.len() - 1]
            } else {
                &bytes
            };
            let parsed = with_record(
                type_id,
                payload,
                DecodePolicy::service(),
                |ctx, document| {
                    let slots = if selected {
                        vec![(slot, 0)]
                    } else {
                        Vec::new()
                    };
                    inventory_with_hole(ctx, document, &slots)
                },
            )
            .expect("selection inventory");
            let selected_ordinal = parsed.features[0].properties.references()[slot]
                .index()
                .checked_sub(1);
            assert_eq!(
                parsed
                    .properties
                    .iter()
                    .filter(|property| {
                        Some(property.identity.record_ordinal) == selected_ordinal
                    })
                    .count(),
                usize::from(selected && !malformed)
            );
            assert_eq!(
                parsed
                    .issues
                    .iter()
                    .filter(|issue| { Some(issue.record_ordinal) == selected_ordinal })
                    .count(),
                usize::from(selected && malformed)
            );
            assert_eq!(parsed.properties.len(), usize::from(!malformed));
            assert_eq!(parsed.issues.len(), usize::from(malformed));
            if let Some(selection) = parsed.properties.first() {
                assert_eq!(selection.identity.record_ordinal, 0);
                assert_eq!(
                    selection.identity.type_id.as_str(),
                    crate::pmdc::type_id_string(type_id)
                );
                let PmDcFeaturePropertyKind::References { family, items } = &selection.kind else {
                    panic!("center references");
                };
                assert_eq!(*family, PmDcFeatureReferenceFamily::HoleCenters);
                assert_eq!(items.references().len(), 1);
            }
        }
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let refused = with_record(type_id, &bytes, policy, |ctx, document| {
        inventory_with_hole(ctx, document, &[(slot, 0)])
    });
    assert!(matches!(refused, Err(CodecError::ResourceLimit(_))));
}

fn inventory_with_hole(
    ctx: &DecodeContext<'_>,
    document: &crate::rse::RseInventory<'_>,
    slots: &[(usize, u32)],
) -> Result<super::super::FeatureInventory, CodecError> {
    let mut parsed = inventory(ctx, document)?;
    let token = document.segments[0].pair.token.key();
    let mut feature = test_feature(100, 25, slots);
    feature.identity.segment_token = token.clone();
    let mut label = test_label(100, 9, HOLE_CLASS_ID, &[]);
    label.identity.segment_token = token.clone();
    parsed.features.push(feature);
    parsed.labels.push(label);
    Ok(parsed)
}

fn center_property(items: PmDcReferenceList) -> PmDcFeatureProperty {
    let mut property = test_property(
        40,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::HoleCenters,
            items,
        },
    );
    property.identity.type_id =
        crate::record_identity::RecordTypeId::from_bytes(hole::CENTERS_TYPE);
    property
}
