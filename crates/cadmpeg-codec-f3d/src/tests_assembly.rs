// SPDX-License-Identifier: Apache-2.0
//! Assembly-domain synthetic tests and fixtures.

use super::*;
use cadmpeg_ir::codec::CodecBackend;

/// A `RedirectionsStream.dat` body with one self design entry plus one design
/// and one XREF reference per `(relative_path, role)` pair.
pub(super) fn redirections_json(own_name: &str, targets: &[(&str, &str)]) -> String {
    let mut designs = vec![format!(
        r#"{{"file-version":1,"targetFileName":"{own_name}","displayName":"root","lineageUrn":"urn:adsk.wipprod:dm.lineage:RootKey","versionUrn":"urn:adsk.wipprod:fs.file:vf.RootKey?version=1"}}"#
    )];
    let mut references = Vec::new();
    for (ordinal, (path, role)) in targets.iter().enumerate() {
        designs.push(format!(
            r#"{{"file-version":1,"targetFileName":"{path}","displayName":"component{ordinal}","lineageUrn":"urn:adsk.wipprod:dm.lineage:Key{ordinal}","versionUrn":"urn:adsk.wipprod:fs.file:vf.Key{ordinal}?version=1"}}"#
        ));
        references.push(format!(
            r#"{{"from":"{own_name}","relativePath":"{path}","type":"XREF","properties":[{{"neutronRole":{{"value":"{role}","dataType":"STRING"}}}},{{"neutronData":{{"value":"{role}","dataType":"STRING"}}}}]}}"#
        ));
    }
    format!(
        r#"{{"name":"RedirectionsStream","schema-version":0,"designs":[{}],"references":[{}]}}"#,
        designs.join(","),
        references.join(",")
    )
}

/// A BREP-less `.f3d` with a docstruct `Properties.dat` and a redirections
/// table referencing `targets`.
pub(super) fn f3d_without_brep(
    doc_type: &str,
    own_name: &str,
    targets: &[(&str, &str)],
) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("Properties.dat", stored).unwrap();
    let properties = format!(
        r#"{{"docstruct":{{"version":"1.0.0","type":"{doc_type}","subtype":"synthetic","attributes":{{}}}}}}"#
    );
    zip.write_all(&u32::try_from(properties.len()).unwrap().to_le_bytes())
        .unwrap();
    zip.write_all(properties.as_bytes()).unwrap();
    zip.start_file("ComponentReferenceData.json", stored)
        .unwrap();
    zip.write_all(b"{}").unwrap();
    zip.start_file("RedirectionsStream.dat", stored).unwrap();
    zip.write_all(redirections_json(own_name, targets).as_bytes())
        .unwrap();
    zip.finish().unwrap().into_inner()
}

/// A minimal ASM text stream: the three header lines, an `asmheader`, one `body`
/// record, and the terminator. Written from the encoding's own structure so the
/// fixture exercises classification without carrying a decodable payload.
pub(super) fn synthetic_asm_text_stream() -> Vec<u8> {
    let mut text = String::new();
    text.push_str("21800 0 1 12           \n");
    text.push_str("16 Autodesk Neutron 23 ASM 218.0.1.400 Unknown 8 Synthetic \n");
    text.push_str("10 9.999999999999999547e-07 1.000000000000000036e-10 \n");
    text.push_str("asmheader $-1 -1 @11 218.0.1.400 #\n");
    text.push_str("body $-1 -1 $-1 $-1 $-1 $-1 #\n");
    text.push_str("End-of-ASM-data\n");
    text.into_bytes()
}

