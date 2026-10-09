// SPDX-License-Identifier: Apache-2.0

use super::super::{has_thumbnail, scan_bytes_ok, ContainerScan, JPEG_MAGIC};
use crate::test_support::{build_toc_section_prt, jpeg_payload, unix_compress_literals};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn assert_positive_work(scan: &ContainerScan<'_>, operations: &[(&'static str, u64)]) {
    let work: u64 = operations.iter().map(|(_, amount)| amount).sum();
    for allowed in 0..=work {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = allowed;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = has_thumbnail(&ctx, scan);
        let original = if allowed < work {
            let Err(CodecError::ResourceLimit(refusal)) = result else {
                panic!("the next actual thumbnail operation must refuse");
            };
            let mut used = 0;
            let &(operation, additional) = operations.iter().find(|(_, amount)| {
                if used + amount > allowed {
                    true
                } else {
                    used += amount;
                    false
                }
            }).expect("cap below source-derived work");
            assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
            assert_eq!(refusal.operation, operation);
            assert_eq!((refusal.used, refusal.additional), (used, additional));
            refusal
        } else {
            assert!(result.expect("source-derived positive thumbnail cap"));
            let refusal = ctx.charge_work_limit(1, "after positive thumbnail")
                .expect_err("exact work cap");
            assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
            assert_eq!((refusal.used, refusal.additional), (work, 1));
            refusal
        };
        assert!(matches!(has_thumbnail(&ctx, scan),
            Err(CodecError::ResourceLimit(refusal)) if refusal == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
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
    assert_positive_work(&scan, &[
        ("creo thumbnail section selection", 1),
        ("find Creo thumbnail", u64::try_from(thumbnail_length + JPEG_MAGIC.len())
            .expect("fixture search bound")),
    ]);
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
    let name_work = u64::try_from(NAME.len()).expect("fixture name bound");
    assert_positive_work(&scan, &[
        ("creo thumbnail section selection", 1),
        ("creo expanded section selection", 1),
        ("creo expanded section name comparison", name_work),
        ("creo expanded section name comparison", name_work),
        ("find Creo expanded thumbnail", u64::try_from(jpeg.len() + JPEG_MAGIC.len())
            .expect("fixture expanded search bound")),
    ]);
}
