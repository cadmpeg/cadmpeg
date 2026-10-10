// SPDX-License-Identifier: Apache-2.0

use super::super::{transfer_section_entities, SectionEntityTransfer};
use crate::feature::definitions::{
    DecodedField, DefinitionIdentity, FeatureDefinition, FeatureSavedEntity, FeatureSavedLine,
    FeatureSavedSection, FeatureSavedSpline,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::sketches::{SketchEntity, SketchId};
use std::collections::{BTreeMap, BTreeSet};

fn spline() -> FeatureSavedEntity {
    FeatureSavedEntity::Spline(FeatureSavedSpline {
        entity_id: Some(11),
        declared_point_count: Some(2),
        interpolation_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        interpolation_points_body: Vec::new(),
        endpoint_tangents: Some(DecodedField {
            value: [[1.0, 0.0, 0.0]; 2],
            body: Vec::new(),
        }),
        parameters: Some(DecodedField {
            value: vec![0.0, 1.0],
            body: Vec::new(),
        }),
        offset: 64,
    })
}

fn line() -> FeatureSavedEntity {
    FeatureSavedEntity::Line(FeatureSavedLine {
        entity_id: 3,
        references: Vec::new(),
        attributes: Vec::new(),
        endpoints: [[Some(0.0), Some(0.0), None], [Some(1.0), Some(0.0), None]],
        body: Vec::new(),
        offset: 64,
    })
}

fn transfer(
    ctx: &DecodeContext<'_>,
    saved: &FeatureSavedEntity,
    count: usize,
) -> Result<Vec<SketchEntity>, CodecError> {
    let scan = crate::test_support::empty_container_scan();
    let definition = FeatureDefinition {
        identity: DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(1),
            owner_feature_id: None,
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: Some(FeatureSavedSection {
            entities: vec![saved.clone(); count],
            offset: 0,
        }),
        offset: 0,
    };
    let sketch = SketchId::mint("creo:model:sketch#1").expect("fixture identity");
    let mut identity_storage = ctx.reserve_scoped(0, "test saved identity scratch")?;
    let unique_saved_ids = identity_storage.with_storage(|| {
        super::super::super::identity::unique_saved_section_internal_ids(ctx, &definition)
    })?;
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let mut losses = Vec::new();
    let mut carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    let (entities, profiles) = transfer_section_entities(
        ctx,
        SectionEntityTransfer {
            scan: &scan,
            ir: &mut ir,
            annotations: &mut annotations,
            definition: &definition,
            transform: None,
            sketch_id: &sketch,
            segments: &[],
            unique_segment_ids: &BTreeSet::new(),
            unique_saved_ids: &unique_saved_ids,
            ambiguous_segment_ids: &BTreeSet::new(),
            complete_segment_table: false,
            solved: &BTreeSet::new(),
            segment_geometries: &BTreeMap::new(),
            resolved_segment_geometries: &BTreeMap::new(),
            circle_geometries: &BTreeMap::new(),
            point_geometries: &BTreeMap::new(),
            centered_line_geometries: &BTreeMap::new(),
            reference_line_geometries: &BTreeMap::new(),
            materialized_saved_section_external_ids: &BTreeSet::new(),
            profiles: Vec::new(),
            profile_entities: &BTreeSet::new(),
            losses: &mut losses,
            source_carriers: &mut carriers,
        },
    )?;
    assert!(profiles.is_empty());
    assert!(ir.model.curves.is_empty());
    assert!(losses.is_empty());
    Ok(entities)
}

fn assert_duplicate_storage(saved: &FeatureSavedEntity, dimension: ResourceDimension) {
    let run = |count, cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
            _ => panic!("fixture storage dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        transfer(&ctx, saved, count)
    };
    let cap = crate::test_support::allocation_limit_at(dimension, None, |cap| run(2, cap));
    let expected = run(2, cap).expect("short fixture");
    assert_eq!(
        run(16, cap).expect("duplicates use no surviving extra storage"),
        expected
    );
}

#[test]
fn discarded_saved_splines_release_their_geometry() {
    assert_duplicate_storage(&spline(), ResourceDimension::MaterializedBytes);
}

#[test]
fn discarded_saved_spline_identities_are_not_retained() {
    assert_duplicate_storage(&spline(), ResourceDimension::RetainedBytes);
}

#[test]
fn resolved_saved_lines_do_not_retain_discarded_fallbacks_or_curve_ids() {
    assert_duplicate_storage(&line(), ResourceDimension::RetainedBytes);
}
