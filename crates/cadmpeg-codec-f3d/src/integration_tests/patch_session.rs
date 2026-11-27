// SPDX-License-Identifier: Apache-2.0
//! A patch write reads its retained archive in the one session of the encode.

use std::io::{Cursor, Read, Write};

use cadmpeg_core::decode::{LimitScope, ResourceDimension, ResourceLimits};
use cadmpeg_ir::codec::write::{EncodeError, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::report::export::WritePath;
use cadmpeg_ir::{CadIr, SourceFidelity};
use zip::CompressionMethod;

use crate::test_support::act_links::f3d_with_act_component_links;
use crate::test_support::assert_patch_declined;
use crate::test_support::native_test::update_f3d_native;
use crate::F3dCodec;

const UNREAD_MEMBER: &str = "FusionAssetName[Active]/Unread.dat";

fn decode_within(source: &[u8], limits: ResourceLimits) -> Option<DecodeResult> {
    let options = DecodeOptions {
        policy: cadmpeg_core::decode::DecodePolicy { limits },
        ..DecodeOptions::default()
    };
    F3dCodec.decode(&mut Cursor::new(source), &options).ok()
}

fn with_decompressed_total(ceiling: u64) -> ResourceLimits {
    ResourceLimits {
        max_decompressed_bytes_total: ceiling,
        ..ResourceLimits::default()
    }
}

/// The document of `decoded` with its component link renamed, an edit that
/// the writer patches into the retained archive.
fn renamed_link(decoded: DecodeResult) -> (CadIr, SourceFidelity) {
    let (mut ir, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut ir, |native| {
        let link = &mut native.act_component_links[0];
        link.references.tracked_entity = 5;
        link.references.instance_root = 71;
        link.references.components_root = 72;
        link.try_set_strings("0_5".into(), "Renamed!!".into())
            .unwrap();
    });
    (ir, fidelity)
}

/// The least ceiling on the bytes expanded in all under which `source`
/// decodes.
fn least_decompressed_total(source: &[u8]) -> u64 {
    let (mut refused, mut admitted) = (0_u64, 1_u64 << 26);
    assert!(decode_within(source, with_decompressed_total(refused)).is_none());
    assert!(decode_within(source, with_decompressed_total(admitted)).is_some());
    while admitted - refused > 1 {
        let ceiling = refused + (admitted - refused) / 2;
        if decode_within(source, with_decompressed_total(ceiling)).is_some() {
            admitted = ceiling;
        } else {
            refused = ceiling;
        }
    }
    admitted
}

/// The inflated size the central directory of `archive` declares for its
/// members in all.
fn declared_member_total(archive: &[u8]) -> u64 {
    let mut archive = zip::ZipArchive::new(Cursor::new(archive)).unwrap();
    (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().size())
        .sum()
}

#[test]
fn a_patch_refuses_when_its_source_decode_and_member_expansions_pass_the_ceiling_together() {
    let source = deflated_source();
    let ceiling = least_decompressed_total(&source).max(declared_member_total(&source));
    let limits = with_decompressed_total(ceiling);
    let (ir, fidelity) = renamed_link(decode_within(&source, limits).unwrap());

    let error = F3dCodec
        .plan(EncodeInput {
            limits,
            ..EncodeInput::new(&ir, Some(&fidelity))
        })
        .unwrap_err();

    let EncodeError::ResourceLimit(limit) = error else {
        panic!("expected a resource refusal: {error}")
    };
    assert_eq!(limit.scope, LimitScope::Session);
    assert_eq!(limit.dimension, ResourceDimension::DecompressedBytes);
    assert_eq!(limit.limit, ceiling);
}

#[test]
fn a_patch_writes_when_the_ceiling_holds_its_source_decode_and_member_expansions() {
    let source = deflated_source();
    let ceiling = 2 * (least_decompressed_total(&source) + declared_member_total(&source));
    let limits = with_decompressed_total(ceiling);
    let (ir, fidelity) = renamed_link(decode_within(&source, limits).unwrap());

    let plan = F3dCodec
        .plan(EncodeInput {
            limits,
            ..EncodeInput::new(&ir, Some(&fidelity))
        })
        .unwrap();

    assert!(matches!(
        plan.report().write_path(),
        WritePath::Patched { .. }
    ));
}

/// A writer that holds the members of `archive`, each stored with `method`.
fn repacked(archive: &[u8], method: CompressionMethod) -> zip::ZipWriter<Cursor<Vec<u8>>> {
    let mut source = zip::ZipArchive::new(Cursor::new(archive)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = crate::zip_write::file_options(method);
    for index in 0..source.len() {
        let mut entry = source.by_index(index).unwrap();
        let mut payload = Vec::new();
        entry.read_to_end(&mut payload).unwrap();
        out.start_file(entry.name().unwrap(), options).unwrap();
        out.write_all(&payload).unwrap();
    }
    out
}

/// The component-link source with every member compressed, so that its
/// decode and the patch each expand members.
fn deflated_source() -> Vec<u8> {
    repacked(
        &f3d_with_act_component_links(&[(4, 0)]),
        CompressionMethod::Deflated,
    )
    .finish()
    .unwrap()
    .into_inner()
}

/// `archive` with a Deflate member that the decoder does not read, whose
/// stored CRC-32 its data does not match.
fn with_corrupt_unread_member(archive: &[u8]) -> Vec<u8> {
    let mut out = repacked(archive, CompressionMethod::Stored);
    let deflated = crate::zip_write::file_options(CompressionMethod::Deflated);
    out.start_file(UNREAD_MEMBER, deflated).unwrap();
    out.write_all(&[0x5a; 4096]).unwrap();
    let mut bytes = out.finish().unwrap().into_inner();
    let name = UNREAD_MEMBER.as_bytes();
    for (signature, crc, name_length, name_start) in
        [(b"PK\x03\x04", 14, 26, 30), (b"PK\x01\x02", 16, 28, 46)]
    {
        let header = (0..bytes.len() - name_start)
            .find(|&offset| {
                let length = usize::from(u16::from_le_bytes([
                    bytes[offset + name_length],
                    bytes[offset + name_length + 1],
                ]));
                bytes[offset..offset + 4] == *signature
                    && bytes.get(offset + name_start..offset + name_start + length) == Some(name)
            })
            .unwrap();
        for byte in &mut bytes[header + crc..header + crc + 4] {
            *byte = !*byte;
        }
    }
    bytes
}

#[test]
fn a_member_that_does_not_inflate_declines_the_patch() {
    let source = with_corrupt_unread_member(&f3d_with_act_component_links(&[(4, 0)]));
    let (ir, fidelity) = renamed_link(decode_within(&source, ResourceLimits::default()).unwrap());

    assert_patch_declined(&ir, Some(&fidelity), &format!("ZIP entry {UNREAD_MEMBER}"));
}

/// `archive` with a Deflate member of `size` bytes that the decoder does not
/// read.
fn with_unread_member(archive: &[u8], size: usize) -> Vec<u8> {
    let mut out = repacked(archive, CompressionMethod::Stored);
    let deflated = crate::zip_write::file_options(CompressionMethod::Deflated);
    out.start_file(UNREAD_MEMBER, deflated).unwrap();
    out.write_all(&vec![0x5a; size]).unwrap();
    out.finish().unwrap().into_inner()
}

/// The largest inflated size the central directory of `archive` declares
/// for one member.
fn largest_member(archive: &[u8]) -> u64 {
    let mut archive = zip::ZipArchive::new(Cursor::new(archive)).unwrap();
    (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().size())
        .max()
        .unwrap()
}

/// A member that declares more bytes than one expansion of the session
/// admits is a member the patch cannot rewrite: the patch declines, and the
/// session stays open for the synthesis.
#[test]
fn a_member_past_the_per_expansion_allowance_declines_the_patch() {
    let base = f3d_with_act_component_links(&[(4, 0)]);
    let allowance = largest_member(&base);
    let source = with_unread_member(&base, usize::try_from(allowance).unwrap() + 1);
    let limits = ResourceLimits {
        max_decompressed_bytes_per_expand: allowance,
        ..ResourceLimits::default()
    };
    let (ir, fidelity) = renamed_link(decode_within(&source, limits).unwrap());

    let plan = F3dCodec
        .plan(EncodeInput {
            limits,
            ..EncodeInput::new(&ir, Some(&fidelity))
        })
        .unwrap();

    let Some(cadmpeg_ir::report::export::SynthesisCause::PatchDeclined { reason }) =
        plan.report().synthesis_cause()
    else {
        panic!("the patch declines: {:?}", plan.report().write_path());
    };
    assert!(
        reason.contains(&format!("ZIP entry {UNREAD_MEMBER} declares"))
            && reason.contains("-byte per-expansion allowance"),
        "{reason}"
    );
}
