// SPDX-License-Identifier: Apache-2.0

use super::{copy_ring_coedges, sketch_profiles_cover_generated_extrusion_sides};
use crate::decode::tests::surface_row;
use cadmpeg_ir::sketches::{Sketch, SketchEntityId, SketchEntityUse, SketchId, SketchPlacement};

fn ring_copy_at_limits(
    collection_limit: u64,
    retained_limit: u64,
    collection_operation: &'static str,
    identity_operation: &'static str,
) -> Result<Vec<cadmpeg_ir::ids::CoedgeId>, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let ids = [cadmpeg_ir::ids::CoedgeId::mint("creo:brep:coedge#10:0")
        .expect("valid coedge identity")];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    copy_ring_coedges(&ctx, &ids, collection_operation, identity_operation)
}

#[test]
fn bottom_ring_copy_refuses_collection_limit() {
    assert!(matches!(ring_copy_at_limits(0, u64::MAX,
        "creo extrusion bottom ring coedge copies", "creo extrusion bottom ring coedge identities"),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion bottom ring coedge copies"));
}

#[test]
fn bottom_ring_copy_refuses_retained_limit() {
    assert!(matches!(ring_copy_at_limits(1, 0,
        "creo extrusion bottom ring coedge copies", "creo extrusion bottom ring coedge identities"),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion bottom ring coedge identities"));
}

#[test]
fn top_ring_copy_refuses_collection_limit() {
    assert!(matches!(ring_copy_at_limits(0, u64::MAX,
        "creo extrusion top ring coedge copies", "creo extrusion top ring coedge identities"),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion top ring coedge copies"));
}

#[test]
fn top_ring_copy_refuses_retained_limit() {
    assert!(matches!(ring_copy_at_limits(1, 0,
        "creo extrusion top ring coedge copies", "creo extrusion top ring coedge identities"),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion top ring coedge identities"));
}

#[test]
fn side_ring_copy_refuses_collection_limit() {
    assert!(matches!(ring_copy_at_limits(0, u64::MAX,
        "creo extrusion side ring coedge copies", "creo extrusion side ring coedge identities"),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion side ring coedge copies"));
}

#[test]
fn side_ring_copy_refuses_retained_limit() {
    assert!(matches!(ring_copy_at_limits(1, 0,
        "creo extrusion side ring coedge copies", "creo extrusion side ring coedge identities"),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion side ring coedge identities"));
}

#[test]
fn ring_copy_preserves_coedge_identity_order() {
    let ids = ring_copy_at_limits(1, u64::MAX,
        "creo extrusion side ring coedge copies", "creo extrusion side ring coedge identities")
        .expect("admitted ring copy");
    assert_eq!(ids[0].as_str(), "creo:brep:coedge#10:0");
}

fn definition() -> crate::feature::definitions::FeatureDefinition {
    crate::feature::definitions::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(7),
            owner_feature_id: Some(7),
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
        saved_section: None,
        offset: 0,
    }
}

fn generated_side_table() -> crate::feature::entity::FeatureEntityTable {
    crate::feature::entity::FeatureEntityTable::new(
        7,
        29,
        vec![crate::feature::entity::FeatureEntityTableEntry {
            entity_id: 31,
            payload: crate::feature::entity::entry_payload(200, Some(11), None, None),
            prefixed: false,
            offset: 0,
            end_offset: 0,
        }],
        &std::collections::BTreeSet::new(),
        0,
    )
    .with_surface_ids([31])
}

fn sketch() -> Sketch {
    let sketch_id = SketchId::mint("creo:model:sketch#7".to_string()).expect("valid test fixture");
    let entity = SketchEntityId::mint("creo:featdefs:sketch_entity#7:11".to_string())
        .expect("valid test fixture");
    Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![SketchEntityUse {
            entity,
            reversed: false,
        }]])
        .expect("valid test fixture"),
        native_ref: None,
    }
}

fn generated_side_coverage_at_limits(
    collection_limit: u64,
    materialized_limit: u64,
) -> Result<bool, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.features.entity_tables.push(generated_side_table());
    scan.surfaces
        .rows
        .push(surface_row(31, 7, crate::surface::SurfaceKind::Plane));
    let definition = definition();
    let sketch = sketch();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_materialized_bytes = materialized_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    sketch_profiles_cover_generated_extrusion_sides(&ctx, &scan, &definition, 7, &sketch)
}

#[test]
fn generated_side_profile_entity_nodes_refuse_limit() {
    assert!(matches!(generated_side_coverage_at_limits(0, u64::MAX),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion profile entity ID nodes"));
}

#[test]
fn generated_side_expected_entity_text_refuses_materialized_limit() {
    assert!(matches!(generated_side_coverage_at_limits(2, 0),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion expected sketch entity ID"));
}

#[test]
fn generated_side_expected_entity_nodes_refuse_limit() {
    assert!(matches!(generated_side_coverage_at_limits(1, u64::MAX),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion expected entity ID nodes"));
    assert!(generated_side_coverage_at_limits(2, u64::MAX)
        .expect("admitted coverage"));
}

#[test]
fn generated_side_coverage_rejects_duplicate_surface_rows() {
    crate::decode::with_test_decode_ctx(|ctx| {
    let definition = definition();
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.features.entity_tables.push(generated_side_table());
    scan.surfaces
        .rows
        .push(surface_row(31, 7, crate::surface::SurfaceKind::Plane));
    let sketch = sketch();

    assert!(sketch_profiles_cover_generated_extrusion_sides(ctx,
        &scan,
        &definition,
        7,
        &sketch,
    ).expect("admitted profile coverage"));

    let mut duplicate_profile = sketch.clone();
    let repeated_use = duplicate_profile.profiles[0][0].clone();
    duplicate_profile
        .profiles
        .edit(|profiles| profiles[0].push(repeated_use))
        .expect("valid test fixture");
    assert!(!sketch_profiles_cover_generated_extrusion_sides(ctx,
        &scan,
        &definition,
        7,
        &duplicate_profile,
    ).expect("admitted profile coverage"));

    let duplicate = scan.surfaces.rows[0].clone();
    scan.surfaces.rows.push(duplicate);
    assert!(!sketch_profiles_cover_generated_extrusion_sides(ctx,
        &scan,
        &definition,
        7,
        &sketch,
    ).expect("admitted profile coverage"));
    });
}

#[test]
fn generated_side_coverage_accepts_explicit_rowless_results() {
    crate::decode::with_test_decode_ctx(|ctx| {
    let definition = definition();
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    let mut table = generated_side_table();
    let cap = |entity_id, class_id| crate::feature::entity::FeatureEntityTableEntry {
        payload: crate::feature::entity::entry_payload(class_id, None, None, None),

        entity_id,
        prefixed: false,
        offset: 0,
        end_offset: 0,
    };
    let materialized = crate::feature::entity::FeatureEntityTableEntry {
        entity_id: 32,
        payload: crate::feature::entity::entry_payload(200, Some(13), None, None),
        prefixed: false,
        offset: 0,
        end_offset: 0,
    };
    table.entries = vec![
        cap(29, 204),
        cap(30, 203),
        table.entries[0].clone(),
        materialized,
    ];
    table.mark_surface_ids([29, 30, 32]);
    scan.features.entity_tables.push(table);
    scan.surfaces
        .rows
        .extend([surface_row(29, 7, crate::surface::SurfaceKind::Plane)]);
    scan.surfaces
        .rows
        .extend([surface_row(30, 7, crate::surface::SurfaceKind::Plane)]);
    scan.surfaces
        .rows
        .extend([surface_row(32, 7, crate::surface::SurfaceKind::Plane)]);
    let mut sketch = sketch();
    sketch
        .profiles
        .edit(|profiles| {
            profiles[0].push(SketchEntityUse {
                entity: SketchEntityId::mint("creo:featdefs:sketch_entity#7:13".to_string())
                    .expect("valid test fixture"),
                reversed: false,
            });
        })
        .expect("valid test fixture");

    assert!(sketch_profiles_cover_generated_extrusion_sides(ctx,
        &scan,
        &definition,
        7,
        &sketch,
    ).expect("admitted profile coverage"));
    });
}
