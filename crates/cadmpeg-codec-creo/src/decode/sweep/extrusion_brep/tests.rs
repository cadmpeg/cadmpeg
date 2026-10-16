// SPDX-License-Identifier: Apache-2.0

use super::{
    cap_coedge_ids, copy_ring_coedges, generated_extrusion_identity, push_rejected_extrusion,
    refused_lane_message, sketch_profiles_cover_generated_extrusion_sides, JoinedLaneRecords,
};
use crate::decode::tests::surface_row;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchEntityId, SketchEntityUse, SketchId, SketchPlacement};
use cadmpeg_ir::AnnotationBuilder;

#[test]
fn extrusion_refused_lane_message_joins_records() {
    let records = ["first".to_string(), "second".to_string()];
    assert_eq!(
        refused_lane_message("cap 7", &records),
        "Refused lanes on cap 7: first; second"
    );
}

fn admitted_extrusion_fixture() -> (crate::container::ContainerScan<'static>, CadIr) {
    use cadmpeg_ir::sketches::{
        SketchEntity, SketchGeometry, SketchGeometryDefinition, SketchProfiles,
    };
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 7,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Protrusion),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("feature row body"),
        body_offset: 1,
        offset: 0,
    });
    scan.features.definitions.push(definition());
    scan.features.section_transforms.push(
        crate::placement::FeatureSectionTransform::new(
            7,
            Some(7),
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            0,
        )
        .expect("section frame"),
    );
    scan.features
        .operations
        .push(crate::feature::operations::FeatureOperation {
            feature_id: 7,
            kind: crate::feature::operations::OperationKind::Extrude,
            name: crate::feature::operations::OperationName::Derived,
            recipe: crate::feature::operations::RecipeResolution::Resolved(
                crate::feature::operations::FeatureRecipe::ProtrudeExtrude,
            ),
            display_state_conflict: false,
            depdb: None,
            offset: 0,
            state_offset: 0,
        });
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            7,
            29,
            vec![
                crate::feature::entity::FeatureEntityTableEntry {
                    entity_id: 31,
                    payload: crate::feature::entity::entry_payload(204, None, None, None),
                    prefixed: false,
                    offset: 0,
                    end_offset: 0,
                },
                crate::feature::entity::FeatureEntityTableEntry {
                    entity_id: 32,
                    payload: crate::feature::entity::entry_payload(203, None, None, None),
                    prefixed: false,
                    offset: 0,
                    end_offset: 0,
                },
                crate::feature::entity::FeatureEntityTableEntry {
                    entity_id: 33,
                    payload: crate::feature::entity::entry_payload(200, Some(11), None, None),
                    prefixed: false,
                    offset: 0,
                    end_offset: 0,
                },
                crate::feature::entity::FeatureEntityTableEntry {
                    entity_id: 34,
                    payload: crate::feature::entity::entry_payload(200, Some(12), None, None),
                    prefixed: false,
                    offset: 0,
                    end_offset: 0,
                },
                crate::feature::entity::FeatureEntityTableEntry {
                    entity_id: 35,
                    payload: crate::feature::entity::entry_payload(200, Some(13), None, None),
                    prefixed: false,
                    offset: 0,
                    end_offset: 0,
                },
                crate::feature::entity::FeatureEntityTableEntry {
                    entity_id: 36,
                    payload: crate::feature::entity::entry_payload(200, Some(14), None, None),
                    prefixed: false,
                    offset: 0,
                    end_offset: 0,
                },
            ],
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([31, 32, 33, 34, 35, 36]),
    );
    let row = |id| crate::surface::SurfaceRow {
        id,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 7,
        reversed: id == 31,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: usize::try_from(id).expect("fixture index fits usize"),
    };
    scan.surfaces.rows.extend((31..=36).map(row));
    let plane = |id, z| Surface {
        id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("surface identity"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, z),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("plane"),
        )),
        source_object: None,
    };
    let mut ir = CadIr::empty();
    ir.model.surfaces.extend([plane(31, 0.0), plane(32, 2.0)]);
    let sketch_id = SketchId::mint("creo:model:sketch#7").expect("sketch identity");
    let mut uses = Vec::new();
    for (index, (start, end)) in [
        ([0.0, 0.0], [1.0, 0.0]),
        ([1.0, 0.0], [1.0, 1.0]),
        ([1.0, 1.0], [0.0, 1.0]),
        ([0.0, 1.0], [0.0, 0.0]),
    ]
    .into_iter()
    .enumerate()
    {
        let id = SketchEntityId::mint(format!("creo:featdefs:sketch_entity#7:{}", index + 11))
            .expect("sketch entity identity");
        ir.model.sketch_entities.push(SketchEntity::new(
            id.clone(),
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start[0], start[1]),
                end: Point2::new(end[0], end[1]),
            })
            .expect("line"),
        ));
        uses.push(SketchEntityUse {
            entity: id,
            reversed: false,
        });
    }
    ir.model.sketches.push(Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: SketchProfiles::try_from(vec![uses]).expect("profile"),
        native_ref: None,
    });
    (scan, ir)
}

