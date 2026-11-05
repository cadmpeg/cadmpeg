// SPDX-License-Identifier: Apache-2.0
//! Independently framed SMT headers do not control B-rep survival.

use crate::test_support::assembly_test::f3d_with_text_brep_stream;
use crate::F3dCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

const HEADER_LINEAR_EPS_MM: f64 = 1.0e-12;

const SPHERE_SMT: &str = concat!(
    "23200 0 2 2\n",
    "16 Autodesk Neutron 21 ASM 232.4.0.65535 OSX 9 Synthetic\n",
    "1 0.000001 0.0000000001\n",
    "asmheader $-1 -1 @13 232.4.0.65535 #\n",
    "body $-1 -1 $-1 $2 $-1 $-1 #\n",
    "lump $-1 -1 $-1 $-1 $3 $1 #\n",
    "shell $-1 -1 $-1 $-1 $-1 $4 $-1 $2 #\n",
    "face $-1 -1 $-1 $-1 $-1 $3 $-1 $5 forward single #\n",
    "sphere-surface $-1 -1 $-1 0 0 0 25 1 0 0 0 0 1 forward_v I I I I #\n",
    "End-of-ASM-data\n",
);

#[test]
fn malformed_smt_metadata_keeps_geometry_and_reports_recovery() {
    let original = SPHERE_SMT;
    let entry = "FusionAssetName[Active]/Breps.BlobParts/Body1.smt";
    let decode = |text: &str| {
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_text_brep_stream(&[entry], text.as_bytes())),
                &DecodeOptions::default(),
            )
            .unwrap()
    };
    let expected = decode(original);
    assert_eq!(expected.ir().model.faces.len(), 1);
    for source in [
        original.replace("9 Synthetic", ""),
        original.replace(
            "16 Autodesk Neutron 21 ASM 232.4.0.65535 OSX 9 Synthetic",
            "",
        ),
        original.replace("0.000001", "invalid"),
        original.replace("0.0000000001", "NaN"),
        original.replace("0.0000000001", ""),
        original.replacen(" 0 2 2", " -1 2 2", 1),
        original.replacen(" 0 2 2", " 4294967296 2 2", 1),
        original.replacen(" 0 2 2", " invalid 2 2", 1),
    ] {
        let recovered = decode(&source);
        assert_eq!(recovered.ir().model, expected.ir().model);
        assert!(recovered.report().losses.iter().any(|loss| loss.code
            == crate::loss::F3dLossCode::TextHeaderNoncanonical
                .note("")
                .code));
        if source.contains("invalid") {
            assert_eq!(
                recovered.ir().tolerances.angular,
                expected.ir().tolerances.angular
            );
        } else if source.contains("NaN") {
            assert_eq!(
                recovered.ir().tolerances.linear,
                expected.ir().tolerances.linear
            );
        }
        assert!(cadmpeg_ir::validate_neutral(recovered.ir(), Vec::new())
            .unwrap()
            .findings
            .iter()
            .all(|finding| finding.severity < cadmpeg_ir::report::Severity::Error));
    }
}

#[test]
fn binary_product_encoding_recovery_keeps_independent_geometry() {
    use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
    use crate::test_support::zip_test::f3d_with_smbh;
    let original = synthetic_geometry_smbh();
    let decode = |source: &[u8]| {
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(source)),
                &DecodeOptions::default(),
            )
            .unwrap()
    };
    let expected = decode(&original);
    assert!(!expected.ir().model.faces.is_empty());
    let mut cursor = 47; // ASM BinaryFile8 fixed words; docs/layouts/asm.toml.
    for _ in 0..3 {
        let mut source = original.clone();
        source[cursor + 2] = 0xff;
        let recovered = decode(&source);
        assert_eq!(recovered.ir().model, expected.ir().model);
        assert_eq!(recovered.ir().tolerances, expected.ir().tolerances);
        assert!(recovered.report().losses.iter().any(
            |loss| loss.code == crate::loss::F3dLossCode::KernelHeaderMetadataUnresolved.kind()
        ));
        assert!(cadmpeg_ir::validate_neutral(recovered.ir(), Vec::new())
            .unwrap()
            .is_ok());
        cursor += 2 + usize::from(original[cursor + 1]);
    }
}

