// SPDX-License-Identifier: Apache-2.0

use super::sketch_profiles_cover_generated_extrusion_sides;
use cadmpeg_ir::sketches::{Sketch, SketchEntityId, SketchEntityUse, SketchId, SketchPlacement};

fn definition() -> crate::feature::FeatureDefinition {
    crate::feature::FeatureDefinition {
        id: 7,
        owner_feature_id: Some(7),
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

fn surface_row(
    id: u32,
    feature_id: u32,
    kind: crate::surface::SurfaceKind,
) -> crate::surface::SurfaceRow {
    crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    }
}

fn generated_side_table() -> crate::feature::FeatureEntityTable {
    crate::feature::FeatureEntityTable {
        feature_id: Some(7),
        table_class_id: 29,
        entry_ids: vec![31],
        entries: vec![crate::feature::FeatureEntityTableEntry {
            entity_id: 31,
            class_id: 200,
            source_entity_id: Some(11),
            related_entity_id: None,
            related_entity_state: None,
            prefixed: false,
            offset: 0,
            end_offset: 0,
        }],
        surface_ids: vec![31],
        non_surface_entity_ids: Vec::new(),
        offset: 0,
    }
}

fn sketch() -> Sketch {
    let sketch_id = SketchId("creo:model:sketch#7".to_string());
    let entity = SketchEntityId("creo:featdefs:sketch_entity#7:11".to_string());
    Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved,
        profiles: vec![vec![SketchEntityUse {
            entity,
            reversed: false,
        }]],
        native_ref: None,
    }
}

#[test]
fn generated_side_coverage_rejects_duplicate_surface_rows() {
    let definition = definition();
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.entity_tables.push(generated_side_table());
    scan.surfaces
        .rows
        .push(surface_row(31, 7, crate::surface::SurfaceKind::Plane));
    let sketch = sketch();

    assert!(sketch_profiles_cover_generated_extrusion_sides(
        &scan,
        &definition,
        7,
        &sketch,
    ));

    let mut duplicate_profile = sketch.clone();
    let repeated_use = duplicate_profile.profiles[0][0].clone();
    duplicate_profile.profiles[0].push(repeated_use);
    assert!(!sketch_profiles_cover_generated_extrusion_sides(
        &scan,
        &definition,
        7,
        &duplicate_profile,
    ));

    let duplicate = scan.surfaces.rows[0].clone();
    scan.surfaces.rows.push(duplicate);
    assert!(!sketch_profiles_cover_generated_extrusion_sides(
        &scan,
        &definition,
        7,
        &sketch,
    ));
}

#[test]
fn generated_side_coverage_accepts_explicit_rowless_results() {
    let definition = definition();
    let mut scan = crate::container::scan_bytes(Vec::new());
    let mut table = generated_side_table();
    let cap = |entity_id, class_id| crate::feature::FeatureEntityTableEntry {
        entity_id,
        class_id,
        source_entity_id: None,
        related_entity_id: None,
        related_entity_state: None,
        prefixed: false,
        offset: 0,
        end_offset: 0,
    };
    let materialized = crate::feature::FeatureEntityTableEntry {
        entity_id: 32,
        class_id: 200,
        source_entity_id: Some(13),
        related_entity_id: None,
        related_entity_state: None,
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
    table.entry_ids = table.entries.iter().map(|entry| entry.entity_id).collect();
    table.surface_ids = vec![29, 30, 32];
    table.non_surface_entity_ids = vec![31];
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
    sketch.profiles[0].push(SketchEntityUse {
        entity: SketchEntityId("creo:featdefs:sketch_entity#7:13".to_string()),
        reversed: false,
    });

    assert!(sketch_profiles_cover_generated_extrusion_sides(
        &scan,
        &definition,
        7,
        &sketch,
    ));
}
