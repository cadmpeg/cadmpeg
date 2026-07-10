// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
//! Unit tests for the IR: the worked cube validates clean, JSON round-trips,
//! and each validation check actually fires when its invariant is broken.

use crate::examples::unit_cube;
use crate::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use crate::ids::{CoedgeId, CurveId, EdgeId, SurfaceId, UnknownId};
use crate::math::{Point3, Vector3};
use crate::provenance::{EntityMeta, Exactness};
use crate::report::Check;
use crate::unknown::UnknownRecord;
use crate::validate::validate;
use crate::{diff, CadIr};

/// Replace the surface of the cube's first face with an unknown surface,
/// optionally linking a preserved record, and return the face id and its
/// surface id. Leaves every loop/coedge/edge of the face intact.
fn make_first_face_surface_unknown(ir: &mut crate::CadIr, record: Option<UnknownId>) -> String {
    let face = &ir.faces[0];
    let surface_id = face.surface.0.clone();
    for s in &mut ir.surfaces {
        if s.id.0 == surface_id {
            s.geometry = SurfaceGeometry::Unknown { record };
            s.meta.exactness = Exactness::Unknown;
            break;
        }
    }
    surface_id
}

#[test]
fn face_on_unknown_surface_validates_clean() {
    let mut ir = unit_cube();
    // Preserve a raw record and point the unknown surface at it.
    let rec = UnknownId("u0".into());
    ir.unknowns.push(UnknownRecord {
        id: rec.clone(),
        offset: 0,
        byte_len: 16,
        sha256: "0".repeat(64),
        data: None,
        links: Vec::new(),
        meta: EntityMeta::synthetic(),
    });
    make_first_face_surface_unknown(&mut ir, Some(rec));

    let report = validate(&ir, Vec::new());
    assert!(
        report.is_ok(),
        "a face on an unknown surface is legal, got: {:?}",
        report.findings
    );
    // The face and its topology stay in the graph.
    assert_eq!(ir.faces.len(), 6);
    // The situation is surfaced as a count.
    assert_eq!(
        report.entity_counts.get("surfaces_unknown_geometry"),
        Some(&1)
    );
}

#[test]
fn unknown_surface_without_record_is_legal() {
    let mut ir = unit_cube();
    make_first_face_surface_unknown(&mut ir, None);
    let report = validate(&ir, Vec::new());
    assert!(
        report.is_ok(),
        "an unknown surface need not preserve bytes, got: {:?}",
        report.findings
    );
    assert_eq!(
        report.entity_counts.get("surfaces_unknown_geometry"),
        Some(&1)
    );
}

#[test]
fn unknown_surface_dangling_record_is_flagged() {
    let mut ir = unit_cube();
    // Link a record id that is not in the unknowns arena.
    make_first_face_surface_unknown(&mut ir, Some(UnknownId("missing".into())));
    let report = validate(&ir, Vec::new());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == Check::ReferentialIntegrity),
        "expected a referential-integrity finding for the dangling record, got: {:?}",
        report.findings
    );
    assert!(!report.is_ok());
}

#[test]
fn unknown_surface_json_round_trips() {
    let mut ir = unit_cube();
    let rec = UnknownId("u0".into());
    ir.unknowns.push(UnknownRecord {
        id: rec.clone(),
        offset: 0,
        byte_len: 16,
        sha256: "0".repeat(64),
        data: None,
        links: Vec::new(),
        meta: EntityMeta::synthetic(),
    });
    make_first_face_surface_unknown(&mut ir, Some(rec));

    let json = ir.to_canonical_json().unwrap();
    let parsed = crate::CadIr::from_json(&json).unwrap();
    assert_eq!(parsed, ir, "round-trip must preserve the unknown surface");
}

#[test]
fn unit_cube_has_expected_census() {
    let ir = unit_cube();
    assert_eq!(ir.bodies.len(), 1);
    assert_eq!(ir.lumps.len(), 1);
    assert_eq!(ir.shells.len(), 1);
    assert_eq!(ir.faces.len(), 6);
    assert_eq!(ir.loops.len(), 6);
    assert_eq!(ir.coedges.len(), 24);
    assert_eq!(ir.edges.len(), 12);
    assert_eq!(ir.vertices.len(), 8);
    assert_eq!(ir.points.len(), 8);
    assert_eq!(ir.surfaces.len(), 6);
    assert_eq!(ir.curves.len(), 12);
}

