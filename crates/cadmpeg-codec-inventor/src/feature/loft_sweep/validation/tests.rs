// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::feature::loft_sweep::slots::SURFACE_LOFT_CLASS;
use crate::feature::loft_sweep::{
    FACE_LOOP_PARTICIPANT_TYPE, LOFT_MODE_TYPE, SECTION_CONDITION_TYPE, SECTION_LIST_TYPE,
    SECTION_TYPE,
};
use crate::feature::tests::{
    raw_parameter, reference, reference_list, segment, test_feature, test_header, test_label,
    test_property,
};
use crate::feature::{PmDcFeatureEnumFamily, PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily};
use crate::pmdc::type_id_string;
use crate::record_identity::{Located, RecordTypeId};
use cadmpeg_ir::features::operation_settings::BooleanOp;
use cadmpeg_ir::features::profiles::SketchProfileRegion;
use cadmpeg_ir::features::LoftGuidance;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, FiniteReal};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement, SketchProfiles,
};

/// The record ordinal of the consumed sketch and of the face-loop participant.
const SKETCH_ORDINAL: u32 = 50;
const PARTICIPANT_ORDINAL: u32 = 51;

struct Surface {
    ir: CadIr,
    raw_sketches: Vec<PmDcSketch>,
    source: PmDcFeature,
    label: PmDcFeatureLabel,
    properties: Vec<PmDcFeatureProperty>,
    parameters: [PmDcParameter; 2],
    values: [ParameterValue; 2],
}

fn edited(
    record: &PmDcFeatureProperty,
    edit: impl FnOnce(&mut PmDcFeaturePropertyKind),
) -> PmDcFeatureProperty {
    let mut payload = (**record).clone();
    edit(&mut payload.kind);
    Located::new(
        payload,
        record.identity.type_id.clone(),
        record.identity.segment_token.clone(),
        record.identity.record_ordinal,
    )
}

fn with_source_index(record: &PmDcFeatureProperty, source_index: u32) -> PmDcFeatureProperty {
    let mut payload = (**record).clone();
    payload.header.source_index = source_index;
    Located::new(
        payload,
        record.identity.type_id.clone(),
        record.identity.segment_token.clone(),
        record.identity.record_ordinal,
    )
}

