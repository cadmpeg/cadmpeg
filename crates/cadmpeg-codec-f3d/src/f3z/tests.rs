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

use cadmpeg_ir::codec::EncodeInput;
use cadmpeg_ir::codec::TargetRequest;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions, Encoder};

use crate::test_support::*;
use crate::{F3dCodec, F3dLossCode};

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
    // A merged F3Z has no retained image of itself, so there is nothing to
    // preserve, and `f3d:f3z-multi-document` is not a row the generator can
    // synthesize. `Inherit` therefore refuses by name rather than quietly
    // handing back a single-document archive under the F3Z document's identity.
    let error = F3dCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .err()
        .expect("the F3Z row is not a synthesis target");
    let cadmpeg_core::CodecError::UnsupportedTarget {
        requested,
        available,
        ..
    } = &error
    else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(
        requested.as_ref().map(cadmpeg_core::TargetToken::as_str),
        Some("f3d:f3z-multi-document")
    );
    assert!(available.contains("f3d:manifest-3-2-0-0"), "{available}");

    // Naming the row is the escape, and it still regenerates the merged model
    // as a single-document archive — now with the report saying so.
    let mut regenerated = Vec::new();
    let report = F3dCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Explicit("f3d:manifest-3-2-0-0"),
        )
        .and_then(|plan| plan.write_to(&mut regenerated))
        .expect("merged F3Z regenerates at the named row");
    assert!(!regenerated.is_empty());
    assert_eq!(
        report
            .target()
            .map(cadmpeg_core::dialect::DialectId::as_str),
        Some("f3d:manifest-3-2-0-0")
    );
    assert!(report
        .notes
        .iter()
        .any(|note| note == "source container regenerated from IR"));
}

#[test]
fn f3z_drawing_root_decodes_its_unambiguous_derived_model() {
    let model = f3d_with_smbh(&synthetic_geometry_smbh());
    let drawing = b"synthetic drawing payload";
    let description = br#"{"designDescription":{"designGraphs":[{"rootIds":[10],"designObjects":[{"id":10,"relativePath":"drawing.f2d","contentType":"f2d","references":[{"type":"DERIVED","ids":[11]}]},{"id":11,"relativePath":"model.f3d","contentType":"f3d","references":[]}]}]}}"#;
    let archive = f3z_archive_with_design_description(
        "drawing.f2d",
        &[("drawing.f2d", drawing), ("model.f3d", model.as_slice())],
        description,
    );

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();

    assert!(decoded.report().geometry_transferred);
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::DrawingDocumentOmitted.kind()));
}