/// A BREP-less `.f3d` whose `Breps.BlobParts` holds text-encoded ASM members
/// only. This is the shape of an early-generation archive: the text streams
/// are the document's geometry carriers.
pub(super) fn f3d_with_text_brep(members: &[&str]) -> Vec<u8> {
    let base = f3d_without_brep("part-design", "part.f3d", &[]);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut source = zip::ZipArchive::new(Cursor::new(base)).unwrap();
    for i in 0..source.len() {
        let mut entry = source.by_index(i).unwrap();
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
        zip.start_file(name, stored).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    for member in members {
        zip.start_file(*member, stored).unwrap();
        zip.write_all(&synthetic_asm_text_stream()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// Wrap members into a `.f3z` archive with `Manifest.json` naming the root.
pub(super) fn f3z_archive(root_name: &str, members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    for (name, bytes) in members {
        zip.start_file(*name, stored).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.start_file("Manifest.json", stored).unwrap();
    zip.write_all(format!(r#"{{"root":"{root_name}"}}"#).as_bytes())
        .unwrap();
    zip.start_file("DesignDescription.json", stored).unwrap();
    zip.write_all(br#"{"name":"Autodesk Design Description","version":"0.1","designDescription":{"id":"0","designGraphs":[]}}"#)
        .unwrap();
    zip.finish().unwrap().into_inner()
}

pub(super) const XREF_ROLE: &str = "aaaabbbb-cccc-dddd-eeee-ffff00001111";

#[test]
fn assembly_root_without_brep_is_not_a_blocking_loss() {
    let archive = f3d_without_brep("assembly-design", "root.f3d", &[("comp.f3d", XREF_ROLE)]);
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert!(
        decoded
            .report
            .losses
            .iter()
            .all(|loss| loss.severity < cadmpeg_ir::report::Severity::Error),
        "assembly document must not report blocking/error losses: {:?}",
        decoded.report.losses
    );
    assert!(decoded
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("assembly document")));
    assert!(decoded
        .report
        .notes
        .iter()
        .any(|note| note.contains("comp.f3d") && note.contains(XREF_ROLE)));
    let native =
        crate::native::F3dNative::load(decoded.ir.native.namespace("f3d").unwrap()).unwrap();
    assert_eq!(native.xref_designs.len(), 2);
    assert_eq!(native.xref_references.len(), 1);
    assert_eq!(native.xref_references[0].relative_path, "comp.f3d");
    assert_eq!(native.xref_references[0].neutron_role, XREF_ROLE);
    let source = decoded.ir.source.unwrap();
    assert_eq!(
        source.attributes.get("docstruct_type").map(String::as_str),
        Some("assembly-design")
    );
}

#[test]
fn part_without_brep_keeps_blocking_losses() {
    // A leaf redirections table (no outgoing references) does not make a
    // BREP-less part a valid assembly.
    let archive = f3d_without_brep("part-design", "part.f3d", &[]);
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert!(decoded
        .report
        .losses
        .iter()
        .any(|loss| loss.severity == cadmpeg_ir::report::Severity::Blocking));
}

/// A document with no ASM BREP stream has no selected stream, so the geometry
/// and topology losses must not name a decode failure of one. Stating a cause
/// that was never reached misreports which carrier is missing.
#[test]
fn brep_less_part_reports_an_absent_stream_not_a_failed_decode() {
    let archive = f3d_without_brep("part-design", "part.f3d", &[]);
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    let message = |code: LossCode| {
        let loss = decoded
            .report
            .losses
            .iter()
            .find(|loss| loss.code == code)
            .unwrap_or_else(|| panic!("{code:?} not reported: {:?}", decoded.report.losses));
        loss.message.clone()
    };

    let geometry = message(LossCode::shared(LossTaxonomy::GeometryNotTransferred));
    assert!(
        geometry.contains("declares no ASM BREP stream"),
        "geometry loss must state the absent stream: {geometry}"
    );
    assert!(
        !geometry.contains("selected stream"),
        "no stream was selected, so none can have failed to decode: {geometry}"
    );

    let topology = message(LossCode::shared(LossTaxonomy::TopologyNotTransferred));
    assert!(
        topology.contains("declares no ASM BREP stream"),
        "topology loss must state the absent stream: {topology}"
    );

    assert_eq!(
        message(LossCode::shared(LossTaxonomy::MissingGeometryStream)),
        "no ASM BREP stream (.smb/.smbh) was found in the container"
    );

    // The decode ran; advising the reader to run it is container-only advice.
    assert!(
        !decoded
            .report
            .notes
            .iter()
            .any(|note| note.starts_with("container-level inspection only")),
        "full decode must not carry container-only advice: {:?}",
        decoded.report.notes
    );
}

/// A document whose only geometry carrier uses the text encoding does declare a
/// carrier. Reporting an absent stream names the wrong gap: the geometry is in
/// the archive and its encoding is not read. The two statements send a reader to
/// A geometry-less text carrier is reported as a carrier whose decode
/// produced nothing, not as an absent stream.
#[test]
fn a_text_only_carrier_without_geometry_is_reported_as_empty_not_absent() {
    let archive = f3d_with_text_brep(&[
        "FusionAssetName[Active]/Breps.BlobParts/BREP0.sat",
        "FusionAssetName[Active]/Breps.BlobParts/BREP1.sat",
    ]);
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    let message = |code: LossCode| {
        let loss = decoded
            .report
            .losses
            .iter()
            .find(|loss| loss.code == code)
            .unwrap_or_else(|| panic!("{code:?} not reported: {:?}", decoded.report.losses));
        loss.message.clone()
    };

    let geometry = message(LossCode::shared(LossTaxonomy::GeometryNotTransferred));
    assert!(
        geometry.contains("text-encoded ASM stream(s)") && geometry.contains("BREP0.sat"),
        "geometry loss must name the empty text carrier: {geometry}"
    );
    assert!(
        !geometry.contains("declares no ASM BREP stream"),
        "a text carrier is a declared carrier: {geometry}"
    );

    let topology = message(LossCode::shared(LossTaxonomy::TopologyNotTransferred));
    assert!(
        topology.contains("text-encoded"),
        "topology loss must name the encoding: {topology}"
    );

    assert_eq!(
        message(LossCode::shared(LossTaxonomy::MissingGeometryStream)),
        "2 ASM BREP stream(s) are present in the text encoding (.sat/.smt) and produced no \
         geometry; no binary stream (.smb/.smbh) was found"
    );
}

/// A text carrier with a complete solid decodes through the shared B-rep
/// path: the loopless closed sphere face reaches the model arenas with its
/// radius in millimetres per the stream's unit rule.
#[test]
fn a_text_carrier_with_geometry_decodes_through_the_shared_brep_path() {
    let mut text = String::new();
    text.push_str("23200 0 2 2 \n");
    text.push_str("16 Autodesk Neutron 21 ASM 232.4.0.65535 OSX 9 Synthetic \n");
    text.push_str("1 9.999999999999999547e-07 1.000000000000000036e-10 \n");
    text.push_str("asmheader $-1 -1 @13 232.4.0.65535 #\n");
    text.push_str("body $-1 -1 $-1 $2 $-1 $-1 #\n");
    text.push_str("lump $-1 -1 $-1 $-1 $3 $1 #\n");
    text.push_str("shell $-1 -1 $-1 $-1 $-1 $4 $-1 $2 #\n");
    text.push_str("face $-1 -1 $-1 $-1 $-1 $3 $-1 $5 forward single #\n");
    text.push_str("sphere-surface $-1 -1 $-1 0 0 0 25 1 0 0 0 0 1 forward_v I I I I #\n");
    text.push_str("End-of-ASM-data\n");

    let base = f3d_without_brep("part-design", "part.f3d", &[]);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut source = zip::ZipArchive::new(Cursor::new(base)).unwrap();
    for i in 0..source.len() {
        let mut entry = source.by_index(i).unwrap();
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
        zip.start_file(name, stored).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/BREP0.sat", stored)
        .unwrap();
    zip.write_all(text.as_bytes()).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.faces.len(), 1);
    assert_eq!(decoded.ir.model.surfaces.len(), 1);
    let surface = &decoded.ir.model.surfaces[0];
    let cadmpeg_ir::geometry::SurfaceGeometry::Sphere { radius, .. } = &surface.geometry else {
        panic!("sphere carrier expected, got {:?}", surface.geometry);
    };
    // 25 stream units at scale 1 (millimetres per unit) are 25 mm.
    assert!((radius - 25.0).abs() < 1e-9);
}

/// Several BREP streams with no Design body map leave the selection ambiguous.
/// The streams are present, so a note claiming none was found is false; the
/// finding is that none of them is identified as the document's geometry.
#[test]
fn ambiguous_brep_selection_reports_the_streams_that_are_present() {
    let archive = synthetic_ambiguous_multi_brep_f3d();
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(archive),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    let message = |code: LossCode| {
        let loss = decoded
            .report
            .losses
            .iter()
            .find(|loss| loss.code == code)
            .unwrap_or_else(|| panic!("{code:?} not reported: {:?}", decoded.report.losses));
        loss.message.clone()
    };

    assert_eq!(
        message(LossCode::shared(LossTaxonomy::MissingGeometryStream)),
        "2 ASM BREP stream(s) are present, but none of them was selected as the document's \
         geometry stream"
    );
    let geometry = message(LossCode::shared(LossTaxonomy::GeometryNotTransferred));
    assert!(
        geometry.contains("2 BREP stream(s) were located") && geometry.contains("ambiguous"),
        "geometry loss must state the ambiguous selection: {geometry}"
    );
    assert!(
        !geometry.contains("selected stream"),
        "no stream was selected: {geometry}"
    );
}

/// Two BREP streams, no history partition to break the tie and no `Design1`
/// pair to select the legacy complete set.
pub(super) fn synthetic_ambiguous_multi_brep_f3d() -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    for name in ["first", "second"] {
        let mut smb = synthetic_smbh();
        smb[39..47].copy_from_slice(&2u64.to_le_bytes()); // no history partition
        smb.truncate(60);
        zip.start_file(
            format!("FusionAssetName[Active]/Breps.BlobParts/BREP.{name}.smb"),
            stored,
        )
        .unwrap();
        zip.write_all(&smb).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn redirections_leaf_form_parses_empty_object_references() {
    let table = crate::xref::parse(
        br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"part.f3d","displayName":"part","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":{}}"#,
    )
    .unwrap();
    assert_eq!(table.designs.len(), 1);
    assert_eq!(table.designs[0].target_file_name, "part.f3d");
    assert!(table.references.is_empty());
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
    assert!(decoded.report.geometry_transferred);
    assert!(
        decoded
            .report
            .losses
            .iter()
            .all(|loss| loss.severity < cadmpeg_ir::report::Severity::Error),
        "{:?}",
        decoded.report.losses
    );
    assert!(decoded
        .report
        .notes
        .iter()
        .any(|note| note.contains("merged 1 external occurrence")));
    assert_eq!(
        decoded.ir.model.bodies.len(),
        component_alone.ir.model.bodies.len()
    );
    assert_eq!(
        decoded.ir.model.faces.len(),
        component_alone.ir.model.faces.len()
    );
    assert_eq!(
        decoded.ir.model.points.len(),
        component_alone.ir.model.points.len()
    );
    let prefix = format!("f3d:xref/{XREF_ROLE}/");
    let body = &decoded.ir.model.bodies[0];
    assert!(body.id.0.starts_with(&prefix), "{}", body.id.0);
    for shell_owner in &decoded.ir.model.shells {
        assert!(
            shell_owner.id.0.starts_with(&prefix),
            "occurrence graph must stay internally consistent: {}",
            shell_owner.id.0
        );
    }
    assert!(decoded
        .source_fidelity
        .retained_record(crate::ids::FILE_SOURCE_IMAGE_ID)
        .is_none());
    assert_eq!(
        decoded.source_fidelity.annotations.provenance.len(),
        component_alone.source_fidelity.annotations.provenance.len()
    );
    assert!(decoded
        .source_fidelity
        .annotations
        .provenance
        .keys()
        .all(|id| id.starts_with(&prefix)));
    let mut regenerated = Vec::new();
    let report = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &decoded.ir,
            fidelity: Some(&decoded.source_fidelity),
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
    let component_unknowns = component_alone.ir.native_unknowns("f3d").unwrap();
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
    let merged_unknowns = decoded.ir.native_unknowns("f3d").unwrap();
    assert_eq!(merged_unknowns.len(), component_unknowns.len());
    assert!(merged_unknowns
        .iter()
        .all(|record| record.id.0.starts_with(&prefix)));
    let validation = cadmpeg_ir::validate_neutral(&decoded.ir, decoded.report.losses.clone());
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
        .source_fidelity
        .retained_record(crate::ids::FILE_SOURCE_IMAGE_ID)
        .is_some());
    let mut replayed = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &decoded.ir,
            fidelity: Some(&decoded.source_fidelity),
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
        decoded.ir.model.bodies.len(),
        component_alone.ir.model.bodies.len()
    );
    assert!(decoded
        .report
        .notes
        .iter()
        .any(|note| note.contains("merged 2 external occurrence")));
    let body_id = &decoded.ir.model.bodies[0].id.0;
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

    assert!(decoded.report.losses.iter().any(|loss| {
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

/// A report carrying the BREP-less geometry losses that `build_container_report`
/// states before the design segment is classified.
pub(super) fn brep_less_geometry_report() -> cadmpeg_ir::report::DecodeReport {
    let loss = |code: LossCode, severity| cadmpeg_ir::report::LossNote {
        code,
        severity,
        message: "stated before classification".to_owned(),
        provenance: None,
    };
    cadmpeg_ir::report::DecodeReport {
        format: "f3d".to_owned(),
        container_only: false,
        geometry_transferred: false,
        coverage: std::collections::BTreeMap::new(),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses: vec![
            loss(
                LossCode::shared(LossTaxonomy::GeometryNotTransferred),
                Severity::Blocking,
            ),
            loss(
                LossCode::shared(LossTaxonomy::TopologyNotTransferred),
                Severity::Blocking,
            ),
            loss(
                LossCode::shared(LossTaxonomy::MissingGeometryStream),
                Severity::Error,
            ),
        ],
        notes: Vec::new(),
    }
}

/// A design whose content is sketch curves declares no body, so it has no
/// B-rep to lose: the sketch entities are its complete geometry.
#[test]
fn sketch_only_design_is_not_a_geometry_loss() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 0, 0, 0, 13, 0);
    assert!(report.geometry_transferred);
    assert!(
        report
            .losses
            .iter()
            .all(|loss| loss.severity < Severity::Error),
        "sketch-only design must not keep blocking losses: {:?}",
        report.losses
    );
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains("13 sketch entity(s)")));
}

/// A reference-image timeline object is presentation content. A bodyless
/// document that contains one does not require a BREP geometry carrier.
#[test]
fn presentation_only_design_is_not_a_geometry_loss() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 0, 0, 0, 0, 1);
    assert!(report.geometry_transferred);
    assert!(
        report
            .losses
            .iter()
            .all(|loss| loss.severity < Severity::Error),
        "presentation-only design must not keep blocking losses: {:?}",
        report.losses
    );
    assert!(report.losses.iter().any(|loss| loss
        .message
        .contains("1 reference-image timeline object(s)")));
}

/// A declared body whose BREP stream is absent is a real missing carrier. Its
/// sketches do not stand in for the solid the document says it has.
#[test]
fn a_declared_body_without_a_brep_stream_keeps_its_geometry_losses() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 0, 0, 1, 13, 0);
    assert!(!report.geometry_transferred);
    assert_eq!(report.losses.len(), 3);
}

/// A document with no sketch entities transferred nothing, so nothing settles
/// the loss. An imported drawing whose only entity the importer did not author
/// produces exactly this: a document with no body and no sketch.
#[test]
fn a_document_without_sketch_entities_keeps_its_geometry_losses() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 0, 0, 0, 0, 0);
    assert!(!report.geometry_transferred);
    assert_eq!(report.losses.len(), 3);
}

