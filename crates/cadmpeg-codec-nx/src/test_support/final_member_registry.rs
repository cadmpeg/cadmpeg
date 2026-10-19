// SPDX-License-Identifier: Apache-2.0
//! Complete final registry heads without a record-area pointer.

pub(crate) fn prt() -> Vec<u8> {
    let mut payload = Vec::new();
    for tail in [
        &[0x78, 0x01][..],
        &[0x80, 0xd0, 0x80, 0x00][..],
        &[0xa0, 0x37, 0xde, 0xa0, 0x00, 0x01][..],
        &[0x83, 0x87, 0x02][..],
    ] {
        let mut section = vec![0; 16];
        section[..4].fill(0xff);
        section[12..16].copy_from_slice(b"OM\x01\x01");
        append(&mut section, b"UGS::OM::Meta", &[0x02]);
        section.push(0x01);
        append(
            &mut section,
            b"UGS::OM::RootObject",
            &[0x38, 0x00, 16, 17, 18, 19, 20, 21, 22, 23, 0x01],
        );
        append(
            &mut section,
            b"UGS::OM::ChildObject",
            &[0x80, 0xc9, 0x01, 32, 33, 34, 35, 36, 37, 38, 39, 0x01],
        );
        section.push(0x02);
        append(&mut section, b"m_final", tail);
        // This complete-looking plain declaration has no independent lane bound.
        append(&mut section, b"first_record_area", &[0x80, 0xcf, 0x02]);
        let length = u32::try_from(section.len() - 16).expect("small registry fixture");
        section[8..12].copy_from_slice(&length.to_be_bytes());
        payload.extend(section);
    }
    super::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)])
}

fn append(bytes: &mut Vec<u8>, name: &[u8], tail: &[u8]) {
    bytes.push(u8::try_from(name.len() + 1).expect("small registry name"));
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(tail);
}
