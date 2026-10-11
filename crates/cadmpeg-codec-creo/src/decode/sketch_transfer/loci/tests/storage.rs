// SPDX-License-Identifier: Apache-2.0

use crate::feature::definitions::{
    DefinitionIdentity, FeatureDefinition, FeaturePointSegment, FeatureSegment, FeatureSegmentKind,
    FeatureSegmentTable, FeatureSkampItem,
};
use crate::feature::segment_rows::SegmentRow;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::{SketchEntityId, SketchId, SketchLocus};
use std::cell::Cell;

fn definition(arc: bool) -> FeatureDefinition {
    let line = |external_id, point_ids| {
        SegmentRow::Ordinary(FeatureSegment {
            kind: if arc && external_id == 7 {
                FeatureSegmentKind::Arc(point_ids)
            } else {
                FeatureSegmentKind::Line(point_ids)
            },
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
    };
    FeatureDefinition {
        identity: DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(1),
            owner_feature_id: None,
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: Some(FeatureSegmentTable {
            declared_count: 3,
            has_elided_prototype: false,
            entity_ref: None,
            rows: [
                line(7, [1, 2]),
                line(8, [2, 3]),
                SegmentRow::Point(FeaturePointSegment {
                    external_id: 9,
                    point_id: 4,
                    offset: 0,
                }),
            ]
            .into_iter()
            .collect(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    }
}

#[test]
fn failed_locus_routes_release_their_discarded_identity() {
    let definition = definition(false);
    let sketch = SketchId::mint("creo:model:sketch#1").expect("sketch");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let refusal = Cell::new(None);
    for _ in 0..16 {
        let item = FeatureSkampItem {
            entity_id: 7,
            sense: 99,
        };
        assert!(
            super::super::section_skamp_locus(&ctx, &refusal, &definition, &sketch, &item)
                .expect("unsupported locus")
                .is_none()
        );
        let item = FeatureSkampItem {
            entity_id: 7,
            sense: 0,
        };
        assert!(super::super::section_skamp_incidence_locus(
            &ctx,
            &refusal,
            &definition,
            &sketch,
            &item,
            None
        )
        .expect("absent geometry")
        .is_none());
        let item = FeatureSkampItem {
            entity_id: 9,
            sense: 0,
        };
        assert!(super::super::section_skamp_oriented_line(
            &ctx,
            &refusal,
            &definition,
            &sketch,
            &item,
            None
        )
        .expect("point is no oriented line")
        .is_none());
        assert!(refusal.take().is_none());
    }
    assert_eq!(
        ctx.copy_retained_text("x", "test surviving locus output")
            .expect("failed loci retained no bytes"),
        "x"
    );
    ctx.finish_session().expect("active routes");
}

#[test]
fn shared_tangent_releases_the_failed_selected_pair() {
    let definition = definition(false);
    let sketch = SketchId::mint("creo:model:sketch#1").expect("sketch");
    let expected = [
        SketchLocus::End(SketchEntityId::mint("creo:featdefs:sketch_entity#1:7").expect("first")),
        SketchLocus::Start(
            SketchEntityId::mint("creo:featdefs:sketch_entity#1:8").expect("second"),
        ),
    ];
    let run = |first_sense, cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let refusal = Cell::new(None);
        let loci = super::super::section_skamp_tangent_loci(
            &ctx,
            &refusal,
            &definition,
            &sketch,
            (
                &FeatureSkampItem {
                    entity_id: 7,
                    sense: first_sense,
                },
                &FeatureSkampItem {
                    entity_id: 8,
                    sense: 2,
                },
            ),
            true,
            None,
        )?;
        if let Some(error) = refusal.take() {
            return Err(error);
        }
        assert_eq!(loci, Some(expected.clone()));
        Ok::<_, CodecError>(loci)
    };
    let cap =
        crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, None, |cap| {
            run(3, cap)
        });
    run(0, cap).expect("shared tangent retains only its two selected loci");
}

#[test]
fn midpoint_releases_the_unselected_direction() {
    let definition = definition(true);
    let sketch = SketchId::mint("creo:model:sketch#1").expect("sketch");
    let first = FeatureSkampItem {
        entity_id: 7,
        sense: 0,
    };
    let second = FeatureSkampItem {
        entity_id: 8,
        sense: 2,
    };
    let expected = (
        SketchLocus::Start(SketchEntityId::mint("creo:featdefs:sketch_entity#1:8").expect("point")),
        SketchEntityId::mint("creo:featdefs:sketch_entity#1:7").expect("target"),
    );
    let run = |midpoint, cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let refusal = Cell::new(None);
        let result = if midpoint {
            super::super::section_skamp_midpoint(
                &ctx,
                &refusal,
                &definition,
                &sketch,
                &first,
                &second,
                None,
            )?
        } else {
            let target = super::super::section_skamp_curve_entity(
                &ctx,
                &refusal,
                &definition,
                &sketch,
                &first,
            )?;
            let point = super::super::section_skamp_point_locus(
                &ctx,
                &refusal,
                &definition,
                &sketch,
                &second,
            )?;
            point.zip(target)
        };
        if let Some(error) = refusal.take() {
            return Err(error);
        }
        assert_eq!(result, Some(expected.clone()));
        Ok::<_, CodecError>(result)
    };
    let cap =
        crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, None, |cap| {
            run(false, cap)
        });
    run(true, cap).expect("midpoint retains only its selected locus and target");
}