/// A present BREP stream that produced no geometry is a decode failure, not a
/// sketch-only design, however many sketches the document also carries.
#[test]
fn a_present_brep_stream_is_never_reclassified_as_sketch_only() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 1, 0, 0, 13, 0);
    assert!(!report.geometry_transferred);
    assert_eq!(report.losses.len(), 3);
}

/// A text-encoded B-rep carrier is not sketch-only, regardless of sketch count.
#[test]
fn a_text_brep_carrier_is_never_reclassified_as_sketch_only() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 0, 2, 0, 13, 0);
    assert!(!report.geometry_transferred);
    assert_eq!(report.losses.len(), 3);
}

/// The text encoding of an ASM stream carries the same entity model as the
/// binary one, so its archive members are geometry carriers and must classify as
/// such. Leaving them unclassified is what let the report state that a document
/// holding one declares no carrier at all.
#[test]
fn text_encoded_asm_members_classify_as_geometry_carriers() {
    for name in [
        "Fusion[Active]/Breps.BlobParts/BREP0.sat",
        "Fusion[Active]/Breps.BlobParts/BREP1.SAT",
        "probe/body.smt",
    ] {
        assert_eq!(
            crate::container::classify(name),
            crate::container::role::BREP_TEXT,
            "{name} must classify as a text-encoded BREP carrier"
        );
    }
    assert_eq!(
        crate::container::classify("a/b.smb"),
        crate::container::role::BREP_SMB
    );
    assert_eq!(
        crate::container::classify("a/b.smbh"),
        crate::container::role::BREP_SMBH
    );
}

