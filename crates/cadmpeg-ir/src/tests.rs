// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
//! Unit tests for the IR: the worked cube validates clean, JSON round-trips,
//! and each validation check actually fires when its invariant is broken.

use crate::annotations::{ExactnessNote, Provenance};
use crate::design::SketchRelation;
use crate::examples::unit_cube;
use crate::geometry::{Curve, CurveGeometry, SurfaceGeometry};
use crate::history::{AsmHistoryRecord, Configuration, FeatureHistory, FeatureInputLane};
use crate::ids::{CoedgeId, CurveId, EdgeId, UnknownId};
use crate::math::{Point3, Vector3};
use crate::native::{F3dNative, SldprtNative};
use crate::provenance::Exactness;
use crate::report::Check;
use crate::tessellation::TessellationChannel;
use crate::unknown::UnknownRecord;
use crate::validate::validate;
use crate::{diff, CadIr};
use serde::{de::DeserializeOwned, Serialize};
use std::fmt::Debug;

fn assert_base64_round_trip_and_rejection<T>(value: &T, field: &str)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let mut json = serde_json::to_value(value).unwrap();
    assert_eq!(json[field], "AQID");
    assert_eq!(serde_json::from_value::<T>(json.clone()).unwrap(), *value);
    json[field] = serde_json::Value::String("%%%".into());
    assert!(serde_json::from_value::<T>(json).is_err());
}

/// Replace the surface of the cube's first face with an unknown surface,
/// optionally linking a preserved record, and return the face id and its
/// surface id. Leaves every loop/coedge/edge of the face intact.
fn make_first_face_surface_unknown(ir: &mut crate::CadIr, record: Option<UnknownId>) -> String {
    let face = &ir.model.faces[0];
    let surface_id = face.surface.0.clone();
    for s in &mut ir.model.surfaces {
        if s.id.0 == surface_id {
            s.geometry = SurfaceGeometry::Unknown { record };
            break;
        }
    }
    surface_id
}

#[test]
fn face_on_unknown_surface_validates_clean() {
    let mut ir = unit_cube();
    // Preserve a raw record and point the unknown surface at it.
    let rec = UnknownId("synthetic:cube:unknown#0".into());
    ir.unknowns.push(UnknownRecord {
        id: rec.clone(),
        offset: 0,
        byte_len: 16,
        sha256: "0".repeat(64),
        data: None,
        links: Vec::new(),
    });
    make_first_face_surface_unknown(&mut ir, Some(rec));

    let report = validate(&ir, Vec::new());
    assert!(
        report.is_ok(),
        "a face on an unknown surface is legal, got: {:?}",
        report.findings
    );
    // The face and its topology stay in the graph.
    assert_eq!(ir.model.faces.len(), 6);
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
    let rec = UnknownId("synthetic:cube:unknown#0".into());
    ir.unknowns.push(UnknownRecord {
        id: rec.clone(),
        offset: 0,
        byte_len: 16,
        sha256: "0".repeat(64),
        data: None,
        links: Vec::new(),
    });
    make_first_face_surface_unknown(&mut ir, Some(rec));

    let json = ir.to_canonical_json().unwrap();
    let parsed = crate::CadIr::from_json(&json).unwrap();
    assert_eq!(parsed, ir, "round-trip must preserve the unknown surface");
}

#[test]
fn unit_cube_has_expected_census() {
    let ir = unit_cube();
    assert_eq!(ir.model.bodies.len(), 1);
    assert_eq!(ir.model.regions.len(), 1);
    assert_eq!(ir.model.shells.len(), 1);
    assert_eq!(ir.model.faces.len(), 6);
    assert_eq!(ir.model.loops.len(), 6);
    assert_eq!(ir.model.coedges.len(), 24);
    assert_eq!(ir.model.edges.len(), 12);
    assert_eq!(ir.model.vertices.len(), 8);
    assert_eq!(ir.model.points.len(), 8);
    assert_eq!(ir.model.surfaces.len(), 6);
    assert_eq!(ir.model.curves.len(), 12);
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

    assert_eq!(
        &diff_kinds[..CadIr::arena_names().len()],
        CadIr::arena_names()
    );
    for name in CadIr::arena_names() {
        assert!(
            report.entity_counts.contains_key(*name),
            "entity counts omitted registered arena {name}"
        );
    }
}