impl Surface {
    fn new() -> Self {
        let enumeration = |family, type_value, value| PmDcFeaturePropertyKind::Enumeration {
            family,
            type_value,
            value,
        };
        let boolean = PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value: false,
        };
        let section = |entity, source_value| PmDcFeaturePropertyKind::LoftSection {
            entity_link: reference(entity),
            value: 0,
            condition: reference(9),
            parameters: [reference(10), reference(11)],
            map_value: 0,
            source_value,
            option: reference(0),
        };
        let mut properties = vec![
            test_property(
                0,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::LoftSections,
                    items: reference_list(&[7, 8]),
                },
            ),
            test_property(1, enumeration(PmDcFeatureEnumFamily::PartOperation, 5, 5)),
            test_property(2, boolean.clone()),
            test_property(3, boolean),
            test_property(4, enumeration(PmDcFeatureEnumFamily::Auxiliary, 2, 0)),
            test_property(
                5,
                PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) },
            ),
            // The face-loop section: `source_value` equals its source index.
            test_property(6, section(100, 7)),
            test_property(7, section(101, 1)),
            test_property(8, enumeration(PmDcFeatureEnumFamily::Auxiliary, 2, 0)),
        ];
        properties[6] = with_source_index(&properties[6], 7);
        for (ordinal, type_id) in [
            (0, SECTION_LIST_TYPE),
            (4, LOFT_MODE_TYPE),
            (6, SECTION_TYPE),
            (7, SECTION_TYPE),
            (8, SECTION_CONDITION_TYPE),
        ] {
            properties[ordinal].identity.type_id = RecordTypeId::from_bytes(type_id);
        }
        let right_angle = FiniteReal::new(std::f64::consts::FRAC_PI_2).expect("right angle");
        let mut second = (*raw_parameter(10)).clone();
        second.nominal_value = right_angle;
        second.model_value = right_angle;
        let second = Located::new(
            second,
            raw_parameter(10).identity.type_id.clone(),
            segment(),
            10,
        );
        let raw_sketch = Located::new(
            crate::sketch::PmDcSketchPayload {
                save_version_major: 19,
                header: test_header(),
                state: 0,
                count_value: 0,
                entities: crate::pmdc::PmDcReferenceList::new(8, None, Vec::new())
                    .expect("empty entity list"),
                transform: reference(91),
                direction: reference(61),
                values: [0; 2],
                auxiliary: None,
            },
            RecordTypeId::try_from("114d8790d011f8d10008cabc0663dc09".to_owned())
                .expect("test GUID"),
            segment(),
            SKETCH_ORDINAL,
        );
        let sketch_id = SketchId::mint("test:design:sketch#wall").expect("id");
        let corners = [
            Point2::new(0.0, 0.0),
            Point2::new(6.0, 0.0),
            Point2::new(6.0, 4.0),
            Point2::new(0.0, 4.0),
        ];
        let entities = (0..4)
            .map(|side| {
                SketchEntity::new(
                    SketchEntityId::mint(format!("test:design:sketch-entity#{side}")).expect("id"),
                    sketch_id.clone(),
                    SketchGeometry::try_from(SketchGeometryDefinition::Line {
                        start: corners[side],
                        end: corners[(side + 1) % 4],
                    })
                    .expect("line"),
                )
            })
            .collect::<Vec<_>>();
        let sketch = Sketch {
            id: sketch_id,
            name: None,
            configuration: None,
            visible: None,
            placement: SketchPlacement::try_resolved(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("plane"),
            profiles: SketchProfiles::try_from(vec![entities
                .iter()
                .map(|entity| SketchEntityUse {
                    entity: entity.id().clone(),
                    reversed: false,
                })
                .collect()])
            .expect("profile"),
            native_ref: Some(raw_sketch.id()),
        };
        let mut ir = CadIr::empty();
        ir.model.sketches = vec![sketch];
        ir.model.sketch_entities = entities;
        Self {
            ir,
            raw_sketches: vec![raw_sketch],
            source: test_feature(20, 14, &[(0, 0), (1, 1), (2, 2), (9, 3), (10, 4), (6, 5)]),
            label: test_label(
                20,
                1,
                SURFACE_LOFT_CLASS,
                &[SKETCH_ORDINAL + 1, PARTICIPANT_ORDINAL + 1],
            ),
            properties,
            parameters: [raw_parameter(9), second],
            values: [
                ParameterValue::Real(FiniteReal::ZERO),
                ParameterValue::Angle(Angle::from_assigned_real(right_angle)),
            ],
        }
    }

    /// The loft with the sketch section `region` of the sketch and the
    /// face-loop section in the stored order (face loop first).
    fn definition(&self, region: u32) -> FeatureDefinition {
        let sketch = self.ir.model.sketches[0].id.clone();
        let regions = vec![SketchProfileRegion::loops(region, Vec::new()).expect("region")];
        FeatureDefinition::Operation(FeatureOperation::Loft {
            sections: vec![
                LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::native(
                    self.properties[6].id(),
                ))),
                LoftSection::Profile(ProfileRef::Planar(
                    PlanarProfileRef::sketch_regions(sketch, regions).expect("profile"),
                )),
            ],
            guidance: LoftGuidance::default(),
            op: BooleanOp::NewBody,
            closed: false,
            solid: false,
            ruled: false,
            linearize: false,
            max_degree: None,
            allow_multi_profile_faces: None,
        })
    }

    fn matches(&self, definition: &FeatureDefinition) -> bool {
        self.matches_typed(definition, &self.label, PARTICIPANT_ORDINAL)
    }

    fn matches_typed(
        &self,
        definition: &FeatureDefinition,
        label: &PmDcFeatureLabel,
        typed: u32,
    ) -> bool {
        let index = ValidationIndex::new(&self.ir, &self.raw_sketches, &[], &[], &[]);
        let participant = type_id_string(FACE_LOOP_PARTICIPANT_TYPE);
        index.surface_matches(
            &self.source,
            label,
            definition,
            |r| {
                self.properties
                    .iter()
                    .find(|p| Some(p.identity.record_ordinal) == r.record_ordinal())
            },
            |r| {
                let at = usize::try_from(r.record_ordinal()?).ok()?.checked_sub(9)?;
                Some((self.parameters.get(at)?, self.values.get(at)?))
            },
            controls::SurfaceContext {
                type_at: |ordinal| (ordinal == typed).then_some(participant.as_str()),
                wrapper_used_earlier: false,
            },
        )
    }
}

