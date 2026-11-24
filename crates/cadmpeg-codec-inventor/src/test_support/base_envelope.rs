// SPDX-License-Identifier: Apache-2.0
//! Synthetic Base-envelope design records for crate tests.

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;

use crate::layout::{kernel_entity_link as link_layout, pm_dc_content_header as header};
use crate::test_support::test_fixtures::{
    acis_sphere_kernel_stream_with_body_key, base_envelope_fixture, content, push_u16, push_u32,
    push_utf16, stored_kernel_body_fixture, BaseEnvelopeRecords,
};
use crate::InventorCodec;

/// Source index of both Base features of the fixture.
pub(crate) const SOURCE_INDEX: u32 = 100;
/// Entity key of the result body of the fixture.
pub(crate) const RESULT_KEY: i64 = 7;

pub(crate) fn list(bytes: &mut Vec<u8>, references: &[u32]) {
    push_u16(bytes, 2);
    push_u16(bytes, 0x3000);
    push_u32(
        bytes,
        u32::try_from(references.len()).expect("synthetic count"),
    );
    if !references.is_empty() {
        push_u32(
            bytes,
            u32::try_from(references.len()).expect("synthetic count"),
        );
        push_u32(bytes, 0);
        for &reference in references {
            push_u32(bytes, reference);
        }
    }
}

pub(crate) fn label(owner: u32, members: &[u32], base: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 0);
    push_u16(&mut bytes, 0);
    for value in [0, 0, owner, 0, 0] {
        push_u32(&mut bytes, value);
    }
    push_u32(&mut bytes, owner);
    list(&mut bytes, members);
    push_utf16(&mut bytes, "");
    let class = if base {
        "2d11a90cd0118b83000819b00524dc09"
    } else {
        "00000000000000000000000000000000"
    };
    for digits in class.as_bytes().chunks_exact(2) {
        let text = std::str::from_utf8(digits).expect("synthetic class digits");
        bytes.push(u8::from_str_radix(text, 16).expect("synthetic class byte"));
    }
    bytes
}

pub(crate) type SyntheticRecords = Vec<([u8; 16], Vec<u8>)>;

pub(crate) fn fixtures() -> (SyntheticRecords, BaseEnvelopeRecords) {
    let mut root = Vec::new();
    push_u32(&mut root, 0);
    push_u16(&mut root, 1);
    push_u32(&mut root, 0);
    push_u32(&mut root, 0x40);
    root.extend_from_slice(&[0x31; 16]);
    push_utf16(&mut root, "");
    list(&mut root, &[]);
    for index in 0..15 {
        push_u32(&mut root, u32::from(index == 12));
    }
    push_utf16(&mut root, "PmBRepSegment");
    root.extend_from_slice(&1.0_f64.to_le_bytes());
    push_u32(&mut root, 0);
    let mut base = content(100);
    push_u32(&mut base, 0);
    push_u32(&mut base, 0);
    list(&mut base, &[0x8000_0003, 0x8000_0004, 0]);
    push_u32(&mut base, 0);
    let mut input = content(101);
    push_u32(&mut input, 0);
    let mut output = content(102);
    push_u32(&mut output, 1);
    let mut second = base.clone();
    second[header::FLAGS..header::FLAGS + 4].copy_from_slice(&4_u32.to_le_bytes());
    let design = vec![
        (crate::design::scope::MODEL_ROOT_TYPE, root),
        (crate::pmdc::inventor_id(0x9087_4d91), base),
        (crate::pmdc::inventor_id(0x9087_4d47), input),
        (crate::pmdc::inventor_id(0x9087_4d47), output),
        (
            [
                0x2b, 0xa4, 0x48, 0x2b, 0xd2, 0x11, 0x58, 0x64, 0x60, 0x00, 0x74, 0xb7, 0x9b, 0x49,
                0xeb, 0xb0,
            ],
            label(1, &[7, 2], false),
        ),
        (
            [
                0x2b, 0xa4, 0x48, 0x2b, 0xd2, 0x11, 0x58, 0x64, 0x60, 0x00, 0x74, 0xb7, 0x9b, 0x49,
                0xeb, 0xb0,
            ],
            label(2, &[], true),
        ),
        (crate::pmdc::inventor_id(0x9087_4d91), second),
        (
            [
                0x2b, 0xa4, 0x48, 0x2b, 0xd2, 0x11, 0x58, 0x64, 0x60, 0x00, 0x74, 0xb7, 0x9b, 0x49,
                0xeb, 0xb0,
            ],
            label(7, &[], true),
        ),
    ];
    let mut link = Vec::new();
    push_u32(&mut link, 0);
    push_u16(&mut link, 77);
    push_u32(&mut link, 102);
    link.push(0);
    push_u32(&mut link, 0x8000_0001);
    push_u32(&mut link, 7);
    push_u32(&mut link, 42);
    let mut pairs = vec![0x5a; 16];
    pairs.extend_from_slice(&[0x41; 16]);
    let mut revisions = vec![0x42; 16];
    push_u16(&mut revisions, 0);
    push_u16(&mut revisions, 1);
    let mut references = Vec::new();
    push_u32(&mut references, 0);
    push_u16(&mut references, 0);
    push_u16(&mut references, 77);
    (
        design,
        BaseEnvelopeRecords {
            design_version: 18,
            brep: vec![(
                [
                    0x21, 0x75, 0x0f, 0xcc, 0xd1, 0x11, 0x27, 0x80, 0xe3, 0x86, 0x19, 0x96, 0x22,
                    0x59, 0x01, 0x7a,
                ],
                link,
            )],
            pairs,
            revisions,
            references,
        },
    )
}