/// A report carrying the unconditional appearance loss that
/// `build_container_report` and `build_geometry_report` state before appearance
/// decoding runs.
pub(super) fn appearance_loss_report() -> cadmpeg_ir::report::DecodeReport {
    cadmpeg_ir::report::DecodeReport {
        format: "f3d".to_owned(),
        container_only: false,
        geometry_transferred: false,
        coverage: std::collections::BTreeMap::new(),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses: vec![cadmpeg_ir::report::LossNote {
            code: LossCode::shared(LossTaxonomy::MaterialNotTransferred),
            severity: Severity::Warning,
            message: "Materials/appearances (.protein assets, ACT/design assignments) were not \
                      transferred."
                .to_owned(),
            provenance: None,
        }],
        notes: Vec::new(),
    }
}

pub(super) fn opaque_appearance(guid: &str) -> cadmpeg_ir::appearance::Appearance {
    cadmpeg_ir::appearance::Appearance {
        id: cadmpeg_ir::ids::AppearanceId(format!("f3d:design:appearance#{guid}")),
        name: Some("Prism-Opaque".to_owned()),
        asset_guid: Some(guid.to_owned()),
        library_id: None,
        visual_guid: Some(guid.to_owned()),
        physical_token: None,
        schema: Some("PrismOpaqueSchema".to_owned()),
        category: None,
        base_color: Some(cadmpeg_ir::topology::Color {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        }),
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    }
}