#[test]
fn f3z_drawing_root_rejects_ambiguous_derived_models() {
    let model = f3d_with_smbh(&synthetic_geometry_smbh());
    let description = br#"{"designDescription":{"designGraphs":[{"rootIds":[10],"designObjects":[{"id":10,"relativePath":"drawing.f2d","contentType":"f2d","references":[{"type":"DERIVED","ids":[11,12]}]},{"id":11,"relativePath":"first.f3d","contentType":"f3d","references":[]},{"id":12,"relativePath":"second.f3d","contentType":"f3d","references":[]}]}]}}"#;
    let archive = f3z_archive_with_design_description(
        "drawing.f2d",
        &[
            ("drawing.f2d", b"synthetic drawing payload"),
            ("first.f3d", model.as_slice()),
            ("second.f3d", model.as_slice()),
        ],
        description,
    );

    let result = F3dCodec.decode(&mut Cursor::new(archive), &DecodeOptions::default());

    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
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
        .decode(
            &mut Cursor::new(archive.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(decoded
        .source_fidelity()
        .retained_record(crate::ids::FILE_SOURCE_IMAGE_ID)
        .is_some());
    let plan = F3dCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .expect("unmerged F3Z archive remains replayable");
    let reported = plan
        .report()
        .target()
        .expect("an F3D export names its target")
        .clone();
    let mut replayed = Vec::new();
    plan.write_to(&mut replayed).unwrap();
    assert_eq!(replayed, archive);

    let redecode = F3dCodec
        .decode(&mut Cursor::new(replayed), &DecodeOptions::default())
        .unwrap();
    let primary = redecode
        .report()
        .dialects()
        .expect("re-decoded F3Z archive has a primary layer")
        .primary();
    assert_eq!(primary.dialect(), &reported);
}

#[test]
fn f3z_container_only_stamps_the_outer_document_digest() {
    let root = f3d_with_smbh(&synthetic_geometry_smbh());
    let archive = f3z_archive("root.f3d", &[("root.f3d", root.as_slice())]);
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(archive),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .unwrap();

    let source = decoded.ir().source.as_ref().unwrap();
    assert_eq!(
        source
            .attributes
            .get(cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE),
        Some(&crate::decode::document_local_sha256(decoded.ir()))
    );
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

/// The outer archive's row survives the inner member's decode.
///
/// [`crate::decode::decode`] runs on the root `.f3d` member and classifies that
/// member. The file the codec was handed is the `.f3z`, so both the report and
/// `SourceMeta` must name the F3Z row, at inspect and at decode.
#[test]
fn an_f3z_archive_reports_the_multi_document_row_at_inspect_and_decode() {
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("comp.f3d", XREF_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("comp.f3d", component.as_slice()),
        ],
    );

    let summary = F3dCodec
        .inspect(
            &mut Cursor::new(archive.clone()),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    let inspected = summary
        .dialects()
        .expect("inspect must report a primary F3D layer")
        .primary()
        .clone();
    let inspected_dialects = summary.dialects();
    assert_eq!(inspected.format(), "f3d");
    assert_eq!(inspected.dialect().as_str(), "f3d:f3z-multi-document");
    assert_eq!(
        inspected.declared()["root_document_members"],
        "comp.f3d,root.f3d",
        "each root-level member is recorded as the archive spells it, sorted by path"
    );
    assert_eq!(
        inspected.admission(),
        cadmpeg_core::dialect::Admission::Admitted
    );

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        decoded
            .report()
            .dialects()
            .expect("decode reports F3D layers")
            .primary(),
        inspected_dialects
            .as_ref()
            .expect("inspection reports F3D layers")
            .primary()
    );
    let extra_keys = |layers: &cadmpeg_core::dialect::DialectLayers| {
        layers
            .iter()
            .skip(1)
            .map(|matched| {
                (
                    matched.format().to_owned(),
                    matched.instance().map(str::to_owned),
                )
            })
            .collect::<std::collections::BTreeSet<_>>()
    };
    let inspected_extra_keys = extra_keys(inspected_dialects.as_ref().unwrap());
    let decoded_extra_keys = extra_keys(
        decoded
            .report()
            .dialects()
            .expect("decode reports F3D layers"),
    );
    assert_eq!(decoded_extra_keys, inspected_extra_keys);
    assert!(decoded_extra_keys.contains(&("f3d".to_owned(), Some("root.f3d".to_owned()))));
    assert!(decoded_extra_keys.contains(&("f3d".to_owned(), Some("comp.f3d".to_owned()))));
    assert!(decoded
        .report()
        .dialects()
        .expect("the report is classified")
        .iter()
        .skip(1)
        .all(|matched| matched
            .declared()
            .contains_key(crate::dialect::DECLARED_ARCHIVE_MEMBER)));
    assert!(decoded
        .report()
        .dialects()
        .expect("the report is classified")
        .iter()
        .skip(1)
        .any(|matched| matched.format() == "f3d" && matched.instance() == Some("root.f3d")));
    let source = decoded.ir().source.as_ref().unwrap();
    assert_eq!(source.dialect.as_ref(), Some(&inspected));
    let primary = decoded.report().dialects().unwrap().primary();
    assert_eq!(source.dialect.as_ref(), Some(primary));
}

fn unverified_acis_text_member() -> Vec<u8> {
    b"23200 0 1 0 \n\
      16 Autodesk Neutron 21 ASM 232.4.0.65535 OSX 9 Synthetic \n\
      1 9.999999999999999547e-07 1.000000000000000036e-10 \n\
      body $-1 -1 $-1 $-1 $-1 $-1 #\n\
      End-of-ACIS-data \n"
        .to_vec()
}

#[test]
fn f3z_decode_retains_the_root_kernel_row_and_loss() {
    let stream = unverified_acis_text_member();
    let root = f3d_with_text_brep_stream(
        &["FusionAssetName[Active]/Breps.BlobParts/Body1.sat"],
        &stream,
    );
    let archive = f3z_archive("root.f3d", &[("root.f3d", root.as_slice())]);

    let inspected = F3dCodec
        .inspect(
            &mut Cursor::new(archive.clone()),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    let inspected_extras = inspected
        .dialects()
        .unwrap()
        .iter()
        .skip(1)
        .collect::<Vec<_>>();
    let decoded_extras = decoded
        .report()
        .dialects()
        .unwrap()
        .iter()
        .skip(1)
        .collect::<Vec<_>>();
    assert_eq!(decoded_extras, inspected_extras);
    assert!(decoded
        .report()
        .dialects()
        .into_iter()
        .flat_map(cadmpeg_core::dialect::DialectLayers::iter)
        .any(|matched| {
            matched.format() == crate::dialect::FORMAT
                && matched.dialect().as_str() == "f3d:f3z-multi-document"
        }));
    assert!(
        decoded
            .report()
            .dialects()
            .into_iter()
            .flat_map(cadmpeg_core::dialect::DialectLayers::iter)
            .any(|matched| {
                matched.format() == "acis" && matched.dialect().as_str() == "acis:text-acis"
            }),
        "{:?}",
        decoded.report().dialects()
    );
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::KernelDialectUnverified.kind()));
}

#[test]
fn f3z_decode_retains_member_identity_and_unverified_loss() {
    let root = f3d_with_smbh_and_manifest_version(&synthetic_smbh(), "9-9-9-9");
    let member = F3dCodec
        .decode(&mut Cursor::new(root.clone()), &DecodeOptions::default())
        .unwrap();
    assert!(member
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::SourceDialectUnverified.kind()));
    let archive = f3z_archive("root.f3d", &[("root.f3d", root.as_slice())]);

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::SourceDialectUnverified.kind()));
    let member = decoded
        .report()
        .dialects()
        .expect("the F3Z report is classified")
        .iter()
        .find(|matched| matched.format() == crate::dialect::FORMAT && matched.instance().is_some())
        .expect("the member F3D identity is an extra layer");
    assert_eq!(member.instance(), Some("root.f3d"));
    assert_eq!(member.dialect().as_str(), "f3d:unknown");
}

