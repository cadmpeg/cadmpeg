// SPDX-License-Identifier: Apache-2.0
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

use std::io::{Cursor, Write};

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::LossKind;
use zip::CompressionMethod;

use crate::loss::F3dLossCode;
use crate::test_support::*;
use crate::F3dCodec;

/// A document with no ASM BREP stream has no selected stream, so the geometry
/// and topology losses must not name a decode failure of one. Stating a cause
/// that was never reached misreports which carrier is missing.
#[test]
fn brep_less_part_reports_an_absent_stream_not_a_failed_decode() {
    let archive = f3d_without_brep("part-design", "part.f3d", &[]);
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    let message = |code: LossKind| {
        let loss = decoded
            .report()
            .losses
            .iter()
            .find(|loss| loss.code == code)
            .unwrap_or_else(|| panic!("{code:?} not reported: {:?}", decoded.report().losses));
        loss.message.clone()
    };

    let geometry = message(F3dLossCode::GeometryNotTransferred.kind());
    assert!(
        geometry.contains("declares no ASM BREP stream"),
        "geometry loss must state the absent stream: {geometry}"
    );
    assert!(
        !geometry.contains("selected stream"),
        "no stream was selected, so none can have failed to decode: {geometry}"
    );

    let topology = message(F3dLossCode::TopologyNotTransferred.kind());
    assert!(
        topology.contains("declares no ASM BREP stream"),
        "topology loss must state the absent stream: {topology}"
    );

    assert_eq!(
        message(F3dLossCode::MissingGeometryStream.kind()),
        "no ASM BREP stream (.smb/.smbh) was found in the container"
    );

    // The decode ran; advising the reader to run it is container-only advice.
    assert!(
        !decoded
            .report()
            .notes
            .iter()
            .any(|note| note.starts_with("container-level inspection only")),
        "full decode must not carry container-only advice: {:?}",
        decoded.report().notes
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
    let message = |code: LossKind| {
        let loss = decoded
            .report()
            .losses
            .iter()
            .find(|loss| loss.code == code)
            .unwrap_or_else(|| panic!("{code:?} not reported: {:?}", decoded.report().losses));
        loss.message.clone()
    };

    let geometry = message(F3dLossCode::GeometryNotTransferred.kind());
    assert!(
        geometry.contains("text-encoded ASM stream(s)") && geometry.contains("BREP0.sat"),
        "geometry loss must name the empty text carrier: {geometry}"
    );
    assert!(
        !geometry.contains("declares no ASM BREP stream"),
        "a text carrier is a declared carrier: {geometry}"
    );

    let topology = message(F3dLossCode::TopologyNotTransferred.kind());
    assert!(
        topology.contains("text-encoded"),
        "topology loss must name the encoding: {topology}"
    );

    assert_eq!(
        message(F3dLossCode::MissingGeometryStream.kind()),
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
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert_eq!(decoded.ir().model.surfaces.len(), 1);
    let surface = &decoded.ir().model.surfaces[0];
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
    let message = |code: LossKind| {
        let loss = decoded
            .report()
            .losses
            .iter()
            .find(|loss| loss.code == code)
            .unwrap_or_else(|| panic!("{code:?} not reported: {:?}", decoded.report().losses));
        loss.message.clone()
    };

    assert_eq!(
        message(F3dLossCode::MissingGeometryStream.kind()),
        "2 ASM BREP stream(s) are present, but none of them was selected as the document's \
         geometry stream"
    );
    let geometry = message(F3dLossCode::GeometryNotTransferred.kind());
    assert!(
        geometry.contains("2 BREP stream(s) were located") && geometry.contains("ambiguous"),
        "geometry loss must state the ambiguous selection: {geometry}"
    );
    assert!(
        !geometry.contains("selected stream"),
        "no stream was selected: {geometry}"
    );
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