#[test]
fn native_records_use_own_ids_for_counts_diff_and_validation() {
    let left = unit_cube();
    let mut right = left.clone();
    right.native.f3d = Some(F3dNative {
        act_guids: vec![crate::design::ActGuid {
            id: "f3d:test:act-guid#0".into(),
            byte_offset: 0,
            guid_offset: 4,
            ordinal: 0,
            guid: "00000000-0000-0000-0000-000000000000".into(),
        }],
        ..F3dNative::default()
    });
    right.native.sldprt = Some(SldprtNative {
        feature_histories: vec![FeatureHistory {
            id: "sldprt:test:feature-history#0".into(),
            part_name: None,
            configurations: vec![Configuration {
                id: "sldprt:test:configuration#0".into(),
                name: "Default".into(),
                material: None,
                properties: std::collections::BTreeMap::new(),
            }],
            features: Vec::new(),
        }],
        ..SldprtNative::default()
    });

    let result = diff(&left, &right);
    assert_eq!(
        result
            .per_arena
            .iter()
            .find(|arena| arena.kind == "native.f3d.act_guids")
            .unwrap()
            .added,
        ["f3d:test:act-guid#0"]
    );
    assert_eq!(
        result
            .per_arena
            .iter()
            .find(|arena| arena.kind == "native.sldprt.configurations")
            .unwrap()
            .added,
        ["sldprt:test:configuration#0"]
    );
    let report = validate(&right, Vec::new());
    assert_eq!(report.entity_counts["native.f3d.act_guids"], 1);
    assert_eq!(report.entity_counts["native.sldprt.configurations"], 1);
    assert!(report.is_ok(), "{:?}", report.findings);

    right.native.sldprt.as_mut().unwrap().feature_histories[0].configurations[0].id =
        "f3d:test:act-guid#0".into();
    assert!(validate(&right, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "native record id is empty or duplicated"));
}

