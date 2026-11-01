// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::feature::tests::{
    raw_parameter, reference, reference_list, test_feature, test_property,
};
use crate::record_identity::{Located, RecordTypeId};
use cadmpeg_ir::scalar::{Angle, FiniteReal};

struct Fixture {
    source: PmDcFeature,
    properties: Vec<PmDcFeatureProperty>,
    parameter: PmDcParameter,
    value: ParameterValue,
    dimension: crate::feature::revolve::Dimension,
}

fn edited<T: Clone>(record: &Located<T>, edit: impl FnOnce(&mut T)) -> Located<T> {
    let mut payload = (**record).clone();
    edit(&mut payload);
    Located::new(
        payload,
        record.identity.type_id.clone(),
        record.identity.segment_token.clone(),
        record.identity.record_ordinal,
    )
}

impl Fixture {
    fn new() -> Self {
        let refs = |family, items| PmDcFeaturePropertyKind::References {
            family,
            items: reference_list(items),
        };
        let enumeration = |family, type_value, value| PmDcFeaturePropertyKind::Enumeration {
            family,
            type_value,
            value,
        };
        let selection = |entity_link| PmDcFeaturePropertyKind::ProfileSelection {
            entity_link: reference(entity_link),
            value: 0,
        };
        let mut properties = vec![
            test_property(0, refs(PmDcFeatureReferenceFamily::BoundaryPatch, &[12])),
            test_property(1, selection(101)),
            test_property(2, enumeration(PmDcFeatureEnumFamily::PartOperation, 5, 1)),
            test_property(4, enumeration(PmDcFeatureEnumFamily::Extent, 11, 3)),
            test_property(
                7,
                refs(PmDcFeatureReferenceFamily::FeatureDimensions, &[21]),
            ),
            test_property(8, enumeration(PmDcFeatureEnumFamily::Auxiliary, 3, 0)),
            test_property(9, enumeration(PmDcFeatureEnumFamily::Auxiliary, 2, 0)),
            test_property(10, enumeration(PmDcFeatureEnumFamily::Auxiliary, 2, 0)),
            test_property(
                15,
                refs(PmDcFeatureReferenceFamily::ObjectCollection, &[100]),
            ),
            test_property(11, selection(102)),
        ];
        for (i, type_id) in [
            (0, crate::feature::BOUNDARY_PATCH_TYPE),
            (1, crate::feature::PROFILE_SELECTION_TYPE),
            (2, crate::feature::PART_OPERATION_TYPE),
            (3, crate::feature::EXTENT_TYPE),
            (4, crate::feature::FEATURE_DIMENSIONS_TYPE),
            (5, SWEEP_MODE_TYPE),
            (6, SWEEP_ORIENTATION_TYPE),
            (7, SWEEP_SCALING_TYPE),
            (8, crate::feature::OBJECT_COLLECTION_TYPE),
            (9, crate::feature::PROFILE_SELECTION_TYPE),
        ] {
            properties[i].identity.type_id = RecordTypeId::from_bytes(type_id);
        }
        let parameter = raw_parameter(3);
        let dimension = Located::new(
            crate::feature::revolve::DimensionPayload {
                parameter: reference(4),
                ..Default::default()
            },
            RecordTypeId::from_bytes(crate::feature::revolve::DIMENSION_TYPE),
            parameter.identity.segment_token.clone(),
            20,
        );
        Self {
            source: test_feature(
                30,
                16,
                &[
                    (0, 0),
                    (1, 1),
                    (2, 2),
                    (3, 3),
                    (4, 4),
                    (7, 7),
                    (8, 8),
                    (9, 9),
                    (10, 10),
                    (15, 15),
                ],
            ),
            properties,
            parameter,
            dimension,
            value: ParameterValue::Angle(Angle::from_assigned_real(FiniteReal::ZERO)),
        }
    }

    fn admitted(&self) -> Result<Operands, NativeReason> {
        operands(
            &self.source,
            |r| {
                self.properties
                    .iter()
                    .find(|p| Some(p.identity.record_ordinal) == r.record_ordinal())
            },
            |r| (r.record_ordinal() == Some(3)).then_some((&self.parameter, &self.value)),
            |r| (r.record_ordinal() == Some(20)).then_some(&self.dimension),
        )
    }

    fn validates(&self, definition: &FeatureDefinition) -> bool {
        validates(
            &self.source,
            definition,
            |r| {
                self.properties
                    .iter()
                    .find(|p| Some(p.identity.record_ordinal) == r.record_ordinal())
            },
            |r| (r.record_ordinal() == Some(3)).then_some((&self.parameter, &self.value)),
            |r| (r.record_ordinal() == Some(20)).then_some(&self.dimension),
        )
    }

