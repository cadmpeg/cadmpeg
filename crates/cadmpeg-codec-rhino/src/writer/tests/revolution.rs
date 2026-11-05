// SPDX-License-Identifier: Apache-2.0
//! Native revolution-surface class checksums.

use super::{RhinoArchiveVersion, RhinoCodec};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

pub(super) fn assert_revolution_checksum(
    bytes: &[u8],
    range: std::ops::Range<usize>,
    archive: crate::chunks::ArchiveVersion,
) {
    use crate::chunks::{
        chunk_at, verify_checksum, verify_checksum_ranges, BoundedReader, ChecksumStatus,
    };
    let header = crate::layout::long_chunk_header_wide::LEN;
    let chunk = chunk_at(bytes, range.start - header, bytes.len(), archive, false)
        .expect("bounded surface class data");
    assert_eq!(chunk.body(), range);
    let mut reader =
        BoundedReader::new(bytes, range.start, range.end).expect("bounded surface data");
    assert_eq!(reader.u8().unwrap(), 0x20);
    for _ in 0..16 {
        assert!(reader.f64().unwrap().is_finite());
    }
    assert!(matches!(reader.i32().unwrap(), 0 | 1));
    assert_eq!(reader.u8().unwrap(), 1);
    let child = reader.position();
    assert_eq!(
        verify_checksum_ranges(bytes, &chunk, std::iter::once(range.start..child)),
        Ok(ChecksumStatus::Valid)
    );
    assert_ne!(verify_checksum(bytes, &chunk), Ok(ChecksumStatus::Valid));
    let wrapper = chunk_at(bytes, child, range.end, archive, false).expect("profile class wrapper");
    assert_eq!(wrapper.next_offset(), range.end);
    let mut warnings = crate::loss::Diagnostics::new();
    crate::objects::parse_class_wrapper(bytes, wrapper.range(), archive, &mut warnings)
        .expect("valid embedded curve");
}

#[test]
fn standalone_revolution_checksum_excludes_its_nested_profile() {
    let source = include_bytes!("../../../tests/golden/fixtures/rev_surface.3dm");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("read revolution");
    for version in [RhinoArchiveVersion::V5, RhinoArchiveVersion::V8] {
        let mut bytes = Vec::new();
        RhinoCodec
            .plan(
                EncodeInput::new(decoded.ir(), None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("write revolution");
        let scan = crate::container::scan_owned(bytes).expect("read archive");
        let objects = scan
            .objects
            .iter()
            .filter_map(|object| object.framed())
            .filter(|object| object.class_uuid == crate::surfaces::REV_SURFACE)
            .collect::<Vec<_>>();
        assert_eq!(objects.len(), 1);
        assert_revolution_checksum(scan.data, objects[0].class_data_range.clone(), scan.archive);
    }
}

#[test]
fn standalone_revolution_charges_its_profile_record() {
    let source = include_bytes!("../../../tests/golden/fixtures/rev_surface.3dm");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("read revolution");
    let error = super::budgets::emit(decoded.ir(), 4, 0)
        .expect_err("the object, its profile, and three table records need five entities");
    assert!(matches!(
        error,
        cadmpeg_ir::codec::write::EncodeError::ResourceLimit(_)
    ));
    super::budgets::emit(decoded.ir(), 5, 0).expect("all five records fit");
}

pub(super) fn assert_brep_revolution_checksum(bytes: &[u8]) {
    let scan = crate::container::scan_owned(bytes.to_vec()).expect("read archive");
    let brep_object = scan
        .objects
        .iter()
        .filter_map(|object| object.framed())
        .find(|object| object.class_uuid == crate::brep::ON_BREP)
        .expect("Brep object");
    let ctx = cadmpeg_test_support::service_decode_context();
    let crate::brep::BrepParse::Valid(brep) = crate::brep::parse(
        &ctx,
        scan.data,
        brep_object.class_data_range.clone(),
        scan.archive,
        Some(super::super::OPENNURBS_WRITER_VERSION),
        &[],
    )
    .expect("read Brep") else {
        panic!("valid Brep")
    };
    let surfaces = brep
        .raw()
        .surfaces
        .slots
        .iter()
        .flatten()
        .filter(|surface| surface.class_uuid == crate::surfaces::REV_SURFACE)
        .collect::<Vec<_>>();
    assert_eq!(surfaces.len(), 1);
    assert_revolution_checksum(
        scan.data,
        surfaces[0].class_data_range.clone(),
        scan.archive,
    );
}
