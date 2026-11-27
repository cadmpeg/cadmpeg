// SPDX-License-Identifier: Apache-2.0

use crate::pmdc::{PmDcListMetadata, PmDcReference, PmDcReferenceList};

#[test]
fn spatial_sketch_members_resolve_in_the_owning_segment() {
    let type_id = crate::pmdc::type_id_string(crate::sketch::spatial::TYPE);
    let record = cadmpeg_core::serde_erase::json::from_value(cadmpeg_core::json!({
        "id": "inventor:pmdc:spatial-sketch#segment-0",
        "segment_token": "segment", "record_ordinal": 0, "type_id": type_id,
        "save_version_major": 31,
        "header": {
            "header_value": 0, "header_id": 1,
            "next": PmDcReference::from_packed(0),
            "flags": 0, "flags_extension": 0,
            "context": PmDcReference::from_packed(0), "source_index": 1
        },
        "state": 0, "count_value": 0,
        "entities": {"marker": 8, "metadata": null, "references": []},
        "values": [0,0], "lower": [0,0,0], "upper": [1,1,1]
    }))
    .expect("spatial sketch record");
    let raw = cadmpeg_core::serde_erase::json::from_value(cadmpeg_core::json!({
        "id": "inventor:rse:record#segment-0", "token": "segment", "ordinal": 0,
        "selector": 0, "type_index": 0, "type_id": type_id,
        "payload_offset": 0, "payload_len": 0, "payload_sha256": "",
        "trailing_payload_len": 0, "trailer_len": 0, "trailer_sha256": ""
    }))
    .expect("raw record");
    let mut data = super::empty_data();
    data.pm_dc_spatial_sketches = vec![record];
    data.records = vec![raw];
    let ir = cadmpeg_ir::CadIr::empty();
    let mut findings = Vec::new();
    super::super::validate_sketches(&data, &ir, &mut findings);
    assert!(findings.is_empty());
    data.pm_dc_spatial_sketches[0].edit(|record| {
        record.entities = PmDcReferenceList::new(
            8,
            Some(PmDcListMetadata::U16([16, 0])),
            vec![PmDcReference::from_packed(0x8000_0002)],
        )
        .expect("member list");
    });
    super::super::validate_sketches(&data, &ir, &mut findings);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].check,
        cadmpeg_ir::report::check::Check::NativeLinks
    );
    assert_eq!(
        findings[0].entity.as_deref(),
        Some("inventor:pmdc:spatial-sketch#segment-0")
    );
}