pub(super) fn material_losses(report: &cadmpeg_ir::report::DecodeReport) -> Vec<&str> {
    report
        .losses
        .iter()
        .filter(|loss| loss.code.category() == cadmpeg_ir::report::LossCategory::Material)
        .map(|loss| loss.message.as_str())
        .collect()
}

#[test]
fn appearance_loss_stands_when_no_asset_decodes() {
    let ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut report = appearance_loss_report();
    crate::decode::reconcile_appearance_loss(&mut report, &ir, false);
    assert_eq!(material_losses(&report).len(), 1);
}

#[test]
fn appearance_loss_clears_when_an_unassigned_catalog_transfers() {
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.appearances = vec![opaque_appearance("2F0E19C1-0000-4000-8000-000000000001")];
    let mut report = appearance_loss_report();
    crate::decode::reconcile_appearance_loss(&mut report, &ir, false);
    assert!(material_losses(&report).is_empty());
}

#[test]
fn appearance_loss_counts_assets_whose_assignment_is_unresolved() {
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.appearances = vec![
        opaque_appearance("2F0E19C1-0000-4000-8000-000000000001"),
        opaque_appearance("2F0E19C1-0000-4000-8000-000000000002"),
    ];
    let mut report = appearance_loss_report();
    crate::decode::reconcile_appearance_loss(&mut report, &ir, true);
    let messages = material_losses(&report);
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("2 Protein appearance asset(s)"));
}

#[test]
fn appearance_loss_clears_when_an_assignment_resolves() {
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let appearance = opaque_appearance("2F0E19C1-0000-4000-8000-000000000001");
    ir.model.appearance_bindings = vec![cadmpeg_ir::appearance::AppearanceBinding {
        id: "f3d:appearance:body#0_1:2F0E19C1-0000-4000-8000-000000000001".to_owned(),
        target: cadmpeg_ir::appearance::AppearanceTarget::Body(cadmpeg_ir::ids::BodyId(
            "f3d:brep/a.smbh/brep:entity#1".to_owned(),
        )),
        appearance: appearance.id.clone(),
        source_entity_id: None,
        object_type: None,
        channels: std::collections::BTreeMap::new(),
    }];
    ir.model.appearances = vec![appearance];
    let mut report = appearance_loss_report();
    crate::decode::reconcile_appearance_loss(&mut report, &ir, true);
    assert!(material_losses(&report).is_empty());
}