#[test]
fn f3z_xref_kernel_row_uses_member_path_and_actual_xref_label() {
    let stream = unverified_acis_text_member();
    let component = f3d_with_text_brep_stream(
        &["FusionAssetName[Active]/Breps.BlobParts/Body1.sat"],
        &stream,
    );
    let bare = F3dCodec
        .decode(
            &mut Cursor::new(component.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let bare_loss = bare
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::KernelDialectUnverified.kind())
        .expect("the bare member charges its kernel loss")
        .clone();
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
    let kernel_layers = decoded
        .report()
        .dialects()
        .expect("the report is classified")
        .iter()
        .filter(|matched| matched.format() == cadmpeg_asm::dialect::FORMAT)
        .collect::<Vec<_>>();
    assert_eq!(kernel_layers.len(), 1);
    assert_eq!(kernel_layers[0].instance(), Some("comp.f3d"));
    assert_eq!(
        kernel_layers[0].declared()[crate::dialect::DECLARED_ARCHIVE_MEMBER],
        "comp.f3d"
    );
    assert_eq!(
        kernel_layers[0].declared()["carrier"],
        "FusionAssetName[Active]/Breps.BlobParts/Body1.sat"
    );
    let loss = decoded
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::KernelDialectUnverified.kind())
        .expect("component kernel loss travels with its row");
    assert_eq!(loss.code, bare_loss.code);
    assert_eq!(loss.severity, bare_loss.severity);
    assert_eq!(
        loss.message,
        format!("xref component0 (member comp.f3d): {}", bare_loss.message)
    );
}

#[test]
fn duplicate_member_layer_identity_is_a_recorded_loss() {
    let mut target =
        cadmpeg_core::dialect::DialectLayers::of(crate::dialect::F3dDialect::classify_f3z(&[
            "part.f3d",
        ]));
    let member = cadmpeg_core::dialect::DialectLayers::of(
        crate::dialect::F3dDialect::classify_document("3-2-0-0"),
    );

    assert!(super::merge_member_layers(&mut target, &member, "part.f3d").is_empty());
    let losses = super::merge_member_layers(&mut target, &member, "part.f3d");

    assert_eq!(target.iter().count(), 2);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code, F3dLossCode::DialectLayerCollision.kind());
    assert!(losses[0].message.contains("f3d"));
    assert!(losses[0].message.contains("part.f3d"));
}

/// A single-document archive names its own row, not the F3Z one.
#[test]
fn a_document_archive_reports_the_manifest_row_at_inspect_and_decode() {
    let document = f3d_with_smbh(&synthetic_geometry_smbh());

    let summary = F3dCodec
        .inspect(
            &mut Cursor::new(document.clone()),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    let inspected = summary
        .dialects()
        .expect("inspect must report exactly one primary F3D layer")
        .primary()
        .clone();
    let inspected_dialects = summary.dialects();
    assert_eq!(inspected.format(), "f3d");
    assert_eq!(inspected.dialect().as_str(), "f3d:manifest-3-2-0-0");
    assert_eq!(
        inspected.declared()["top_level_manifest_version"],
        "3-2-0-0"
    );
    assert_eq!(
        inspected.admission(),
        cadmpeg_core::dialect::Admission::Admitted
    );

    let decoded = F3dCodec
        .decode(&mut Cursor::new(document), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.report().dialects(), inspected_dialects);
    let source = decoded.ir().source.as_ref().unwrap();
    assert_eq!(source.dialect.as_ref(), Some(&inspected));
}
