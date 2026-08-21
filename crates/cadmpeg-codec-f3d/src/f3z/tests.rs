// SPDX-License-Identifier: Apache-2.0
//! F3Z merge and archive tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions, Encoder};

use crate::test_support::*;
use crate::F3dCodec;

use crate::records::DesignSketchPlacement;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};
use cadmpeg_ir::ids::{BodyId, RegionId};
use cadmpeg_ir::topology::{Body, BodyKind, Region};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::{Native, NativeRecord};

fn feature(id: &str, ordinal: u64) -> Feature {
    Feature {
        id: FeatureId(id.into()),
        ordinal,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: "test".into(),
            parameters: std::collections::BTreeMap::new(),
            properties: std::collections::BTreeMap::new(),
        },
        native_ref: None,
    }
}

#[test]
fn component_feature_history_follows_the_parent_without_losing_relative_order() {
    let parent = Model {
        features: vec![
            feature("f3d:feature#parent-0", 4),
            feature("f3d:feature#parent-1", 8),
        ],
        ..Model::default()
    };
    let mut component = Model {
        features: vec![
            feature("f3d:feature#component-0", 10),
            feature("f3d:feature#component-1", 12),
        ],
        ..Model::default()
    };

    super::append_feature_history(&parent, &mut component).unwrap();

    assert_eq!(
        component
            .features
            .iter()
            .map(|feature| feature.ordinal)
            .collect::<Vec<_>>(),
        vec![9, 11]
    );
}

#[test]
fn component_feature_history_refuses_an_exhausted_ordinal_domain() {
    let parent = Model {
        features: vec![feature("f3d:feature#parent", u64::MAX)],
        ..Model::default()
    };
    let mut component = Model {
        features: vec![feature("f3d:feature#component", 0)],
        ..Model::default()
    };

    let error = super::append_feature_history(&parent, &mut component).unwrap_err();

    assert!(error
        .to_string()
        .contains("merged F3Z feature ordinal exceeds u64::MAX"));
}

/// The rescoping round-trip carries an entity through an untyped value tree.
/// A coordinate is an `f64`, and a decoded one is not guaranteed finite, so
/// the tree must hold the value itself rather than a decimal rendering of it.
#[test]
fn rescoping_a_model_entity_preserves_a_non_finite_coordinate() {
    use cadmpeg_ir::document::EntityRewrite;
    use cadmpeg_ir::ids::PointId;
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::Point;

    let point = Point {
        id: PointId("f3d:model:point#1".into()),
        position: Point3 {
            x: f64::NAN,
            y: f64::INFINITY,
            z: f64::NEG_INFINITY,
        },
        source_object: None,
    };

    let rescoped = super::OccurrenceScope {
        occurrence: "role/occurrence-0",
    }
    .rewrite(point)
    .expect("a model entity rescopes through the value tree");

    assert_eq!(rescoped.id.0, "f3d:xref/role/occurrence-0/model:point#1");
    assert!(rescoped.position.x.is_nan());
    assert_eq!(rescoped.position.y, f64::INFINITY);
    assert_eq!(rescoped.position.z, f64::NEG_INFINITY);
}