    fn change_kind(&mut self, i: usize, edit: impl FnOnce(&mut PmDcFeaturePropertyKind)) {
        self.properties[i] = edited(&self.properties[i], |p| edit(&mut p.kind));
    }
}

fn definition() -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::Sweep {
        shape: SweepShape::Solid {
            op: SolidSweepOperation::NewBody,
            section: SweepSection::Profile(cadmpeg_ir::features::PlanarProfileRef::native(
                "synthetic:profile#p".into(),
            )),
            sections: Vec::new(),
        },
        path: Some(cadmpeg_ir::features::PathRef::Native(
            "synthetic:path#p".into(),
        )),
        orientation: None,
        transition: None,
        transformation: None,
        path_tangent: false,
        linearize: false,
        twist: None,
        path_extent: None,
        guide_rail: None,
        taper: None,
        scale: None,
        allow_multi_profile_faces: None,
    })
}

#[test]
fn circular_operands_and_validator_share_every_control_gate() {
    let f = Fixture::new();
    assert_eq!(
        f.admitted(),
        Ok(Operands {
            section: reference(102),
            path: reference(101)
        })
    );
    assert!(f.validates(&definition()));
    for property in [1, 9] {
        let mut f = Fixture::new();
        f.properties[property].identity.type_id =
            RecordTypeId::from_bytes(crate::feature::PLATE_PROFILE_SELECTION_TYPE);
        assert!(f.admitted().is_ok());
        assert!(f.validates(&definition()));
    }
    for (property, slot) in [(2, 2), (3, 4), (5, 8), (6, 9), (7, 10)] {
        for discriminator in [false, true] {
            let mut f = Fixture::new();
            f.change_kind(property, |kind| {
                let PmDcFeaturePropertyKind::Enumeration {
                    type_value, value, ..
                } = kind
                else {
                    panic!("enumeration")
                };
                if discriminator {
                    *type_value += 1;
                } else {
                    *value += 1;
                }
            });
            assert_eq!(f.admitted(), Err(NativeReason::Slot(slot)));
            assert!(!f.validates(&definition()));
        }
        let mut f = Fixture::new();
        f.properties[property].identity.type_id = RecordTypeId::from_bytes(SWEEP_SCALING_TYPE);
        if slot == 10 {
            f.properties[property].identity.type_id = RecordTypeId::from_bytes(SWEEP_MODE_TYPE);
        }
        assert_eq!(f.admitted(), Err(NativeReason::Slot(slot)));
        assert!(!f.validates(&definition()));
    }
}

#[test]
fn angle_and_dimension_refusals_check_nominal_model_units_and_reference() {
    for control in 0..5 {
        let mut f = Fixture::new();
        let one = FiniteReal::new(1.0).expect("one");
        match control {
            0 => f.parameter = edited(&f.parameter, |p| p.nominal_value = one),
            1 => f.parameter = edited(&f.parameter, |p| p.model_value = one),
            2 => f.value = ParameterValue::Real(FiniteReal::ZERO),
            3 => f.value = ParameterValue::Angle(Angle::from_assigned_real(one)),
            _ => f.parameter.identity.type_id = RecordTypeId::from_bytes(SWEEP_MODE_TYPE),
        }
        assert_eq!(f.admitted(), Err(NativeReason::Angle));
        assert!(!f.validates(&definition()));
    }
    let mut f = Fixture::new();
    f.dimension = edited(&f.dimension, |d| d.parameter = reference(5));
    assert_eq!(f.admitted(), Err(NativeReason::Dimension));
    assert!(!f.validates(&definition()));
    let mut f = Fixture::new();
    f.change_kind(4, |kind| {
        *kind = PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::FeatureDimensions,
            items: reference_list(&[21, 22]),
        }
    });
    assert_eq!(f.admitted(), Err(NativeReason::Dimension));
    assert!(!f.validates(&definition()));
}

#[test]
fn selection_result_and_extra_slot_refusals_are_ordered() {
    for (i, reason) in [
        (0, NativeReason::SectionSelection),
        (1, NativeReason::PathSelection),
        (8, NativeReason::ResultCollection),
        (9, NativeReason::SectionSelection),
    ] {
        let mut f = Fixture::new();
        f.change_kind(i, |kind| match kind {
            PmDcFeaturePropertyKind::References { items, .. } => *items = reference_list(&[]),
            PmDcFeaturePropertyKind::ProfileSelection { value, .. } => *value = 1,
            _ => panic!("selection or result"),
        });
        assert_eq!(f.admitted(), Err(reason));
        assert!(!f.validates(&definition()));
    }
    let mut f = Fixture::new();
    f.source = test_feature(30, 16, &[(5, 11), (15, 15)]);
    assert_eq!(f.admitted(), Err(NativeReason::Slot(5)));
    f.source = test_feature(30, 15, &[]);
    assert_eq!(f.admitted(), Err(NativeReason::Slots));
    assert!(!f.validates(&definition()));
}