#[test]
fn closed_extrusion_reaches_brep_admission() {
    let (scan, mut ir) = admitted_extrusion_fixture();
    assert!(super::feature_allows_additive_linear_extrusion(&scan, 7));
    assert!(super::feature_is_first_material_operation(&scan, 7));
    let definition = &scan.features.definitions[0];
    let transform = &scan.features.section_transforms[0];
    let sketch_id = super::model_sketch_id(&scan, definition).expect("sketch ID admitted");
    crate::decode::with_service_context(|ctx| {
        assert!(super::resolved_feature_extrusion_span(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            definition,
            transform
        )
        .expect("compute the resolved feature extrusion span")
        .is_some());
        assert!(super::sketch_profiles_cover_generated_extrusion_sides(
            &scan,
            definition,
            7,
            &ir.model.sketches[0]
        ));
        let profiles = super::resolved_sketch_profiles(
            ctx,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            &sketch_id,
            1,
        )
        .expect("compute the resolved sketch profiles")
        .expect("profile geometry");
        assert!(super::ordered_extrusion_profiles(ctx, profiles)
            .expect("compute the ordered extrusion profiles")
            .is_some());
    });
    let mut diagnostics = crate::decode::surfaces::brep::BrepTransferDiagnostics::default();
    let count = crate::decode::with_service_context(|ctx| {
        super::transfer_resolved_extrusion_breps(
            ctx,
            &scan,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &mut diagnostics,
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("admitted transfer");
    assert_eq!(count, 1);
    assert_eq!(ir.model.bodies.len(), 1);
    assert!(diagnostics.rejected_extrusion_bodies.is_empty());
}

#[test]
fn extrusion_cap_of_an_arc_stored_with_extra_turns_traces_one_sweep() {
    use cadmpeg_ir::sketches::{SketchEntity, SketchGeometry, SketchGeometryDefinition};
    // A half turn plus 30_000 full turns sweeps the same half turn as the
    // profile area and the edge curves, so the cap stays within two
    // quarter-turn segments and the extrusion transfers.
    let (scan, mut ir) = admitted_extrusion_fixture();
    let entity = &ir.model.sketch_entities[2];
    let arc = SketchEntity::new(
        entity.id().clone(),
        entity.sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(0.5, 1.0),
            radius: cadmpeg_ir::scalar::Length::new(0.5).expect("radius"),
            start_angle: cadmpeg_ir::scalar::Angle::new(0.0).expect("start angle"),
            end_angle: cadmpeg_ir::scalar::Angle::new(
                std::f64::consts::PI + 30_000.0 * std::f64::consts::TAU,
            )
            .expect("end angle"),
        })
        .expect("arc"),
    );
    ir.model.sketch_entities[2] = arc;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("empty root admitted");
    let mut diagnostics = crate::decode::surfaces::brep::BrepTransferDiagnostics::default();
    let count = super::transfer_resolved_extrusion_breps(
        &ctx,
        &scan,
        &mut ir,
        &mut AnnotationBuilder::new(),
        &mut diagnostics,
        &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
    .expect("admitted transfer");
    assert_eq!(count, 1);
    assert_eq!(ir.model.bodies.len(), 1);
    assert!(diagnostics.rejected_extrusion_bodies.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
    assert!(ctx.finish_session().is_ok());
}

fn rejected_extrusion() -> crate::decode::surfaces::brep::BrepTransferDiagnostics {
    let mut diagnostics = crate::decode::surfaces::brep::BrepTransferDiagnostics::default();
    let records = [
        "first refused lane".to_string(),
        "second refused lane".to_string(),
    ];
    push_rejected_extrusion(
        &mut diagnostics,
        cadmpeg_ir::ids::BodyId::mint("creo:feature:extrusion#7:body")
            .expect("valid body identity"),
        format_args!(
            "refused extrusion side lanes: {}",
            JoinedLaneRecords(&records)
        ),
    );
    diagnostics
}

#[test]
fn extrusion_rejection_preserves_joined_reason_order() {
    let diagnostics = rejected_extrusion();
    assert_eq!(
        diagnostics.rejected_extrusion_bodies[0].1,
        "refused extrusion side lanes: first refused lane; second refused lane"
    );
}

#[test]
fn generated_extrusion_identity_composes_feature_key() {
    let id = generated_extrusion_identity::<cadmpeg_ir::ids::BodyId>(format_args!("7:body"))
        .expect("generated identity");
    assert_eq!(
        id.as_str(),
        cadmpeg_ir::ids::BodyId::compose(
            &crate::identity::FEATURE_EXTRUSION,
            cadmpeg_ir::ids::IdentityKey::from(7).colon(cadmpeg_ir::identity_key!("body")),
        )
        .as_str()
    );
}

#[test]
fn extrusion_entity_id_copy_preserves_identity() {
    let source = cadmpeg_ir::ids::BodyId::compose(
        &crate::identity::FEATURE_EXTRUSION,
        cadmpeg_ir::ids::IdentityKey::from(7).colon(cadmpeg_ir::identity_key!("body")),
    );
    let copied = crate::identity::copy_checked_id::<cadmpeg_ir::ids::BodyId>(source.as_str())
        .expect("identity copy");
    assert_eq!(copied, source);
}

#[test]
fn cap_coedge_ids_preserve_ring_order() {
    let bottom = cap_coedge_ids(7, 0, 2, "bottom-cap", true).expect("bottom ring");
    let top = cap_coedge_ids(7, 0, 2, "top-cap", false).expect("top ring");
    assert!(bottom[0].as_str().ends_with("#7:coedge:0:1:bottom-cap"));
    assert!(bottom[1].as_str().ends_with("#7:coedge:0:0:bottom-cap"));
    assert!(top[0].as_str().ends_with("#7:coedge:0:0:top-cap"));
    assert!(top[1].as_str().ends_with("#7:coedge:0:1:top-cap"));
}

#[test]
fn ring_copy_preserves_coedge_identity_order() {
    let ids =
        copy_ring_coedges(&[cadmpeg_ir::ids::CoedgeId::mint("creo:brep:coedge#10:0")
            .expect("valid coedge identity")])
        .expect("ring copy");
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
        section_3d: Some(crate::feature::definitions::FeatureSection3d {
            sketch_plane_entity_id: None,
            sketch_plane_flip: None,
            reference_planes: crate::feature::definitions::ReferencePlanes::Named(Vec::new()),
            reference_plane_datum_geometry_id: None,
            orientation: crate::feature::definitions::FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 0,
        }),
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

#[test]
fn generated_side_coverage_rejects_duplicate_surface_rows() {
    {
        let definition = definition();
        let mut scan = crate::test_support::empty_container_scan();
        scan.features.entity_tables.push(generated_side_table());
        scan.surfaces
            .rows
            .push(surface_row(31, 7, crate::surface::SurfaceKind::Plane));
        let sketch = sketch();

        assert!(sketch_profiles_cover_generated_extrusion_sides(
            &scan,
            &definition,
            7,
            &sketch
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
            &duplicate_profile
        ));

        let duplicate = scan.surfaces.rows[0].clone();
        scan.surfaces.rows.push(duplicate);
        assert!(!sketch_profiles_cover_generated_extrusion_sides(
            &scan,
            &definition,
            7,
            &sketch
        ));
    };
}

#[test]
fn generated_side_coverage_accepts_explicit_rowless_results() {
    {
        let definition = definition();
        let mut scan = crate::test_support::empty_container_scan();
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

        assert!(sketch_profiles_cover_generated_extrusion_sides(
            &scan,
            &definition,
            7,
            &sketch
        ));
    };
}