#[test]
fn truncated_stored_brep_keeps_its_complete_kernel_header() {
    use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
    use crate::test_support::smbh_header_test::smbh_header_prefix;
    use crate::test_support::zip_test::f3d_with_smbh;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&source, &arena, &policy).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let entry = archive
        .entries()
        .iter()
        .find(|entry| entry.name.ends_with("Body1.smbh"))
        .unwrap();
    let local_header = usize::try_from(entry.header_start).unwrap();
    let start = usize::try_from(entry.data_start.unwrap()).unwrap();
    let cut = start + smbh_header_prefix().len();
    crate::test_support::zip_test::with_scan(&source[..cut], |scan| {
        let header = scan
            .partial_brep_header()
            .expect("complete partial BREP header");
        assert_eq!(header.model_metadata().unwrap().linear, Some(1.0e-6));
    });
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source[..cut]), &DecodeOptions::default())
        .unwrap();
    assert!(
        (decoded
            .ir()
            .tolerances
            .linear
            .expect("stated linear tolerance")
            .get()
            - 1.0e-5)
            .abs()
            < HEADER_LINEAR_EPS_MM
    );
    assert_eq!(decoded.ir().tolerances.angular.get(), 1.0e-10);
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code
            == cadmpeg_ir::codec::DecodeLossCode::RecordUnreadable
                .note(String::new())
                .code
            && loss.message.contains("zip.entry")
    }));
    let partial_cut = cut - 9;
    crate::test_support::zip_test::with_scan(&source[..partial_cut], |scan| {
        let header = scan.partial_brep_header().unwrap();
        let metadata = header.model_metadata().unwrap();
        assert_eq!(metadata.linear, Some(1.0e-6));
        assert_eq!(metadata.angular, None);
    });
    let partial = F3dCodec
        .decode(
            &mut Cursor::new(&source[..partial_cut]),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(
        (partial
            .ir()
            .tolerances
            .linear
            .expect("stated linear tolerance")
            .get()
            - 1.0e-5)
            .abs()
            < HEADER_LINEAR_EPS_MM
    );
    assert_eq!(
        partial.ir().tolerances.angular,
        cadmpeg_ir::units::Tolerances::default().angular
    );
    let short = F3dCodec
        .decode(
            &mut Cursor::new(&source[..start]),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(short.ir().tolerances.linear, None);
    assert!(short
        .report()
        .losses
        .iter()
        .any(|loss| loss.dropped_fields.iter().any(|field| {
            field.record == cadmpeg_ir::report::loss::DOCUMENT_HEADER
                && field.field == "tolerances.linear"
        })));
    let incomplete_name = F3dCodec
        .decode(
            &mut Cursor::new(&source[..local_header + 32]),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(incomplete_name.ir().tolerances.linear, None);
    assert!(incomplete_name.report().losses.iter().any(|loss| {
        loss.dropped_fields.iter().any(|field| {
            field.record == cadmpeg_ir::report::loss::DOCUMENT_HEADER
                && field.field == "tolerances.linear"
        })
    }));
    const ZIP_LOCAL_COMPRESSED_LENGTH_OFFSET: usize = 18;
    let mut invalid = source[..cut].to_vec();
    invalid[local_header + ZIP_LOCAL_COMPRESSED_LENGTH_OFFSET
        ..local_header + ZIP_LOCAL_COMPRESSED_LENGTH_OFFSET + 4]
        .copy_from_slice(&1_u32.to_le_bytes());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(invalid), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().tolerances.linear, None);
}

#[test]
fn smt_tolerance_overflow_in_document_units_keeps_geometry() {
    let original = SPHERE_SMT.replace("1 0.000001", "10 0.000001");
    let modified = original.replace("0.000001", "1.7976931348623157e308");
    let entry = "FusionAssetName[Active]/Breps.BlobParts/Body1.smt";
    let decode = |source: &str| {
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_text_brep_stream(&[entry], source.as_bytes())),
                &DecodeOptions::default(),
            )
            .unwrap()
    };
    let expected = decode(&original);
    let recovered = decode(&modified);
    assert_eq!(recovered.ir().model, expected.ir().model);
    assert_eq!(
        recovered.ir().tolerances.linear,
        cadmpeg_ir::units::Tolerances::default().linear
    );
    assert_eq!(
        recovered.ir().tolerances.angular,
        expected.ir().tolerances.angular
    );
    assert!(recovered
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::KernelHeaderToleranceUnresolved.kind()));
}