#[test]
fn surface_match_accepts_the_recomputed_sketch_and_face_loop_sections() {
    let f = Surface::new();
    assert!(f.matches(&f.definition(0)));
}

#[test]
fn surface_match_rejects_a_different_loop_of_the_same_sketch() {
    let f = Surface::new();
    assert!(!f.matches(&f.definition(1)));
    // A sketch with two loops is not the whole-sketch form either.
    let mut two = Surface::new();
    let first = two.ir.model.sketches[0]
        .profiles
        .iter()
        .next()
        .cloned()
        .expect("profile");
    two.ir.model.sketches[0]
        .profiles
        .edit(|profiles| profiles.push(first))
        .expect("profiles");
    assert!(!two.matches(&two.definition(0)));
    assert!(!two.matches(&two.definition(1)));
}

#[test]
fn surface_match_rejects_the_wrong_participant_type_and_a_shared_wire() {
    let f = Surface::new();
    let definition = f.definition(0);
    // No participant record has the face-loop participant type.
    assert!(!f.matches_typed(&definition, &f.label, PARTICIPANT_ORDINAL + 1));
    // A label without a second participant has no face-loop participant.
    let alone = test_label(20, 1, SURFACE_LOFT_CLASS, &[SKETCH_ORDINAL + 1]);
    assert!(!f.matches_typed(&definition, &alone, PARTICIPANT_ORDINAL));
    // Both sections cannot select the same wire.
    let mut shared = Surface::new();
    shared.properties[7] = edited(&shared.properties[7], |kind| {
        let PmDcFeaturePropertyKind::LoftSection { entity_link, .. } = kind else {
            panic!("section");
        };
        *entity_link = reference(100);
    });
    assert!(!shared.matches(&shared.definition(0)));
}

#[test]
fn surface_match_rejects_a_face_loop_section_of_another_record() {
    let f = Surface::new();
    let FeatureDefinition::Operation(FeatureOperation::Loft { mut sections, .. }) = f.definition(0)
    else {
        panic!("loft");
    };
    sections[0] = LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::native(
        f.properties[7].id(),
    )));
    let swapped = FeatureDefinition::Operation(FeatureOperation::Loft {
        sections,
        guidance: LoftGuidance::default(),
        op: BooleanOp::NewBody,
        closed: false,
        solid: false,
        ruled: false,
        linearize: false,
        max_degree: None,
        allow_multi_profile_faces: None,
    });
    assert!(!f.matches(&swapped));
}

#[test]
fn surface_match_rejects_a_loft_whose_wrapper_an_earlier_feature_names() {
    let f = Surface::new();
    let index = ValidationIndex::new(&f.ir, &f.raw_sketches, &[], &[], &[]);
    let participant = type_id_string(FACE_LOOP_PARTICIPANT_TYPE);
    let definition = f.definition(0);
    let run = |used| {
        index.surface_matches(
            &f.source,
            &f.label,
            &definition,
            |r| {
                f.properties
                    .iter()
                    .find(|p| Some(p.identity.record_ordinal) == r.record_ordinal())
            },
            |r| {
                let at = usize::try_from(r.record_ordinal()?).ok()?.checked_sub(9)?;
                Some((f.parameters.get(at)?, f.values.get(at)?))
            },
            controls::SurfaceContext {
                type_at: |ordinal| (ordinal == PARTICIPANT_ORDINAL).then_some(participant.as_str()),
                wrapper_used_earlier: used,
            },
        )
    };
    assert!(run(false));
    assert!(!run(true));
}
