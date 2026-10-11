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

fn transfer<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    saved: &FeatureSavedEntity,
    count: usize,
) -> Result<(Vec<SketchEntity>, cadmpeg_core::decode::ScopedReservation<'ctx>), CodecError> {
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
    let mut entity_slots = ctx.reserve_scoped(0, "test section entity staging")?;
    let (entities, profiles) = transfer_section_entities(ctx,
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
            entity_slots: &mut entity_slots,
            source_carriers: &mut carriers,
        },
 )?;
    assert!(profiles.is_empty());
    assert!(ir.model.curves.is_empty());
    assert!(losses.is_empty());
    Ok((entities, entity_slots))
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
        transfer(&ctx, saved, count).and_then(|(rows, slots)| slots.commit_value(rows))
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

#[test]
fn saved_entity_emission_refuses_before_staging_growth() {
    let error = crate::test_support::last_refusal_at(&[], ResourceDimension::CollectionItems, "creo section entities", |ctx| {
        transfer(ctx, &line(), 1).map(|_| ())
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems && resource.operation == "creo section entities"
        && resource.additional == 1));
    let entities = crate::decode::with_test_decode_ctx(|ctx| {
        let (rows, storage) = transfer(ctx, &line(), 1)?;
        storage.commit_value(rows)
    }).expect("service row");
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].id().as_str(), "creo:featdefs:sketch_entity#1:saved3");
}

#[test]
fn saved_entity_staging_backing_is_live_until_source_admission_finishes() {
    for admitted in [false, true] {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let (entities, slots) = transfer(&ctx, &line(), 1).expect("saved entity");
        let backing = u64::try_from(entities.capacity() * std::mem::size_of::<SketchEntity>()).expect("staging bytes");
        let (expected, _live_storage) = if admitted {
            let mut ir = CadIr::empty();
            crate::decode::source_carriers::SourceUnitCarriers::default().admit_sketch_entities(&ctx, &mut ir, entities).expect("source admission");
            assert_eq!(ir.model.sketch_entities.len(), 1);
            drop(slots);
            (0, None)
        } else { (backing, Some(slots)) };
        let error = ctx.reserve_scoped(u64::MAX, "test live entity backing").expect_err("live storage probe");
        assert!(matches!(error, CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes && resource.used == expected));
    }
}

#[test]
fn saved_lines_keep_only_one_live_base_reference() {
    let scan = crate::test_support::empty_container_scan();
    let definition = FeatureDefinition {
        identity: DefinitionIdentity::Parsed { schema_id: std::num::NonZeroU32::new(1), owner_feature_id: None },
        body: Vec::new(), parameter_frames: Vec::new(), outlines: Vec::new(), variables: None,
        segments: None, trim_entities: None, trim_vertices: None, order_table: None,
        section_3d: None, dimensions: None, relations: None,
        saved_section: Some(FeatureSavedSection {
            entities: (1..=64).map(|entity_id| FeatureSavedEntity::Line(FeatureSavedLine {
                entity_id, references: Vec::new(), attributes: Vec::new(),
                endpoints: [[Some(0.0), Some(0.0), None], [Some(1.0), Some(0.0), None]],
                body: Vec::new(), offset: 64,
            })).collect(), offset: 0,
        }), offset: 0,
    };
    let scope = "x".repeat(8192);
    let short_sketch = SketchId::mint("creo:model:sketch#1").expect("short sketch");
    let long_sketch = SketchId::mint(format!("creo:model:sketch#{scope}")).expect("long sketch");
    let run = |sketch: &SketchId, cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        // Provenance insertion compares long identities across the saved rows.
        // Keep that work limit above the materialized-byte boundary under test.
        policy.limits.max_work_units = 128 * 64 * 64 * u64::try_from(scope.len() + 128).expect("fixture work bound");
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut identity_storage = ctx.reserve_scoped(0, "test saved identity scratch")?;
        let unique_saved_ids = identity_storage.with_storage(|| super::super::super::identity::unique_saved_section_internal_ids(&ctx, &definition))?;
        let mut ir = CadIr::empty();
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        let mut losses = Vec::new();
        let mut carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
        let mut slots = ctx.reserve_scoped(0, "test saved row staging")?;
        let (entities, profiles) = transfer_section_entities(&ctx, SectionEntityTransfer {
            scan: &scan, ir: &mut ir, annotations: &mut annotations, definition: &definition,
            transform: None, sketch_id: sketch, segments: &[], unique_segment_ids: &BTreeSet::new(),
            unique_saved_ids: &unique_saved_ids, ambiguous_segment_ids: &BTreeSet::new(),
            complete_segment_table: false, solved: &BTreeSet::new(), segment_geometries: &BTreeMap::new(),
            resolved_segment_geometries: &BTreeMap::new(), circle_geometries: &BTreeMap::new(),
            point_geometries: &BTreeMap::new(), centered_line_geometries: &BTreeMap::new(), reference_line_geometries: &BTreeMap::new(),
            materialized_saved_section_external_ids: &BTreeSet::new(), profiles: Vec::new(), profile_entities: &BTreeSet::new(),
            losses: &mut losses, entity_slots: &mut slots, source_carriers: &mut carriers,
        })?;
        assert_eq!(entities.len(), 64);
        assert!(profiles.is_empty());
        for (index, entity) in entities.iter().enumerate() {
            assert_eq!(entity.id().as_str(), format!("creo:featdefs:sketch_entity#{}:saved{}", crate::decode::sketch_ids::sketch_identity_scope(sketch), index + 1));
        }
        slots.commit_value(entities)
    };
    let baseline = crate::test_support::allocation_limit_at(ResourceDimension::MaterializedBytes, None, |cap| run(&short_sketch, cap));
    // Each of the 64 scratch identity keys keeps its scope. Four more scope
    // lengths cover the current entity and its short-lived base reference.
    let cap = baseline + 68 * u64::try_from(scope.len()).expect("scope length");
    run(&long_sketch, cap).expect("saved rows must release each base reference before the next row");
}