#[test]
fn invalid_binary_kernel_tolerances_keep_independent_geometry() {
    use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
    use crate::test_support::smbh_header_test::smbh_header_prefix;
    use crate::test_support::zip_test::f3d_with_smbh;
    let original = synthetic_geometry_smbh();
    let decode = |source: &[u8]| {
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(source)),
                &DecodeOptions::default(),
            )
            .unwrap()
    };
    let expected = decode(&original);
    assert!(!expected.ir().model.faces.is_empty());
    // The header builder ends with three tagged doubles; each payload is 8 bytes.
    let header_end = smbh_header_prefix().len();
    for (offset, values) in [
        (
            header_end - 17,
            &[f64::NAN, f64::INFINITY, -1.0, 0.0, f64::MAX][..],
        ),
        (header_end - 8, &[f64::NAN, f64::INFINITY, -1.0, 0.0][..]),
    ] {
        for value in values {
            let mut modified = original.clone();
            modified[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            let recovered = decode(&modified);
            assert_eq!(recovered.ir().model, expected.ir().model);
            assert!(recovered.report().losses.iter().any(|loss| loss.code
                == crate::loss::F3dLossCode::KernelHeaderToleranceUnresolved.kind()));
            assert!(cadmpeg_ir::validate_neutral(recovered.ir(), Vec::new())
                .unwrap()
                .is_ok());
        }
    }
}

#[test]
fn manifest_metadata_recovery_preserves_geometry_and_eligible_archives() {
    use std::io::{Read, Write};
    let original = crate::test_support::zip_test::f3d_with_smbh(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
    );
    let expected = F3dCodec
        .decode(&mut Cursor::new(&original), &DecodeOptions::default())
        .unwrap();
    for field in 0..5 {
        let mut source = zip::ZipArchive::new(Cursor::new(&original)).unwrap();
        let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for index in 0..source.len() {
            let mut entry = source.by_index(index).unwrap();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            if entry.name().expect("entry name") == "Manifest.dat" {
                if field == 0 {
                    data[4] = 0xff;
                } else {
                    let mut cursor = 0;
                    for _ in 0..2 {
                        let count = usize::try_from(u32::from_le_bytes(
                            data[cursor..cursor + 4].try_into().unwrap(),
                        ))
                        .expect("fixture string length fits usize");
                        cursor += 4 + count;
                    }
                    for _ in 0..field {
                        let count = usize::try_from(u32::from_le_bytes(
                            data[cursor..cursor + 4].try_into().unwrap(),
                        ))
                        .expect("fixture string length fits usize");
                        cursor += 4 + 2 * count;
                    }
                    data[cursor + 4..cursor + 6].copy_from_slice(&0xd800u16.to_le_bytes());
                }
            }
            output
                .start_file(
                    entry.name().expect("entry name"),
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            output.write_all(&data).unwrap();
        }
        let bytes = output.finish().unwrap().into_inner();
        let recovered = cadmpeg_test_support::EditableDecodeResult::from(
            F3dCodec
                .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
                .unwrap(),
        );
        assert_eq!(recovered.ir().model, expected.ir().model);
        assert!(recovered
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::ManifestMetadataUnresolved.kind()));
        assert!(cadmpeg_ir::validate_neutral(recovered.ir(), Vec::new())
            .unwrap()
            .is_ok());
        let mut written = Vec::new();
        let path = crate::test_support::plan_inherited_write(
            recovered.ir(),
            recovered.source_fidelity(),
            &mut written,
        )
        .unwrap();
        if field == 0 {
            assert!(matches!(
                path,
                cadmpeg_ir::report::export::WritePath::Synthesized { .. }
            ));
            let output = F3dCodec
                .decode(&mut Cursor::new(&written), &DecodeOptions::default())
                .unwrap();
            assert_eq!(
                output.ir().model.faces.len(),
                recovered.ir().model.faces.len()
            );
        } else {
            assert_eq!(written, bytes);
        }
    }
}