pub(crate) fn decode(
    design: &[([u8; 16], Vec<u8>)],
    base: &BaseEnvelopeRecords,
) -> cadmpeg_ir::CadIr {
    let bytes = base_envelope_fixture(
        &acis_sphere_kernel_stream_with_body_key(21_800, 7),
        design,
        base,
    );
    InventorCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthetic Base decode")
        .into_parts()
        .0
}

pub(crate) fn kind(hex: &str) -> [u8; 16] {
    let bytes: Vec<u8> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).expect("hex pair"), 16).expect("hex byte")
        })
        .collect();
    bytes.try_into().expect("sixteen-byte identifier")
}

pub(crate) fn derived(mirror: bool) -> (SyntheticRecords, BaseEnvelopeRecords) {
    let (mut design, mut base) = fixtures();
    base.design_version = 19;
    let mut feature = content(100);
    push_u32(&mut feature, 0);
    push_u32(&mut feature, 0);
    list(&mut feature, &[0x8000_0003, 0x8000_0004, 0x8000_0009]);
    push_u32(&mut feature, 0);
    design[1].1 = feature;
    let label = &mut design[5].1;
    let len = label.len();
    label[len - 16..].copy_from_slice(&kind("eecb298e2c45650820104eb55d062c80"));
    let mut source = content(110);
    push_u32(&mut source, 0x8000_000a);
    design.push((kind("3cc048210042420ba62103b20b7f4891"), source));
    let mut component = content(111);
    for value in [
        u32::MAX,
        0x8000_000b,
        0x8000_000c,
        0x8000_000d,
        0x8000_000e,
        0x3000_0004,
        0,
        0x8000_000f,
    ] {
        push_u32(&mut component, value);
    }
    design.push((kind("a034363fd31191b5c0000385e89c6b4f"), component));
    design.push((kind("9f34363fd31191b5c0000385e89c6b4f"), content(114)));
    design.push((kind("604d8790d011f8d10008cabc0663dc09"), content(115)));
    let mut transform = content(112);
    push_u16(&mut transform, 0x8421);
    push_u16(&mut transform, if mirror { 0x7bdf } else { 0x7bde });
    design.push((kind("184d8790d011f8d10008cabc0663dc09"), transform));
    design.push((kind("d77e5d25d311f2b5c0000385e89c6b4f"), content(116)));
    design.push((kind("a7fbcb1db64f16cfe979019eae94a4df"), content(117)));
    design[2].1[header::LEN..header::LEN + 4].copy_from_slice(&0x8000_0010_u32.to_le_bytes());
    let mut link = base.brep[0].1.clone();
    link[link_layout::HEADER_ID..link_layout::HEADER_ID + 2].copy_from_slice(&88_u16.to_le_bytes());
    link[link_layout::SELECTION_SOURCE..link_layout::SELECTION_SOURCE + 4]
        .copy_from_slice(&101_u32.to_le_bytes());
    link[link_layout::BODY_REFERENCE..link_layout::BODY_REFERENCE + 4]
        .copy_from_slice(&0x8000_0011_u32.to_le_bytes());
    link[link_layout::ENTITY_ID..link_layout::ENTITY_ID + 4].copy_from_slice(&9_u32.to_le_bytes());
    design.push((base.brep[0].0, link));
    design.push((
        crate::kernel::KERNEL_RECORD_TYPE_ID,
        stored_kernel_body_fixture(
            &acis_sphere_kernel_stream_with_body_key(21_800, 9),
            0x8000_0011,
        ),
    ));
    let mut tagging = content(120);
    push_u32(&mut tagging, 0);
    push_u32(&mut tagging, 0);
    list(&mut tagging, &[0x8000_0012]);
    push_u32(&mut tagging, 0);
    design[6].1 = tagging;
    let label = &mut design[7].1;
    let len = label.len();
    label[len - 16..].copy_from_slice(&kind("e899dbdfd31165afc0000185e89c6b4f"));
    let mut collection = content(121);
    list(&mut collection, &[0x8000_0004]);
    design.push((kind("ae70680ed14a1e86d76248b03c2a96e1"), collection));
    (design, base)
}

pub(crate) fn derived_at_version(
    mirror: bool,
    version: u8,
) -> (SyntheticRecords, BaseEnvelopeRecords) {
    let (mut design, mut base) = derived(mirror);
    base.design_version = version;
    if version >= 23 {
        let stamps = if version >= 25 { 2 } else { 1 };
        for ordinal in [2, 3, 8, 12, 17] {
            for _ in 0..stamps {
                design[ordinal]
                    .1
                    .splice(header::LEN..header::LEN, (-1_i32).to_le_bytes());
            }
        }
        push_u32(
            &mut design[9].1,
            crate::layout::pm_dc_derived_component_suffix::STATE_VALUE,
        );
        let footer = design[16].1.len() - 2 * std::mem::size_of::<u32>();
        design[16].1.insert(footer, 0);
    }
    (design, base)
}

pub(crate) fn decoded(
    design: &SyntheticRecords,
    base: &BaseEnvelopeRecords,
) -> cadmpeg_ir::codec::DecodeResult {
    let bytes = base_envelope_fixture(
        &acis_sphere_kernel_stream_with_body_key(21_800, 7),
        design,
        base,
    );
    crate::InventorCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("derived decode")
}

pub(crate) fn design_records() -> SyntheticRecords {
    fixtures().0
}

pub(crate) fn base_records() -> BaseEnvelopeRecords {
    fixtures().1
}

/// The model of the state that introduces the result body: the decode of the
/// fixture holds one body with the result key.
pub(crate) fn state_model() -> CadIr {
    decode(&design_records(), &base_records())
}