#[test]
fn unit_cube_validates_clean() {
    let ir = unit_cube();
    let report = validate(&ir, Vec::new());
    assert!(
        report.is_ok(),
        "cube should have no error findings, got: {:?}",
        report.findings
    );
    assert_eq!(report.error_count(), 0);
    assert_eq!(report.warning_count(), 0);
    assert_eq!(report.entity_counts.get("coedges"), Some(&24));
}

#[test]
fn arena_registry_drives_counts_and_diff_dispatch() {
    let ir = unit_cube();
    let report = validate(&ir, Vec::new());
    let diff_kinds = diff(&ir, &ir)
        .per_arena
        .into_iter()
        .map(|arena| arena.kind)
        .collect::<Vec<_>>();

    assert_eq!(diff_kinds, CadIr::arena_names());
    for name in CadIr::arena_names() {
        assert!(
            report.entity_counts.contains_key(*name),
            "entity counts omitted registered arena {name}"
        );
    }
}

#[test]
fn every_cube_edge_has_two_opposite_sense_coedges() {
    let ir = unit_cube();
    for edge in &ir.edges {
        let coedges: Vec<_> = ir.coedges.iter().filter(|c| c.edge == edge.id).collect();
        assert_eq!(coedges.len(), 2, "edge {} should have 2 coedges", edge.id);
        assert_ne!(
            coedges[0].sense, coedges[1].sense,
            "edge {} coedges should have opposite sense",
            edge.id
        );
        // Partners point at each other.
        assert_eq!(coedges[0].partner.as_ref(), Some(&coedges[1].id));
        assert_eq!(coedges[1].partner.as_ref(), Some(&coedges[0].id));
    }
}

#[test]
fn json_round_trips_and_is_deterministic() {
    let ir = unit_cube();
    let json1 = ir.to_canonical_json().unwrap();
    let json2 = ir.to_canonical_json().unwrap();
    assert_eq!(json1, json2, "serialization must be deterministic");

    let parsed = crate::CadIr::from_json(&json1).unwrap();
    assert_eq!(parsed, ir, "round-trip must preserve the document");
    assert_eq!(parsed.to_canonical_json().unwrap(), json1);
}

#[test]
fn appearance_asset_and_binding_round_trip() {
    use crate::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use crate::ids::AppearanceId;

    let mut ir = unit_cube();
    let body = ir.bodies[0].id.clone();
    ir.appearances.push(Appearance {
        id: AppearanceId("appearance:prism-001".into()),
        name: Some("Prism-001".into()),
        asset_guid: Some("visual-guid".into()),
        visual_guid: Some("visual-guid".into()),
        physical_token: Some("physical-token".into()),
        schema: Some("GenericSchema".into()),
        category: None,
        base_color: Some(crate::topology::Color {
            r: 0.1,
            g: 0.2,
            b: 0.3,
            a: 1.0,
        }),
        properties: std::collections::BTreeMap::new(),
        meta: EntityMeta::synthetic(),
    });
    ir.appearance_bindings.push(AppearanceBinding {
        target: AppearanceTarget::Body(body),
        appearance: AppearanceId("appearance:prism-001".into()),
        source_entity_id: Some("0_1".into()),
        object_type: Some("Body".into()),
        channels: std::collections::BTreeMap::new(),
        meta: EntityMeta::synthetic(),
    });

    let json = ir.to_canonical_json().unwrap();
    let decoded = CadIr::from_json(&json).unwrap();
    assert_eq!(decoded.appearances, ir.appearances);
    assert_eq!(decoded.appearance_bindings, ir.appearance_bindings);
}

#[test]
fn dangling_reference_is_flagged() {
    let mut ir = unit_cube();
    // Point a coedge's edge at something that does not exist.
    ir.coedges[0].edge = EdgeId("does-not-exist".into());
    let report = validate(&ir, Vec::new());
    assert!(report
        .findings
        .iter()
        .any(|f| f.check == Check::ReferentialIntegrity));
    assert!(!report.is_ok());
}