#[test]
fn every_cube_edge_has_two_opposite_sense_coedges() {
    let ir = unit_cube();
    for edge in &ir.model.edges {
        let coedges: Vec<_> = ir
            .model
            .coedges
            .iter()
            .filter(|c| c.edge == edge.id)
            .collect();
        assert_eq!(coedges.len(), 2, "edge {} should have 2 coedges", edge.id);
        assert_ne!(
            coedges[0].sense, coedges[1].sense,
            "edge {} coedges should have opposite sense",
            edge.id
        );
        // Partners point at each other.
        assert_eq!(coedges[0].radial_next, coedges[1].id);
        assert_eq!(coedges[1].radial_next, coedges[0].id);
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
    let body = ir.model.bodies[0].id.clone();
    ir.model.appearances.push(Appearance {
        id: AppearanceId("synthetic:test:appearance#prism-001".into()),
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
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "synthetic:test:appearance-binding#0".into(),
        target: AppearanceTarget::Body(body),
        appearance: AppearanceId("synthetic:test:appearance#prism-001".into()),
        source_entity_id: Some("0_1".into()),
        object_type: Some("Body".into()),
        channels: std::collections::BTreeMap::new(),
    });

    let json = ir.to_canonical_json().unwrap();
    let decoded = CadIr::from_json(&json).unwrap();
    assert_eq!(decoded.model.appearances, ir.model.appearances);
    assert_eq!(
        decoded.model.appearance_bindings,
        ir.model.appearance_bindings
    );
}

#[test]
fn dangling_reference_is_flagged() {
    let mut ir = unit_cube();
    // Point a coedge's edge at something that does not exist.
    ir.model.coedges[0].edge = EdgeId("does-not-exist".into());
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
    let foreign = ir.model.coedges[20].id.clone();
    ir.model.coedges[0].next = foreign;
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
    let partner_id: CoedgeId = ir.model.coedges[0].radial_next.clone();
    let other_edge = ir
        .model
        .coedges
        .iter()
        .find(|c| c.edge != ir.model.coedges[0].edge)
        .unwrap()
        .edge
        .clone();
    for c in &mut ir.model.coedges {
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
    ir.model.surfaces[0].geometry = SurfaceGeometry::Sphere {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: -1.0,
    };
    let report = validate(&ir, Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn degenerate_plane_normal_is_flagged() {
    let mut ir = unit_cube();
    if let SurfaceGeometry::Plane { normal, .. } = &mut ir.model.surfaces[0].geometry {
        *normal = Vector3::new(0.0, 0.0, 0.0);
    }
    let report = validate(&ir, Vec::new());
    assert!(report.findings.iter().any(|f| f.check == Check::Bounds));
}

#[test]
fn new_topology_references_are_validated() {
    let mut ir = unit_cube();
    ir.model.shells[0]
        .wire_edges
        .push(EdgeId("missing-wire".into()));
    ir.model.shells[0]
        .free_vertices
        .push(crate::ids::VertexId("missing-free".into()));
    ir.model.coedges[0].radial_next = CoedgeId("missing-radial".into());

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
    let edge_id = ir.model.edges[0].id.0.clone();
    ir.model.edges[0].tolerance = Some(-1.0);
    ir.model.curves.push(Curve {
        id: CurveId("synthetic:test:curve#bad-parabola".into()),
        geometry: CurveGeometry::Parabola {
            vertex: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            major_direction: Vector3::new(1.0, 0.0, 0.0),
            focal_distance: 0.0,
        },
    });
    ir.model.curves.push(Curve {
        id: CurveId("synthetic:test:curve#bad-hyperbola".into()),
        geometry: CurveGeometry::Hyperbola {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            major_direction: Vector3::new(1.0, 0.0, 0.0),
            major_radius: -1.0,
            minor_radius: 1.0,
        },
    });

    let report = validate(&ir, Vec::new());
    for entity in [
        edge_id.as_str(),
        "synthetic:test:curve#bad-parabola",
        "synthetic:test:curve#bad-hyperbola",
    ] {
        assert!(report
            .findings
            .iter()
            .any(
                |finding| (finding.check == Check::Bounds || finding.check == Check::Tolerances)
                    && finding.entity.as_deref() == Some(entity)
            ));
    }
}

#[test]
fn wrong_document_version_is_flagged() {
    let mut ir = unit_cube();
    ir.ir_version = "2".into();
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::Version));
}

#[test]
fn parser_rejects_unsupported_missing_and_non_string_versions() {
    let canonical = serde_json::to_value(unit_cube()).unwrap();
    for version in [
        Some(serde_json::Value::String("0".into())),
        None,
        Some(serde_json::Value::Number(1.into())),
    ] {
        let mut value = canonical.clone();
        let object = value.as_object_mut().unwrap();
        match version {
            Some(version) => {
                object.insert("ir_version".into(), version);
            }
            None => {
                object.remove("ir_version");
            }
        }
        let error = CadIr::from_json(&serde_json::to_string(&value).unwrap()).unwrap_err();
        assert!(!error.is_syntax());
        assert!(error.to_string().contains("unsupported ir_version"));
    }
}

#[test]
fn parser_distinguishes_malformed_json_from_version_rejection() {
    let error = CadIr::from_json("{\"ir_version\":\"1\"").unwrap_err();
    assert!(error.is_syntax() || error.is_eof());
    assert!(!error.to_string().contains("unsupported ir_version"));
}

#[test]
fn entity_ids_follow_canonical_grammar() {
    let mut ir = unit_cube();
    ir.model.points[0].id.0 = "synthetic:scope:point".into();
    let findings = validate(&ir, Vec::new()).findings;
    assert!(findings.iter().any(|finding| {
        finding.check == Check::Identity
            && finding.entity.as_deref() == Some("synthetic:scope:point")
            && finding.message.contains("<format>:<scope>:<kind>#<key>")
    }));
}

#[test]
fn ids_are_globally_unique_across_arenas() {
    let mut ir = unit_cube();
    ir.model.points[0].id.0 = ir.model.vertices[0].id.0.clone();
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::Identity));
}