#[test]
fn native_sweep_refusal_notes_pin_each_reason_and_the_existing_loss_code() {
    for (reason, text) in [
        (NativeReason::Slots, "property vector"),
        (NativeReason::Slot(9), "slot 9"),
        (NativeReason::ResultCollection, "slot 15"),
        (NativeReason::Angle, "slot 3"),
        (NativeReason::Dimension, "slot 7"),
        (NativeReason::SectionSelection, "slot 0"),
        (NativeReason::CircularSection, "circular profile"),
        (NativeReason::PathSelection, "slot 1"),
        (NativeReason::Path, "ordered open wire"),
        (NativeReason::ResultTopology, "generator-owned result"),
        (NativeReason::ResultIdentity, "result body identity"),
    ] {
        let note = reason.note();
        assert_eq!(note.code.local_code(), "feature.sweep-controls-unresolved");
        assert!(note.message.contains(text), "{}", note.message);
    }
}

#[test]
fn projection_emits_the_first_refused_sweep_check() {
    use crate::feature::tests::{test_label, test_projection_index};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let source = test_feature(30, 16, &[]);
    let label = test_label(30, 0, super::super::slots::SWEEP_CLASS, &[]);
    let index = test_projection_index(&[], &[], &[], &[], &[], &[], &[], &[]);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let mut losses = Vec::new();
    assert!(projection::project(
        &ctx,
        &source,
        &label,
        &index,
        crate::feature::FeatureFamily::Sweep,
        &mut losses
    )
    .is_none());
    assert_eq!(losses, vec![NativeReason::ResultCollection.note()]);
}

#[test]
fn validator_refuses_unrepresented_sweep_controls() {
    for field in ["path_tangent", "linearize"] {
        let mut value = serde_json::to_value(definition()).expect("definition");
        value[field] = serde_json::json!(true);
        let value = serde_json::from_value(value).expect("changed definition");
        assert!(!Fixture::new().validates(&value));
    }
}

#[test]
fn versioned_extent_and_absent_dimension_keep_the_same_predicate() {
    let mut f = Fixture::new();
    f.source = edited(&f.source, |s| {
        s.save_version_major = crate::feature::EXTENT_TYPE_12_FIRST_MAJOR;
    });
    assert_eq!(f.admitted(), Err(NativeReason::Slot(4)));
    assert!(!f.validates(&definition()));
    f.change_kind(3, |kind| {
        let PmDcFeaturePropertyKind::Enumeration { type_value, .. } = kind else {
            panic!("extent")
        };
        *type_value = 12;
    });
    assert!(f.admitted().is_ok());
    assert!(f.validates(&definition()));
    f.source = edited(&f.source, |s| {
        let mut items = s.properties.references().to_vec();
        items[7] = reference(0);
        items.extend([reference(0), reference(0)]);
        s.properties = crate::pmdc::PmDcReferenceList::new(
            2,
            Some(crate::pmdc::PmDcListMetadata::U32([18, 0])),
            items,
        )
        .expect("property vector");
    });
    assert!(f.admitted().is_ok());
    assert!(f.validates(&definition()));
}

#[test]
fn selection_dimension_and_result_types_have_adjacent_refusal_tests() {
    for (property, reason) in [
        (0, NativeReason::SectionSelection),
        (1, NativeReason::PathSelection),
        (4, NativeReason::Dimension),
        (8, NativeReason::ResultCollection),
        (9, NativeReason::SectionSelection),
    ] {
        let mut f = Fixture::new();
        f.properties[property].identity.type_id = RecordTypeId::from_bytes(SWEEP_MODE_TYPE);
        assert_eq!(f.admitted(), Err(reason));
        assert!(!f.validates(&definition()));
    }
    let mut f = Fixture::new();
    f.change_kind(8, |kind| {
        let PmDcFeaturePropertyKind::References { items, .. } = kind else {
            panic!("result")
        };
        *items = reference_list(&[0]);
    });
    assert_eq!(f.admitted(), Err(NativeReason::ResultCollection));
    assert!(!f.validates(&definition()));
}
