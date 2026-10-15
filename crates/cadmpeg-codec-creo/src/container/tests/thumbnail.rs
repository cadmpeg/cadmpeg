// SPDX-License-Identifier: Apache-2.0

use super::super::{has_thumbnail, scan_bytes_ok, ContainerScan, JPEG_MAGIC};
use crate::test_support::{build_toc_section_prt, jpeg_payload, unix_compress_literals};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn assert_positive_work(scan: &ContainerScan<'_>, operations: &[(&str, u64)]) {
    assert!(crate::test_support::assert_work_boundaries(
        &operations
            .iter()
            .map(|(operation, _)| *operation)
            .collect::<Vec<_>>(),
        |ctx| has_thumbnail(ctx, scan)
    ));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let work: u64 = operations.iter().map(|(_, amount)| amount).sum();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(has_thumbnail(&ctx, scan).expect("source-derived thumbnail work"));
    let refusal = ctx
        .charge_work_limit(1, "after thumbnail work")
        .expect_err("exact cap exhausted");
    assert_eq!((refusal.used, refusal.additional), (work, 1));
}

#[test]
fn raw_jpeg_witness_skips_unrelated_expanded_sections() {
    const NAME: &str = "THMB_IMG_MAIN";
    let jpeg = jpeg_payload();
    let body = unix_compress_literals(b"ABC");
    let mut data = b"#UGC:2 P test\n#-END_OF_UGC_HEADER\n".to_vec();
    let header_base = data.len();
    data.extend_from_slice(format!("{:<80}\n", "#UGC_TOC 2 2 81 17").as_bytes());
    let thumbnail_header = format!("#{NAME}\n");
    let thumbnail_length = thumbnail_header.len() + jpeg.len();
    let body_header = "#Body\n";
    let body_length = body_header.len() + body.len();
    let thumbnail_offset = 3 * 81;
    let body_offset = thumbnail_offset + thumbnail_length;
    for row in [
        format!("{NAME} {thumbnail_offset:x} {thumbnail_length:x} 0"),
        format!("Body {body_offset:x} {body_length:x} 3"),
    ] {
        data.extend_from_slice(format!("{row:<80}\n").as_bytes());
    }
    assert_eq!(data.len(), header_base + thumbnail_offset);
    data.extend_from_slice(thumbnail_header.as_bytes());
    data.extend_from_slice(&jpeg);
    data.extend_from_slice(body_header.as_bytes());
    data.extend_from_slice(&body);
    let scan = scan_bytes_ok(data);
    assert_eq!(scan.framing.sections.len(), 2);
    assert_eq!(scan.framing.expanded_sections.len(), 1);
    assert_eq!(scan.framing.expanded_sections[0].name, "Body");
    assert_eq!(scan.framing.expanded_sections[0].data, b"ABC");
    assert_positive_work(
        &scan,
        &[
            ("creo thumbnail section selection", 1),
            (
                "find Creo thumbnail",
                u64::try_from(thumbnail_length + JPEG_MAGIC.len()).expect("raw search extent"),
            ),
        ],
    );
}

#[test]
fn compressed_jpeg_witness_uses_expanded_payload_search() {
    const NAME: &str = "THMB_IMG_MAIN";
    let jpeg = jpeg_payload();
    let compressed = unix_compress_literals(&jpeg);
    let scan = scan_bytes_ok(build_toc_section_prt(NAME, &compressed, jpeg.len()));
    assert_eq!(scan.framing.sections.len(), 1);
    assert_eq!(scan.framing.expanded_sections.len(), 1);
    assert_eq!(scan.framing.expanded_sections[0].name, NAME);
    assert_eq!(scan.framing.expanded_sections[0].data, jpeg);
    assert_positive_work(
        &scan,
        &[
            ("creo thumbnail section selection", 1),
            ("creo expanded section selection", 1),
            (
                "creo expanded section name comparison",
                u64::try_from(2 * NAME.len()).expect("two name comparisons"),
            ),
            (
                "find Creo expanded thumbnail",
                u64::try_from(jpeg.len() + JPEG_MAGIC.len()).expect("expanded search extent"),
            ),
        ],
    );
}
