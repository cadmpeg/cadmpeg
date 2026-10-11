// SPDX-License-Identifier: Apache-2.0

use crate::feature::definitions::{
    FeatureCircleSegment, FeatureSegment, FeatureSegmentKind, FeatureSegmentTable, FeatureSkampItem,
};
use crate::feature::segment_rows::SegmentRow;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchEntityId, SketchGeometry, SketchId,
};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
enum Attempt {
    Center,
    Compatibility,
    Directed,
}

fn assert_native_attempt_storage(attempt: Attempt) {
    let compatibility_replacement = matches!(attempt, Attempt::Compatibility);
    let mut definition = super::make_definition(false);
    let segments: Vec<SegmentRow> = if matches!(attempt, Attempt::Directed) {
        [7, 8]
            .into_iter()
            .map(|external_id| {
                SegmentRow::Ordinary(FeatureSegment {
                    kind: FeatureSegmentKind::Line([10, 11]),
                    directions: [None; 3],
                    center_id: None,
                    arc_orientation: None,
                    vertical_horizontal: None,
                    radius_ref: None,
                    radius2_ref: None,
                    external_id,
                    body: Vec::new(),
                    offset: 0,
                })
            })
            .collect()
    } else {
        [7, 8]
            .into_iter()
            .map(|external_id| {
                SegmentRow::Circle(FeatureCircleSegment {
                    center_id: external_id,
                    radius_ref: 42,
                    external_id,
                    offset: 0,
                })
            })
            .collect()
    };
    definition.segments = Some(FeatureSegmentTable {
        declared_count: u32::try_from(segments.len()).expect("fixture count"),
        has_elided_prototype: false,
        entity_ref: None,
        rows: segments.into_iter().collect(),
        offset: 0,
    });
    let skamp = &mut definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("SKAMP table")
        .rows_mut()[0];
    skamp.kind = if matches!(attempt, Attempt::Directed) {
        3
    } else {
        0
    };
    skamp.items = vec![
        FeatureSkampItem {
            entity_id: 7,
            sense: 99,
        },
        FeatureSkampItem {
            entity_id: 8,
            sense: 99,
        },
    ];
    let geometry = BTreeMap::from([(
        SketchEntityId::mint("creo:featdefs:sketch_entity#1:7").expect("geometry ID"),
        SketchGeometry::native(
            cadmpeg_core::text::NonBlankString::try_from("circle").expect("native geometry"),
        ),
    )]);
    let sketch = SketchId::mint("creo:model:sketch#1").expect("sketch");
    let run = |definition: &crate::feature::definitions::FeatureDefinition, cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let rows = super::super::section_skamp_constraints_for_geometry(
            &ctx,
            definition,
            &sketch,
            compatibility_replacement.then_some(&geometry),
        )?;
        assert_eq!(rows.len(), 1);
        let SketchConstraintDefinitionInput::Native {
            entities, operands, ..
        } = rows[0].0.definition.kind()
        else {
            panic!("native fallback");
        };
        let items = &definition.relations.as_ref().expect("relations").skamps()[0].items;
        assert_eq!(
            entities.len(),
            if compatibility_replacement {
                1
            } else {
                items.len()
            }
        );
        assert_eq!(operands.len(), items.len());
        for (operand, item) in operands.iter().zip(items) {
            assert_eq!(operand.object_index, Some(item.entity_id));
            assert_eq!(
                operand.field.as_ref().expect("operand field").role,
                Some(item.sense)
            );
        }
        Ok::<_, cadmpeg_core::CodecError>(rows)
    };
    let cap =
        crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, None, |cap| {
            run(&definition, cap)
        });
    definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("SKAMP table")
        .rows_mut()[0]
        .items[0]
        .sense = if matches!(attempt, Attempt::Directed) {
        0
    } else {
        4
    };
    if compatibility_replacement || matches!(attempt, Attempt::Directed) {
        definition
            .relations
            .as_mut()
            .expect("relations")
            .skamps
            .as_mut()
            .expect("SKAMP table")
            .rows_mut()[0]
            .items[1]
            .sense = if compatibility_replacement { 4 } else { 0 };
    }
    run(&definition, cap)
        .expect("discarded typed storage must not increase the native payload charge");
}

#[test]
fn failed_skamp_center_pair_does_not_retain_partial_identities() {
    assert_native_attempt_storage(Attempt::Center);
}

#[test]
fn incompatible_skamp_definition_releases_its_typed_payload() {
    assert_native_attempt_storage(Attempt::Compatibility);
}

#[test]
fn skamp_point_on_curve_does_not_retain_failed_point_pair_storage() {
    use crate::feature::definitions::FeaturePointSegment;
    let mut definition = super::make_definition(false);
    definition.segments = Some(FeatureSegmentTable {
        declared_count: 2,
        has_elided_prototype: false,
        entity_ref: None,
        rows: [
            SegmentRow::Ordinary(FeatureSegment {
                kind: FeatureSegmentKind::Line([10, 11]),
                directions: [None; 3],
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 7,
                body: Vec::new(),
                offset: 0,
            }),
            SegmentRow::Point(FeaturePointSegment {
                point_id: 42,
                external_id: 8,
                offset: 0,
            }),
        ]
        .into_iter()
        .collect(),
        offset: 0,
    });
    let skamp = &mut definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("SKAMP table")
        .rows_mut()[0];
    skamp.kind = 9;
    skamp.items = vec![
        FeatureSkampItem {
            entity_id: 7,
            sense: 0,
        },
        FeatureSkampItem {
            entity_id: 8,
            sense: 0,
        },
    ];
    let sketch = SketchId::mint("creo:model:sketch#1").expect("sketch");
    let run = |definition: &crate::feature::definitions::FeatureDefinition, cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let rows =
            super::super::section_skamp_constraints_for_geometry(&ctx, definition, &sketch, None)?;
        assert_eq!(rows.len(), 1);
        assert!(
            matches!(rows[0].0.definition.kind(), SketchConstraintDefinitionInput::PointOnObject { point: cadmpeg_ir::sketches::SketchLocus::Entity(point), entity }
            if point.as_str() == "creo:featdefs:sketch_entity#1:8" && entity.as_str() == "creo:featdefs:sketch_entity#1:7")
        );
        Ok::<_, cadmpeg_core::CodecError>(rows)
    };
    let cap =
        crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, None, |cap| {
            run(&definition, cap)
        });
    let expected = run(&definition, cap).expect("direct typed route");
    definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("SKAMP table")
        .rows_mut()[0]
        .kind = 3;
    assert_eq!(
        run(&definition, cap).expect("failed earlier alternatives release their identities"),
        expected
    );
}

#[test]
fn failed_skamp_directed_pairs_release_their_dropped_payloads() {
    assert_native_attempt_storage(Attempt::Directed);
}