#[test]
fn arena_ids_must_be_sorted() {
    let mut ir = unit_cube();
    ir.model.points.swap(0, 1);
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ArenaOrder));
}

#[test]
fn two_member_radial_ring_with_equal_senses_warns() {
    let mut ir = unit_cube();
    let other_id = ir.model.coedges[0].radial_next.clone();
    let sense = ir.model.coedges[0].sense;
    ir.model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.id == other_id)
        .unwrap()
        .sense = sense;
    assert!(validate(&ir, Vec::new()).findings.iter().any(|finding| {
        finding.check == Check::CoedgePairing
            && finding.severity == crate::report::Severity::Warning
    }));
}

#[test]
fn coedge_backed_edge_cannot_be_a_wire_edge() {
    let mut ir = unit_cube();
    ir.model.shells[0]
        .wire_edges
        .push(ir.model.coedges[0].edge.clone());
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::WireTopology));
}

#[test]
fn wire_and_free_topology_negative_cases_are_reported() {
    let mut ir = unit_cube();

    let mut unowned_edge = ir.model.edges[0].clone();
    unowned_edge.id.0 = "synthetic:test:edge#unowned".into();
    ir.model.edges.push(unowned_edge);

    let mut duplicate_edge = ir.model.edges[1].clone();
    duplicate_edge.id.0 = "synthetic:test:edge#duplicate".into();
    ir.model.shells[0]
        .wire_edges
        .extend([duplicate_edge.id.clone(), duplicate_edge.id.clone()]);
    ir.model.edges.push(duplicate_edge);

    let mut unowned_vertex = ir.model.vertices[0].clone();
    unowned_vertex.id.0 = "synthetic:test:vertex#unowned".into();
    ir.model.vertices.push(unowned_vertex);

    ir.model.shells[0]
        .free_vertices
        .push(ir.model.edges[0].start.clone());
    ir.model.bodies[0].kind = crate::topology::BodyKind::Wire;
    ir.finalize();

    let findings = validate(&ir, Vec::new()).findings;
    for message in [
        "wire edge must belong to exactly one shell",
        "free vertex must belong to exactly one shell",
        "free vertex is also referenced by an edge",
        "wire body contains faces",
    ] {
        assert!(
            findings.iter().any(|finding| {
                finding.check == Check::WireTopology && finding.message == message
            }),
            "missing `{message}` in {findings:?}"
        );
    }
}

#[test]
fn empty_shell_is_reported() {
    let mut ir = unit_cube();
    ir.model.shells[0].faces.clear();
    let findings = validate(&ir, Vec::new()).findings;
    assert!(findings.iter().any(|finding| {
        finding.check == Check::WireTopology && finding.message == "shell owns no topology"
    }));
}

#[test]
fn orphan_carrier_is_flagged() {
    let mut ir = unit_cube();
    let mut orphan = ir.model.curves[0].clone();
    orphan.id = CurveId("zz:orphan".into());
    ir.model.curves.push(orphan);
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::CarrierReachability));
}

#[test]
fn annotation_keys_streams_and_field_paths_are_checked() {
    let mut ir = unit_cube();
    ir.annotations.provenance.insert(
        "missing".into(),
        Provenance {
            stream: u32::MAX,
            offset: 0,
            tag: None,
        },
    );
    ir.annotations.exactness.insert(
        ir.model.edges[0].id.0.clone(),
        ExactnessNote {
            entity: Exactness::Derived,
            fields: std::collections::BTreeMap::from([(
                "not_a_serialized_field".into(),
                Exactness::Derived,
            )]),
        },
    );
    let findings = validate(&ir, Vec::new()).findings;
    assert!(findings.iter().any(|finding| {
        finding.check == Check::Annotations && finding.severity == crate::report::Severity::Error
    }));
    assert!(findings.iter().any(|finding| {
        finding.check == Check::Annotations && finding.severity == crate::report::Severity::Warning
    }));
}