#[test]
fn broken_loop_ring_is_flagged() {
    let mut ir = unit_cube();
    // Redirect a coedge's `next` to a valid coedge in a different loop, so the
    // referenced id resolves but the ring no longer closes.
    let foreign = ir.coedges[20].id.clone();
    ir.coedges[0].next = foreign;
    let report = validate(&ir, Vec::new());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == Check::LoopClosure),
        "expected a loop-closure finding, got: {:?}",
        report.findings
    );
}

#[test]
fn mismatched_partner_edge_is_flagged() {
    let mut ir = unit_cube();
    // Force a coedge's partner to reference a coedge on a different edge by
    // repointing the partner's edge. Find coedge[0]'s partner and change it.
    let partner_id: CoedgeId = ir.coedges[0].partner.clone().unwrap();
    let other_edge = ir
        .coedges
        .iter()
        .find(|c| c.edge != ir.coedges[0].edge)
        .unwrap()
        .edge
        .clone();
    for c in &mut ir.coedges {
        if c.id == partner_id {
            c.edge = other_edge.clone();
        }
    }
    let report = validate(&ir, Vec::new());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == Check::CoedgePairing),
        "expected a coedge-pairing finding, got: {:?}",
        report.findings
    );
}

#[test]
fn signed_sphere_radius_is_valid() {
    let mut ir = unit_cube();
    ir.surfaces.push(Surface {
        id: SurfaceId("bad_sphere".into()),
        geometry: SurfaceGeometry::Sphere {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: None,
            ref_direction: None,
            radius: -1.0,
        },
        meta: EntityMeta::synthetic(),
    });
    let report = validate(&ir, Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn degenerate_plane_normal_is_flagged() {
    let mut ir = unit_cube();
    if let SurfaceGeometry::Plane { normal, .. } = &mut ir.surfaces[0].geometry {
        *normal = Vector3::new(0.0, 0.0, 0.0);
    }
    let report = validate(&ir, Vec::new());
    assert!(report.findings.iter().any(|f| f.check == Check::Bounds));
}

#[test]
fn new_topology_references_are_validated() {
    let mut ir = unit_cube();
    ir.shells[0].wire_edges.push(EdgeId("missing-wire".into()));
    ir.shells[0]
        .free_vertices
        .push(crate::ids::VertexId("missing-free".into()));
    ir.coedges[0].radial_next = Some(CoedgeId("missing-radial".into()));

    let report = validate(&ir, Vec::new());
    let messages = report
        .findings
        .iter()
        .map(|finding| finding.message.as_str())
        .collect::<Vec<_>>();
    assert!(messages.iter().any(|message| message.contains("wire edge")));
    assert!(messages
        .iter()
        .any(|message| message.contains("free vertex")));
    assert!(messages
        .iter()
        .any(|message| message.contains("coedge(radial_next)")));
}

#[test]
fn topology_tolerance_and_new_conics_are_bounds_checked() {
    let mut ir = unit_cube();
    ir.edges[0].tolerance = Some(-1.0);
    ir.curves.push(Curve {
        id: CurveId("bad-parabola".into()),
        geometry: CurveGeometry::Parabola {
            vertex: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            major_direction: Vector3::new(1.0, 0.0, 0.0),
            focal_distance: 0.0,
        },
        meta: EntityMeta::synthetic(),
    });
    ir.curves.push(Curve {
        id: CurveId("bad-hyperbola".into()),
        geometry: CurveGeometry::Hyperbola {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            major_direction: Vector3::new(1.0, 0.0, 0.0),
            major_radius: -1.0,
            minor_radius: 1.0,
        },
        meta: EntityMeta::synthetic(),
    });

    let report = validate(&ir, Vec::new());
    for entity in ["e0", "bad-parabola", "bad-hyperbola"] {
        assert!(report
            .findings
            .iter()
            .any(|finding| finding.check == Check::Bounds
                && finding.entity.as_deref() == Some(entity)));
    }
}

#[test]
fn schema_generation_produces_definitions() {
    let schema = crate::cadir_json_schema();
    // The schema must reference the entity types, not just be an empty object.
    let json = serde_json::to_string(&schema).unwrap();
    assert!(json.contains("Body"));
    assert!(json.contains("Coedge"));
    assert!(json.contains("SurfaceGeometry"));
    assert!(!schema.definitions.is_empty());
}
