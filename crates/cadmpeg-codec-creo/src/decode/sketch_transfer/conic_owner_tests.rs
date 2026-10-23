// SPDX-License-Identifier: Apache-2.0
//! Independently authored owner/admission and incidence-fallback fixtures.

use super::entities::{transfer_section_entities, SectionEntityTransfer};
use crate::feature::definitions::*;
use crate::feature::segment_rows::SegmentRow;
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinitionInput, SketchEntity, SketchGeometryDefinition,
    SketchId,
};
use std::collections::{BTreeMap, BTreeSet};

fn opaque_owner(id: u32, selector: u32, offset: usize) -> SegmentRow {
    SegmentRow::Opaque(FeatureOpaqueSegment {
        kind: 58,
        directions: [Some(0); 3],
        point_ids: [None, Some(301)],
        center_id: Some(302),
        arc_orientation: Some(0),
        vertical_horizontal: Some(selector),
        radius_ref: Some(303),
        radius2_ref: Some(304),
        external_id: id,
        body: Vec::new(),
        offset,
    })
}

fn definition(circles: bool, selector: u32) -> FeatureDefinition {
    // These are authored source-model rows, not serialized corpus records.
    let rows = if circles {
        vec![
            SegmentRow::Circle(FeatureCircleSegment {
                center_id: 301,
                radius_ref: 303,
                external_id: 41,
                offset: 400,
            }),
            SegmentRow::Circle(FeatureCircleSegment {
                center_id: 302,
                radius_ref: 304,
                external_id: 52,
                offset: 440,
            }),
        ]
    } else {
        vec![
            opaque_owner(41, selector, 400),
            opaque_owner(52, selector, 440),
        ]
    };
    FeatureDefinition {
        identity: DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(71),
            owner_feature_id: None,
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: Some(FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: rows.into_iter().collect(),
            offset: 360,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: Some(FeatureRelationTable {
            declared_count: 1,
            entity_ref: None,
            rows: Vec::new(),
            skamps: Some(SolverSubtable::Declared {
                header: FeatureSolverTableHeader {
                    declared_count: 1,
                    entity_ref: 91,
                    offset: 480,
                },
                rows: vec![FeatureSkamp {
                    id: 9,
                    kind: 0,
                    flags: 0,
                    status: 35,
                    items: vec![
                        FeatureSkampItem {
                            entity_id: 52,
                            sense: 4,
                        },
                        FeatureSkampItem {
                            entity_id: 41,
                            sense: 4,
                        },
                    ],
                    offset: 500,
                }],
            }),
            triples: None,
            offset: 470,
        }),
        saved_section: None,
        offset: 320,
    }
}

fn emit(definition: &FeatureDefinition) -> Vec<SketchEntity> {
    let scan = crate::container::scan_bytes_ok(crate::test_support::build_prt("conic-owner", &[]));
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let sketch = SketchId::mint("creo:model:sketch#offset:731").expect("authored sketch");
    let table = definition.segments.as_ref().expect("authored source table");
    let unique: BTreeSet<_> = table.rows.unique_ids().collect();
    let ambiguous = table.rows.ids().filter(|id| !unique.contains(id)).collect();
    let empty_ids = BTreeSet::new();
    let empty_profiles = BTreeSet::new();
    let empty_geometries = BTreeMap::new();
    let empty_solved_geometries = BTreeMap::new();
    let mut losses = Vec::new();
    let mut source_carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    let (entities, profiles) = crate::decode::with_service_context(|ctx| {
        transfer_section_entities(
            ctx,
            SectionEntityTransfer {
                scan: &scan,
                ir: &mut ir,
                annotations: &mut annotations,
                definition,
                transform: None,
                sketch_id: &sketch,
                segments: &[],
                unique_segment_ids: &unique,
                unique_saved_ids: &empty_ids,
                ambiguous_segment_ids: &ambiguous,
                complete_segment_table: table.is_complete(),
                solved: &empty_ids,
                segment_geometries: &empty_solved_geometries,
                resolved_segment_geometries: &empty_solved_geometries,
                circle_geometries: &empty_geometries,
                point_geometries: &empty_geometries,
                centered_line_geometries: &empty_geometries,
                reference_line_geometries: &empty_geometries,
                materialized_saved_section_external_ids: &empty_ids,
                profiles: Vec::new(),
                profile_entities: &empty_profiles,
                losses: &mut losses,
                source_carriers: &mut source_carriers,
            },
        )
    })
    .expect("emit authored source rows");
    assert!(profiles.is_empty());
    assert!(ir.model.curves.is_empty());
    assert!(ir.model.surfaces.is_empty());
    assert!(entities.iter().all(|entity| entity.geometry_ref.is_none()));
    entities
}

fn emitted_kind(entity: &SketchEntity) -> &str {
    let SketchGeometryDefinition::Native { native_kind } = entity.geometry.definition() else {
        panic!("no numerical carrier is established by this fixture");
    };
    native_kind.as_str()
}

pub(in crate::decode) fn authored_conic_incidence() -> Vec<SketchConstraint> {
    let definition = definition(false, 3);
    let entities = emit(&definition);
    assert_eq!(entities.len(), 2);
    assert!(entities
        .iter()
        .all(|entity| emitted_kind(entity) == "conic"));
    let geometry = entities
        .into_iter()
        .map(|entity| (entity.id().clone(), entity.geometry))
        .collect();
    let sketch = SketchId::mint("creo:model:sketch#offset:731").expect("authored sketch");
    crate::decode::with_service_context(|ctx| {
        super::skamp_constraints::section_skamp_constraints_for_geometry(
            ctx,
            &definition,
            &sketch,
            Some(&geometry),
        )
    })
    .expect("retain source-backed incidence")
    .into_iter()
    .map(|(constraint, _)| constraint)
    .collect()
}

#[test]
fn complete_conic_owners_emit_conic_for_every_uninterpreted_selector() {
    for selector in [0, 2, 3, 4] {
        let entities = emit(&definition(false, selector));
        assert_eq!(entities.len(), 2);
        assert!(entities
            .iter()
            .all(|entity| emitted_kind(entity) == "conic"));
    }
}

#[test]
fn incomplete_or_duplicate_opaque_conic_owners_are_not_positive_admission() {
    let mut incomplete = definition(false, 0);
    incomplete.segments.as_mut().expect("table").declared_count = 3;
    let entities = emit(&incomplete);
    assert_eq!(entities.len(), 2);
    assert!(entities
        .iter()
        .all(|entity| emitted_kind(entity) == "segment_type:58"));

    let mut duplicate = definition(false, 0);
    let table = duplicate.segments.as_mut().expect("table");
    table.rows.insert(opaque_owner(41, 4, 460));
    table.declared_count = 3;
    assert!(table.is_complete());
    let entities = emit(&duplicate);
    assert_eq!(entities.len(), 3);
    let owned = entities
        .iter()
        .filter(|entity| emitted_kind(entity) == "conic")
        .count();
    assert_eq!(
        owned, 1,
        "only the independently unique external ID remains admitted"
    );
    assert_eq!(
        entities
            .iter()
            .filter(|entity| emitted_kind(entity) == "segment_type:58")
            .count(),
        2
    );
}

#[test]
fn explicit_circle_owner_still_allows_source_backed_concentric_constraint() {
    let definition = definition(true, 0);
    let entities = emit(&definition);
    assert_eq!(entities.len(), 2);
    assert!(entities
        .iter()
        .all(|entity| emitted_kind(entity) == "circle"));
    let geometry = entities
        .into_iter()
        .map(|entity| (entity.id().clone(), entity.geometry))
        .collect();
    let sketch = SketchId::mint("creo:model:sketch#offset:731").expect("authored sketch");
    let constraints = crate::decode::with_service_context(|ctx| {
        super::skamp_constraints::section_skamp_constraints_for_geometry(
            ctx,
            &definition,
            &sketch,
            Some(&geometry),
        )
    })
    .expect("explicit circular owner control");
    assert_eq!(constraints.len(), 1);
    let SketchConstraintDefinitionInput::Concentric { first, second } =
        constraints[0].0.definition.kind()
    else {
        panic!("typed circle owners support the original concentric contract");
    };
    assert_eq!(
        first,
        &crate::decode::sketch_ids::sketch_entity_id(&sketch, 52).expect("first")
    );
    assert_eq!(
        second,
        &crate::decode::sketch_ids::sketch_entity_id(&sketch, 41).expect("second")
    );
}

#[test]
fn conic_incidence_retains_exact_native_items_instead_of_false_concentric() {
    let constraints = authored_conic_incidence();
    assert_eq!(constraints.len(), 1);
    let constraint = &constraints[0];
    assert!(constraint.id.as_str().ends_with(":skamp:9"));
    assert_eq!(constraint.active, Some(true));
    let SketchConstraintDefinitionInput::Native {
        native_kind,
        native_state,
        native_flags,
        native_properties,
        entities,
        parameter,
        operands,
    } = constraint.definition.kind()
    else {
        panic!("conic coefficients do not prove circularity");
    };
    assert_eq!(native_kind, "creo:skamp:0");
    assert_eq!(*native_state, Some(35));
    assert_eq!(*native_flags, Some(0));
    assert!(native_properties.is_empty());
    assert!(parameter.is_none());
    let sketch = SketchId::mint("creo:model:sketch#offset:731").expect("authored sketch");
    assert_eq!(
        entities,
        &vec![
            crate::decode::sketch_ids::sketch_entity_id(&sketch, 52).expect("first"),
            crate::decode::sketch_ids::sketch_entity_id(&sketch, 41).expect("second"),
        ]
    );
    assert_eq!(
        operands.len(),
        2,
        "no unowned equation reference is invented"
    );
    let reference = crate::decode::sketch_ids::sketch_native_ref(&sketch);
    for (operand, id) in operands.iter().zip([52, 41]) {
        assert_eq!(operand.native_kind, "skamp_ptr");
        let field = operand.field.as_ref().expect("source field role");
        assert_eq!(field.name, "items.entity_id");
        assert_eq!(field.role, Some(4));
        assert_eq!(operand.object_index, Some(id));
        assert_eq!(operand.native_ref.as_ref(), Some(&reference));
    }
}
