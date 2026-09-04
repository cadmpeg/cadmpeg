// SPDX-License-Identifier: Apache-2.0
//! Crate-wide IR tests with no single production owner.

#![allow(clippy::unwrap_used)]
#![allow(clippy::redundant_closure_for_method_calls)]
#![allow(clippy::ignored_unit_patterns)]

use proptest::prelude::*;
use proptest::string::string_regex;

use crate::document::CadIr;
use crate::draft::{DraftError, ModelDraft};
use crate::ids::{BodyId, PointId, RegionId, ShellId, VertexId};
use crate::math::Point3;
use crate::provenance::SourceObjectAssociation;
use crate::report::{Check, Severity, ValidationReport};
use crate::topology::{Body, BodyKind, Point, Region, Shell, Vertex};
use crate::validate::{entity_census, validate_neutral};

const SEG: &str = "[a-z][a-z0-9_-]{0,7}";

fn config() -> ProptestConfig {
    ProptestConfig::with_cases(64)
}

fn id_strategy() -> impl Strategy<Value = String> {
    (
        string_regex(SEG).unwrap(),
        string_regex(SEG).unwrap(),
        string_regex(SEG).unwrap(),
        string_regex(SEG).unwrap(),
    )
        .prop_map(|(a, b, c, k)| format!("{a}:{b}:{c}#{k}"))
}

fn free_carrier(object_id: &str) -> SourceObjectAssociation {
    SourceObjectAssociation {
        format: crate::CodecFormat::Step,
        object_id: object_id.into(),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    }
}

fn point(id: &str) -> Point {
    Point {
        id: PointId(id.to_owned()),
        position: Point3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        // Free points are reachable only via source association (or a vertex).
        source_object: Some(free_carrier(id)),
    }
}

fn ir_strategy() -> impl Strategy<Value = CadIr> {
    proptest::collection::btree_set(id_strategy(), 1..=8).prop_map(|ids| {
        let mut ir = CadIr::empty();
        for id in ids {
            ir.model.points.push(point(&id));
        }
        ir.finalize();
        ir
    })
}

fn has_error(report: &ValidationReport, check: Check) -> bool {
    report
        .findings
        .iter()
        .any(|finding| finding.check == check && finding.severity == Severity::Error)
}

fn strip_one_namespace_component(id: &str) -> String {
    let (namespace, key) = id.split_once('#').expect("generated id has key");
    let mut parts = namespace.split(':');
    let a = parts.next().expect("first component");
    let b = parts.next().expect("second component");
    format!("{a}:{b}#{key}")
}

/// Wire body that owns one free vertex so `WireTopology` accepts the vertex.
fn insert_free_vertex_shell(
    draft: &mut ModelDraft,
    point_id: &str,
    vertex_id: &str,
    body_id: &str,
    region_id: &str,
    shell_id: &str,
) {
    draft.insert(point(point_id)).unwrap();
    draft
        .insert(Vertex {
            id: VertexId(vertex_id.into()),
            point: PointId(point_id.into()),
            tolerance: None,
        })
        .unwrap();
    draft
        .insert(Body {
            id: BodyId(body_id.into()),
            kind: BodyKind::Wire,
            regions: vec![RegionId(region_id.into())],
            transform: None,
            name: None,
            color: None,
            visible: None,
        })
        .unwrap();
    draft
        .insert(Region {
            id: RegionId(region_id.into()),
            body: BodyId(body_id.into()),
            shells: vec![ShellId(shell_id.into())],
        })
        .unwrap();
    draft
        .insert(Shell {
            id: ShellId(shell_id.into()),
            region: RegionId(region_id.into()),
            faces: Vec::new(),
            wire_edges: Vec::new(),
            free_vertices: vec![VertexId(vertex_id.into())],
        })
        .unwrap();
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn generated_documents_validate_clean(ir in ir_strategy()) {
        let report = validate_neutral(&ir, Vec::new());
        prop_assert_eq!(
            report.error_count(),
            0,
            "findings: {:?}",
            report.findings
        );
    }

    #[test]
    fn census_matches_arena_lengths(ir in ir_strategy()) {
        let census = entity_census(&ir);
        prop_assert_eq!(census["points"], ir.model.points.len());
        let report = validate_neutral(&ir, Vec::new());
        prop_assert_eq!(&census, &report.entity_counts);
    }

    #[test]
    fn single_invariant_breaks_are_caught(ir in ir_strategy()) {
        {
            let mut broken = ir.clone();
            broken.model.points[0].id.0 =
                strip_one_namespace_component(&broken.model.points[0].id.0);
            let report = validate_neutral(&broken, Vec::new());
            prop_assert!(
                has_error(&report, Check::Identity),
                "findings: {:?}",
                report.findings
            );
        }

        {
            let mut broken = ir.clone();
            broken.model.points.push(broken.model.points[0].clone());
            let report = validate_neutral(&broken, Vec::new());
            prop_assert!(
                has_error(&report, Check::Identity),
                "findings: {:?}",
                report.findings
            );
        }

        if ir.model.points.len() >= 2 {
            let mut broken = ir.clone();
            broken.model.points.swap(0, 1);
            let report = validate_neutral(&broken, Vec::new());
            prop_assert!(
                has_error(&report, Check::ArenaOrder),
                "findings: {:?}",
                report.findings
            );
        }

        {
            let mut broken = ir.clone();
            broken.model.vertices.push(Vertex {
                id: "prop:test:vertex#dangling".into(),
                point: PointId("x:y:z#missing".into()),
                tolerance: None,
            });
            broken.finalize();
            let report = validate_neutral(&broken, Vec::new());
            prop_assert!(
                has_error(&report, Check::ReferentialIntegrity),
                "findings: {:?}",
                report.findings
            );
        }
    }

    #[test]
    fn validation_is_deterministic(ir in ir_strategy()) {
        let a = validate_neutral(&ir, Vec::new());
        let b = validate_neutral(&ir, Vec::new());
        prop_assert_eq!(a.findings, b.findings);
    }

    #[test]
    fn draft_commit_rejects_dangling_references(
        (point_id, vertex_id, body_id, region_id, shell_id) in (
            id_strategy(),
            id_strategy(),
            id_strategy(),
            id_strategy(),
            id_strategy(),
        )
            .prop_filter("distinct ids", |(p, v, b, r, s)| {
                let ids = [p.as_str(), v.as_str(), b.as_str(), r.as_str(), s.as_str()];
                (0..ids.len()).all(|i| (i + 1..ids.len()).all(|j| ids[i] != ids[j]))
            })
    ) {
        let missing = "x:y:z#missing";
        let mut dangling = ModelDraft::new();
        dangling
            .insert(Vertex {
                id: vertex_id.clone().into(),
                point: missing.into(),
                tolerance: None,
            })
            .unwrap();
        let mut base = CadIr::empty();
        let dangling_result = dangling.commit_model(&mut base);
        let is_unresolved = matches!(
            dangling_result,
            Err(DraftError::UnresolvedReference { .. })
        );
        prop_assert!(
            is_unresolved,
            "expected UnresolvedReference, got {:?}",
            dangling_result
        );

        let mut draft = ModelDraft::new();
        insert_free_vertex_shell(
            &mut draft,
            &point_id,
            &vertex_id,
            &body_id,
            &region_id,
            &shell_id,
        );
        let mut base = CadIr::empty();
        draft.commit_model(&mut base).unwrap();
        base.finalize();
        let report = validate_neutral(&base, Vec::new());
        prop_assert_eq!(
            report.error_count(),
            0,
            "findings: {:?}",
            report.findings
        );
    }
}
