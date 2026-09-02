// SPDX-License-Identifier: Apache-2.0

use super::*;

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
    assert!(decoded.report().geometry_transferred());
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
        .expect_err("the F3Z row is not a synthesis target");
    let cadmpeg_core::CodecError::UnsupportedTarget(refusal) = &error else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(refusal.requested(), Some("f3d:f3z-multi-document"));
    assert!(
        refusal
            .available()
            .iter()
            .any(|target| target.id.as_str() == "f3d:manifest-3-2-0-0"),
        "{:?}",
        refusal.available()
    );

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

    assert!(decoded.report().geometry_transferred());
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
        Err(cadmpeg_ir::DecodeFailure::Codec(
            cadmpeg_core::CodecError::Malformed(_)
        ))
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