#[test]
fn native_topology_link_must_resolve() {
    let mut ir = unit_cube();
    ir.native.f3d = Some(F3dNative {
        sketch_curve_links: vec![crate::design::SketchCurveLink {
            id: "native:link#0".into(),
            coedge: CoedgeId("missing".into()),
            sketch_curve_id: 0,
            signed_reference: None,
            role: 0,
            closure: 0,
        }],
        ..F3dNative::default()
    });
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::NativeLinks));
}

#[test]
fn periodic_curve_parameter_domain_is_checked() {
    let mut ir = unit_cube();
    let curve_id = ir.model.edges[0].curve.clone().unwrap();
    ir.model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry = CurveGeometry::Circle {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 1.0,
    };
    ir.model.edges[0].param_range = Some([0.0, 7.0]);
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));
}

#[test]
fn document_and_entity_tolerances_are_checked() {
    let mut ir = unit_cube();
    ir.tolerances.angular = f64::NAN;
    ir.model.faces[0].tolerance = Some(0.0);
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::Tolerances));
}

#[test]
fn preserved_payload_digest_is_checked() {
    let mut ir = unit_cube();
    ir.unknowns.push(UnknownRecord {
        id: UnknownId("zz:payload".into()),
        offset: 0,
        byte_len: 3,
        sha256: "0".repeat(64),
        data: Some(vec![1, 2, 3]),
        links: Vec::new(),
    });
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::PayloadIntegrity));
}

#[test]
fn byte_payloads_use_nonempty_base64_and_reject_invalid_text() {
    assert_base64_round_trip_and_rejection(
        &UnknownRecord {
            id: UnknownId("synthetic:test:unknown#0".into()),
            offset: 0,
            byte_len: 3,
            sha256: "00".repeat(32),
            data: Some(vec![1, 2, 3]),
            links: Vec::new(),
        },
        "data",
    );
    assert_base64_round_trip_and_rejection(
        &FeatureInputLane {
            id: "sldprt:test:feature-input-lane#0".into(),
            configuration: None,
            native_payload: vec![1, 2, 3],
            sketch_entities: Vec::new(),
        },
        "native_payload",
    );
    assert_base64_round_trip_and_rejection(
        &AsmHistoryRecord {
            id: "f3d:test:asm-history-record#0".into(),
            index: 0,
            name: "record".into(),
            raw_bytes: vec![1, 2, 3],
        },
        "raw_bytes",
    );
    assert_base64_round_trip_and_rejection(
        &SketchRelation {
            id: "f3d:test:sketch-relation#0".into(),
            record_index: 0,
            class_tag: "001".into(),
            byte_offset: 0,
            state_offset: 0,
            owner_reference: 0,
            auxiliary_references: Vec::new(),
            members: Vec::new(),
            state: 0,
            constraint_kinds: Vec::new(),
            unknown_constraint_bits: 0,
            return_members: Vec::new(),
            raw_bytes: vec![1, 2, 3],
        },
        "raw_bytes",
    );
    assert_base64_round_trip_and_rejection(
        &TessellationChannel {
            item_size: 3,
            kind: 0,
            flags: 0,
            count: 1,
            data: vec![1, 2, 3],
        },
        "data",
    );
}

#[test]
fn schema_generation_produces_definitions() {
    let schema = crate::cadir_json_schema();
    // The schema must reference the entity types, not just be an empty object.
    let json = serde_json::to_string(&schema).unwrap();
    assert!(json.contains("Body"));
    assert!(json.contains("Coedge"));
    assert!(json.contains("SurfaceGeometry"));
    let defs = schema
        .get("$defs")
        .and_then(serde_json::Value::as_object)
        .expect("schema has a $defs object");
    assert!(!defs.is_empty());
}
