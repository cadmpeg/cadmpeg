// SPDX-License-Identifier: Apache-2.0

use super::sketch_profiles_cover_generated_extrusion_sides;
use cadmpeg_ir::sketches::{Sketch, SketchEntityId, SketchEntityUse, SketchId, SketchPlacement};

fn definition() -> crate::feature::FeatureDefinition {
    crate::feature::FeatureDefinition {
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

fn surface_row(
    id: u32,
    feature_id: u32,
    kind: crate::surface::SurfaceKind,
) -> crate::surface::SurfaceRow {
    crate::surface::SurfaceRow {
        id,
        kind,
        feature_id,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    }
}

fn generated_side_table() -> crate::feature::FeatureEntityTable {
    crate::feature::FeatureEntityTable {
        feature_id: 7,
        table_class_id: 29,
        entries: vec![crate::feature::FeatureEntityTableEntry {
            entity_id: 31,
            payload: crate::feature::entry_payload(200, Some(11), None, None),
            prefixed: false,
            offset: 0,
            end_offset: 0,
            is_surface: false,
        }],
        offset: 0,
    }
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
        placement: SketchPlacement::Unresolved,
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![SketchEntityUse {
            entity,
            reversed: false,
        }]])
        .expect("valid test fixture"),
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
    duplicate_profile
        .profiles
        .edit(|profiles| profiles[0].push(repeated_use))
        .expect("valid test fixture");
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
        payload: crate::feature::entry_payload(class_id, None, None, None),

        entity_id,
        prefixed: false,
        offset: 0,
        end_offset: 0,
        is_surface: false,
    };
    let materialized = crate::feature::FeatureEntityTableEntry {
        entity_id: 32,
        payload: crate::feature::entry_payload(200, Some(13), None, None),
        prefixed: false,
        offset: 0,
        end_offset: 0,
        is_surface: false,
    };
    table.entries = vec![
        cap(29, 204),
        cap(30, 203),
        table.entries[0].clone(),
        materialized,
    ];
    for entry in &mut table.entries {
        entry.is_surface = matches!(entry.entity_id, 29 | 30 | 32);
    }
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

    assert!(sketch_profiles_cover_generated_extrusion_sides(
        &scan,
        &definition,
        7,
        &sketch,
    ));
}
