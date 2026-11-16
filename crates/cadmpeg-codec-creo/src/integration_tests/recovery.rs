// SPDX-License-Identifier: Apache-2.0

use crate::container;
use crate::test_support::{assert_annotation, build_prt, visibgeom_payload};
use crate::CreoCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::Exactness;
use cadmpeg_test_support::EditableDecodeResult;
use std::io::Cursor;

#[test]
fn compressed_section_recovery_reports_loss_and_retains_the_bounded_source() {
    for name in [
        "SolidPrimdata",
        "DispDataTable",
        "THMB_IMG_MAIN",
        "VisibGeom",
        "DEPDB_DATA",
    ] {
        let mut bytes = b"#UGC:2 P test\n#-END_OF_UGC_HEADER\n".to_vec();
        let header_base = bytes.len();
        bytes.extend_from_slice(format!("{:<80}\n", "#UGC_TOC 2 2 81 17").as_bytes());
        let mut damaged = format!("#{name}\n").into_bytes();
        damaged.extend_from_slice(&[0x1f, 0x9d, 0x10, 0x41, 0x58, 0x02]);
        // A rejected compressed body must not become a raw PSB input.
        damaged.extend_from_slice(b"srf_array\0\xf8\x05");
        let mut readable = b"#SolidPersistTable\n".to_vec();
        readable.extend_from_slice(&[0x1f, 0x9d, 0x10, 0x41, 0x84, 0x0c, 0x01]);
        let first_offset = 3 * 81;
        let second_offset = first_offset + damaged.len();
        for row in [
            format!("{name} {first_offset:x} {:x} 5", damaged.len()),
            format!("SolidPersistTable {second_offset:x} {:x} 3", readable.len()),
        ] {
            bytes.extend_from_slice(format!("{row:<80}\n").as_bytes());
        }
        assert_eq!(bytes.len(), header_base + first_offset);
        bytes.extend_from_slice(&damaged);
        bytes.extend(readable);
        let decoded = CreoCodec
            .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
            .expect("independent section is decoded");
        let decoded = EditableDecodeResult::from(decoded);
        let losses = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.to_string() == "creo/container.compressed-section-unexpanded")
            .collect::<Vec<_>>();
        assert_eq!(losses.len(), 1);
        assert!(losses[0].message.contains("undefined code"));
        assert!(decoded
            .source_fidelity()
            .retained_records()
            .values()
            .any(|record| record.offset()
                == cadmpeg_core::decode::u64_from_index(header_base + first_offset)
                && record.data() == Some(damaged.as_slice())));
        let scan = crate::container::scan_bytes_ok(bytes);
        assert_eq!(scan.framing.sections.len(), 2);
        assert_eq!(scan.framing.expanded_sections.len(), 1);
        assert_eq!(scan.framing.expanded_sections[0].data, b"ABC");
        assert_eq!(scan.framing.census.srf_array_count, None);
        assert!(scan.framing.unowned_physical_geometry.is_empty());
        assert!(!decoded.report().losses.iter().any(
            |loss| loss.code.to_string() == "creo/container.physical-geometry-owner-unresolved"
        ));
    }
}

#[test]
fn physical_depdb_geometry_retains_the_refused_section() {
    for sparse_visible in [false, true] {
        let mut payload = visibgeom_payload(1, 0);
        payload.extend_from_slice(&[7, 0x22, 4, 1, 0, 0]);
        let data = if sparse_visible {
            build_prt("c", &[("VisibGeom", vec![0]), ("DEPDB_DATA", payload)])
        } else {
            build_prt("c", &[("DEPDB_DATA", payload)])
        };
        assert_physical_geometry_retained(&data);
    }
}

fn assert_physical_geometry_retained(data: &[u8]) {
    let scan = container::scan_bytes_ok(data.to_vec());
    let section = scan
        .framing
        .sections
        .iter()
        .find(|section| section.name() == "DEPDB_DATA")
        .expect("physical metadata section");
    let expected =
        container::section_region(&scan.framing.data, section).expect("complete physical extent");
    let result = EditableDecodeResult::from(
        CreoCodec
            .decode(&mut Cursor::new(data), &DecodeOptions::default())
            .expect("unowned geometry retained"),
    );
    let record = result
        .ir()
        .native_unknowns("creo")
        .expect("retained source")
        .into_iter()
        .find(|record| record.id.as_str().starts_with("creo:DEPDB_DATA:section#"))
        .expect("exact physical namespace");
    assert_eq!(
        result
            .source_fidelity()
            .retained_record(record.id.as_str())
            .expect("physical bytes")
            .data(),
        Some(expected)
    );
    assert_annotation(
        &result.source_fidelity().annotations,
        record.id.as_str(),
        "creo:DEPDB_DATA",
        u64::try_from(section.offset()).expect("small fixture"),
        "unowned_physical_geometry_section",
        Exactness::ByteExact,
    );
    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == "creo/container.physical-geometry-owner-unresolved")
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Warning);
    assert_eq!(
        losses[0].code.taxonomy(),
        cadmpeg_ir::report::loss::LossTaxonomy::RecordNotTyped
    );
}