#[test]
fn occurrence_transform_composes_outside_existing_body_transform() {
    let outer = Transform {
        rows: [
            [0.0, -1.0, 0.0, 20.0],
            [1.0, 0.0, 0.0, 30.0],
            [0.0, 0.0, 1.0, 40.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    let inner = Transform {
        rows: [
            [1.0, 0.0, 0.0, 5.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };

    assert_eq!(
        super::compose_transforms(outer, inner).rows,
        [
            [0.0, -1.0, 0.0, 20.0],
            [1.0, 0.0, 0.0, 35.0],
            [0.0, 0.0, 1.0, 40.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    );
}

#[test]
fn repeated_occurrence_merge_remaps_typed_graphs_disjointly() {
    let mut merged = Model::default();
    let component = Model {
        bodies: vec![Body {
            id: BodyId("f3d:brep:entity#1".into()),
            kind: BodyKind::Solid,
            regions: vec![RegionId("f3d:brep:entity#2".into())],
            transform: None,
            name: None,
            color: None,
            visible: None,
        }],
        regions: vec![Region {
            id: RegionId("f3d:brep:entity#2".into()),
            body: BodyId("f3d:brep:entity#1".into()),
            shells: Vec::new(),
        }],
        ..Model::default()
    };
    for ordinal in 0..2 {
        let occurrence = format!("role/occurrence-{ordinal}");
        let mut scope = super::OccurrenceScope {
            occurrence: &occurrence,
        };
        merged
            .extend_rewritten(component.clone(), &mut scope)
            .expect("merge component arenas");
    }

    for ordinal in 0..2 {
        let prefix = format!("f3d:xref/role/occurrence-{ordinal}/brep:entity#");
        assert_eq!(merged.bodies[ordinal].id.0, format!("{prefix}1"));
        assert_eq!(merged.bodies[ordinal].regions[0].0, format!("{prefix}2"));
        assert_eq!(merged.regions[ordinal].id.0, format!("{prefix}2"));
        assert_eq!(merged.regions[ordinal].body.0, format!("{prefix}1"));
    }
}

#[test]
fn occurrence_merge_remaps_and_retains_native_records() {
    let placement = DesignSketchPlacement {
        id: "f3d:Design/BulkStream.dat:design-sketch-placement#42".into(),
        scope_record_index: None,
        entity_id: "Sketch_1".into(),
        entity_suffix: 1,
        visibility: None,
        byte_offset: 42,
        class_tag: "001".into(),
        record_index: 7,
        frame_length: 34,
        transform: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        transform_offset: None,
        paired_class_tag: "002".into(),
        paired_byte_offset: 76,
        member_run_head: true,
    };
    let mut component = Native::default();
    component
        .namespace_mut("f3d")
        .set_arena("design_sketch_placements", &[placement])
        .expect("store component native");
    let mut root = Native::default();
    super::extend_native(&mut root, component, "role/occurrence-0");

    let merged: Vec<DesignSketchPlacement> = root
        .namespace("f3d")
        .expect("merged f3d namespace")
        .arena_as("design_sketch_placements")
        .expect("read merged arena");
    assert_eq!(
        merged[0].id,
        "f3d:xref/role/occurrence-0/Design/BulkStream.dat:design-sketch-placement#42"
    );
}

#[test]
fn occurrence_merge_remaps_native_record_map_keys_and_nested_payloads() {
    let record = NativeRecord::new(
        "f3d:Design/Configurations.json:design-configuration#1",
        serde_json::json!({
            "channels": {
                "f3d:brep:entity#2": "kept",
                "plain": "f3d:brep:entity#3",
            },
            "payload": [{"link": "f3d:brep:entity#4"}, "not-an-id"],
        })
        .as_object()
        .expect("object payload")
        .clone(),
    );

    let rescoped = super::rescope_record(&record, "role/occurrence-0");

    assert_eq!(
        rescoped.id(),
        "f3d:xref/role/occurrence-0/Design/Configurations.json:design-configuration#1"
    );
    assert_eq!(
        serde_json::Value::Object(rescoped.fields()),
        serde_json::json!({
            "channels": {
                "f3d:xref/role/occurrence-0/brep:entity#2": "kept",
                "plain": "f3d:xref/role/occurrence-0/brep:entity#3",
            },
            "payload": [
                {"link": "f3d:xref/role/occurrence-0/brep:entity#4"},
                "not-an-id",
            ],
        })
    );
}

#[test]
fn f3z_archive_merges_identity_occurrences() {
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let component_alone = F3dCodec
        .decode(
            &mut Cursor::new(component.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("comp.f3d", XREF_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("comp.f3d", component.as_slice()),
        ],
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert!(decoded.report().geometry_transferred);
    assert!(
        decoded
            .report()
            .losses
            .iter()
            .all(|loss| loss.severity < cadmpeg_ir::report::Severity::Error),
        "{:?}",
        decoded.report().losses
    );
    assert!(decoded
        .report()
        .notes
        .iter()
        .any(|note| note.contains("merged 1 external occurrence")));
    assert_eq!(
        decoded.ir().model.bodies.len(),
        component_alone.ir().model.bodies.len()
    );
    assert_eq!(
        decoded.ir().model.faces.len(),
        component_alone.ir().model.faces.len()
    );
    assert_eq!(
        decoded.ir().model.points.len(),
        component_alone.ir().model.points.len()
    );
    let prefix = format!("f3d:xref/{XREF_ROLE}/");
    let body = &decoded.ir().model.bodies[0];
    assert!(body.id.0.starts_with(&prefix), "{}", body.id.0);
    for shell_owner in &decoded.ir().model.shells {
        assert!(
            shell_owner.id.0.starts_with(&prefix),
            "occurrence graph must stay internally consistent: {}",
            shell_owner.id.0
        );
    }
    assert!(decoded
        .source_fidelity()
        .retained_record(crate::ids::FILE_SOURCE_IMAGE_ID)
        .is_none());
    assert_eq!(
        decoded.source_fidelity().annotations.provenance.len(),
        component_alone
            .source_fidelity()
            .annotations
            .provenance
            .len()
    );
    assert!(decoded
        .source_fidelity()
        .annotations
        .provenance
        .keys()
        .all(|id| id.starts_with(&prefix)));
    let mut regenerated = Vec::new();
    let report = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: decoded.ir(),
            fidelity: Some(decoded.source_fidelity()),
        })
        .and_then(|plan| plan.write_to(&mut regenerated))
        .expect("merged F3Z regenerates instead of replaying a member");
    assert!(!regenerated.is_empty());
    assert!(report
        .notes
        .iter()
        .any(|note| note == "source container regenerated from IR"));
}

#[test]
fn f3z_archive_merges_occurrence_scoped_unknown_carriers() {
    let component = f3d_with_smbh(&synthetic_mixed_smbh());
    let component_alone = F3dCodec
        .decode(
            &mut Cursor::new(component.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let component_unknowns = component_alone.ir().native_unknowns("f3d").unwrap();
    assert!(!component_unknowns.is_empty());

    let root = f3d_without_brep("assembly-design", "root.f3d", &[("comp.f3d", XREF_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("comp.f3d", component.as_slice()),
        ],
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();

    let prefix = format!("f3d:xref/{XREF_ROLE}/occurrence-0/");
    let merged_unknowns = decoded.ir().native_unknowns("f3d").unwrap();
    assert_eq!(merged_unknowns.len(), component_unknowns.len());
    assert!(merged_unknowns
        .iter()
        .all(|record| record.id.0.starts_with(&prefix)));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(
        !validation
            .findings
            .iter()
            .any(|finding| { finding.check == cadmpeg_ir::report::Check::ReferentialIntegrity }),
        "{validation:#?}"
    );
}

#[test]
fn f3z_archive_without_merged_components_preserves_root_replay() {
    let root = f3d_with_smbh(&synthetic_geometry_smbh());
    let archive = f3z_archive("root.f3d", &[("root.f3d", root.as_slice())]);
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();

    assert!(decoded
        .source_fidelity()
        .retained_record(crate::ids::FILE_SOURCE_IMAGE_ID)
        .is_some());
    let mut replayed = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: decoded.ir(),
            fidelity: Some(decoded.source_fidelity()),
        })
        .and_then(|plan| plan.write_to(&mut replayed))
        .expect("unmerged F3Z root member remains replayable");
    assert_eq!(replayed, root);
}

#[test]
fn f3z_archive_recursively_merges_nested_occurrences() {
    const CHILD_ROLE: &str = "11112222-3333-4444-5555-666677778888";
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let component_alone = F3dCodec
        .decode(
            &mut Cursor::new(component.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let middle = f3d_without_brep(
        "assembly-design",
        "middle.f3d",
        &[("component.f3d", CHILD_ROLE)],
    );
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("middle.f3d", XREF_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("middle.f3d", middle.as_slice()),
            ("component.f3d", component.as_slice()),
        ],
    );

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();

    assert_eq!(
        decoded.ir().model.bodies.len(),
        component_alone.ir().model.bodies.len()
    );
    assert!(decoded
        .report()
        .notes
        .iter()
        .any(|note| note.contains("merged 2 external occurrence")));
    let body_id = &decoded.ir().model.bodies[0].id.0;
    assert!(body_id.contains(&format!(
        "xref/{XREF_ROLE}/occurrence-0/xref/{CHILD_ROLE}/occurrence-0/"
    )));
}

#[test]
fn f3z_archive_reports_reference_cycles_without_recursing() {
    const CHILD_ROLE: &str = "11112222-3333-4444-5555-666677778888";
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("middle.f3d", XREF_ROLE)]);
    let middle = f3d_without_brep("assembly-design", "middle.f3d", &[("root.f3d", CHILD_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("middle.f3d", middle.as_slice()),
        ],
    );

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();

    assert!(decoded.report().losses.iter().any(|loss| {
        loss.severity == cadmpeg_ir::report::Severity::Error
            && loss.message.contains("reference cycle through root.f3d")
    }));
}

#[test]
fn f3z_prefix_detects_as_f3d() {
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("comp.f3d", XREF_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("comp.f3d", component.as_slice()),
        ],
    );
    assert_eq!(
        F3dCodec.detect(&archive[..512.min(archive.len())]),
        Confidence::High
    );
}
