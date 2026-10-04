// SPDX-License-Identifier: Apache-2.0
//! Read the document's modelling length unit from the Design `UnitSystems`
//! collection.
//!
//! The collection names six unit systems; five are presets with fixed
//! `ModelingLength` names and the sixth, `Custom`, holds the document's active
//! settings. Its `ModelingLength` entry carries the display length unit under
//! the property name `modelingLengthName`.

use cadmpeg_core::decode::index_from_u32;

use cadmpeg_core::container::ContainerRole;

use crate::container::ContainerScan;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::layout::indexed_design_record_header as indexed_header;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

/// An indexed-record header: `u32 3`, three class-tag digits, `u32 index`.
const HEADER_LEN: usize = indexed_header::LEN;
/// One reference slot: `01`, a `u32` record index, and six zero bytes.
const REFERENCE_LEN: usize = 11;
/// The collection names six unit systems.
const UNIT_SYSTEM_COUNT: u32 = 6;
/// Seventeen quantity families plus `ModelingLength` and `ModelingMass`.
const UNIT_ENTRY_COUNT: u32 = 19;
/// The system holding the document's active settings.
const CUSTOM_SYSTEM: &str = "Custom";
/// The property name of the `Custom` system's `ModelingLength` entry.
const MODELING_LENGTH_PROPERTY: &str = "modelingLengthName";
/// The namespace every unit-system record stores.
const SYSTEM_NAMESPACE: &str = "NaFusion";
/// The namespace every unit-entry record stores.
const ENTRY_NAMESPACE: &str = "NsCommonData";
/// The length unit names the `ModelingLength` entry takes.
const LENGTH_UNIT_NAMES: [&str; 5] = ["millimeter", "centimeter", "meter", "inch", "foot"];

/// Read one LP-ASCII field, returning it with the offset past its payload.
///
/// A stored key, name, or namespace is graphic ASCII; a label is display text,
/// so the space is admissible alongside it.
fn ascii_at<'bytes>(ctx: &DecodeContext<'_>, bytes: &'bytes [u8], at: usize)
    -> Result<Option<(&'bytes str, usize)>, CodecError> {
    let Some((raw, end)) = (|| {
        let length = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
        if length > 256 { return None; }
        let start = at.checked_add(4)?;
        let end = start.checked_add(length)?;
        Some((bytes.get(start..end)?, end))
    })() else { return Ok(None); };
    if !ctx.admit_iter(raw, "scan F3D unit ASCII field")?
        .all(|byte| byte.is_ascii_graphic() || *byte == b' ') { return Ok(None); }
    Ok(ctx.validate_utf8(raw, "validate F3D unit UTF-8 field")?.ok().map(|text| (text, end)))
}

/// Read the `u32` field at `at` and check it equals `expected`, returning the
/// offset past it.
fn expect_u32(bytes: &[u8], at: usize, expected: u32) -> Option<usize> {
    (View::u32_le_at(bytes, at)? == expected).then(|| at + 4)
}

/// Check that the four bytes at `at` are zero, returning the offset past them.
fn expect_zero_quad(bytes: &[u8], at: usize) -> Option<usize> {
    (bytes.get(at..at.checked_add(4)?)? == [0u8; 4]).then_some(at + 4)
}

/// Read the record index out of one `01 + u32 index + six zero bytes` slot.
fn reference_at(bytes: &[u8], at: usize) -> Option<u32> {
    if bytes.get(at) != Some(&1)
        || bytes.get(at.checked_add(5)?..at.checked_add(REFERENCE_LEN)?)? != [0u8; 6]
    {
        return None;
    }
    View::u32_le_at(bytes, at + 1)
}

/// Read a `u32 expected` count followed by that many reference slots.
fn references<const N: usize>(bytes: &[u8], at: usize) -> Option<[u32; N]> {
    let mut position = expect_u32(bytes, at, u32::try_from(N).ok()?)?;
    let mut out = [0; N];
    for slot in &mut out {
        *slot = reference_at(bytes, position)?;
        position = position.checked_add(REFERENCE_LEN)?;
    }
    Some(out)
}

/// The payload of one unit-system record: its key and its unit-entry
/// references. The record stores the key, a label, byte `01`, the name
/// `<key>UnitSystemName`, the `NaFusion` namespace, four zero bytes, and the
/// counted entry references.
fn unit_system<'bytes>(ctx: &DecodeContext<'_>, bytes: &'bytes [u8], at: usize) -> Result<Option<(&'bytes str, [u32; index_from_u32(UNIT_ENTRY_COUNT)])>, CodecError> {
    (|| {
    let (key, position) = match ascii_at(ctx, bytes, at) { Ok(value) => value?, Err(error) => return Some(Err(error)) };
    let (_label, position) = match ascii_at(ctx, bytes, position) { Ok(value) => value?, Err(error) => return Some(Err(error)) };
    (bytes.get(position) == Some(&1)).then_some(())?;
    let (name, position) = match ascii_at(ctx, bytes, position + 1) { Ok(value) => value?, Err(error) => return Some(Err(error)) };
    (name.strip_prefix(key) == Some("UnitSystemName")).then_some(())?;
    let (namespace, position) = match ascii_at(ctx, bytes, position) { Ok(value) => value?, Err(error) => return Some(Err(error)) };
    (namespace == SYSTEM_NAMESPACE).then_some(())?;
    let position = expect_zero_quad(bytes, position)?;
    Some(Ok((key, references(bytes, position)?)))
    })().transpose()
}

/// The property name and unit name of one unit-entry record. The record stores
/// a key, a label, byte `01`, the property name, the `NsCommonData` namespace,
/// four zero bytes, and the UTF-16 unit name.
fn unit_entry<'bytes>(ctx: &DecodeContext<'_>, bytes: &'bytes [u8], at: usize) -> Result<Option<(&'bytes str, &'static str)>, CodecError> {
    (|| {
    let (_key, position) = match ascii_at(ctx, bytes, at) { Ok(value) => value?, Err(error) => return Some(Err(error)) };
    let (_label, position) = match ascii_at(ctx, bytes, position) { Ok(value) => value?, Err(error) => return Some(Err(error)) };
    (bytes.get(position) == Some(&1)).then_some(())?;
    let (property, position) = match ascii_at(ctx, bytes, position + 1) { Ok(value) => value?, Err(error) => return Some(Err(error)) };
    let (namespace, position) = match ascii_at(ctx, bytes, position) { Ok(value) => value?, Err(error) => return Some(Err(error)) };
    (namespace == ENTRY_NAMESPACE).then_some(())?;
    let position = expect_zero_quad(bytes, position)?;
    let count = usize::try_from(View::u32_le_at(bytes, position)?).ok()?;
    if count > 64 {
        return None;
    }
    let start = position.checked_add(4)?;
    let end = count
        .checked_mul(2)
        .and_then(|size| start.checked_add(size))?;
    let raw = bytes.get(start..end)?;
    let width = std::num::NonZeroUsize::new(2)?;
    let mut value = None;
    for name in LENGTH_UNIT_NAMES {
        if name.len() != count { continue; }
        let admitted = match ctx.admit_iter(raw, "match F3D unit UTF-16 name") {
            Ok(value) => value, Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
        };
        let expected = match ctx.admit_iter(name.as_bytes(), "scan F3D unit name literal bytes") {
            Ok(value) => value, Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
        };
        if admitted.chunks(width).zip(expected).all(|(unit, byte)| unit == [*byte, 0]) {
            value = Some(name); break;
        }
    }
    Some(Ok((property, value?)))
    })().transpose()
}

/// Offsets of the unit-system reference count following each `UnitSystems`
/// collection name. The name is the LP-ASCII string followed by two zero bytes.
fn collection_counts<'bytes>(ctx: &DecodeContext<'_>, bytes: &'bytes [u8])
    -> Result<impl Iterator<Item = usize> + 'bytes, CodecError> {
    const PREFIX: &[u8] = b"\x0b\x00\x00\x00UnitSystems\x00\x00";
    let width = std::num::NonZeroUsize::new(PREFIX.len())
        .ok_or_else(|| CodecError::malformed("F3D unit collection marker is empty"))?;
    Ok(ctx.admit_iter(bytes, "scan F3D unit collection markers")?.windows(width)
        .enumerate().filter_map(|(start, marker)| (marker == PREFIX).then_some(start + PREFIX.len())))
}

/// The `Custom` system's `modelingLengthName` value, when one design
/// `BulkStream` carries a well-formed `UnitSystems` collection.
///
/// The collection is located by name rather than by offset, so every candidate
/// match is parsed and the first that yields the property wins. A value outside
/// the five stored length unit names is rejected: the search is a byte-window
/// scan, and the closed name set is what separates the collection from a window
/// that merely reads like one.
fn decode_modeling_length_unit(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<String>, CodecError> {
    let offsets = IndexedRecordOffsets::build(ctx, bytes)?;
    let payloads = |record_index: u32| {
        Ok::<_, CodecError>(ctx.admit_iter(offsets.offsets(record_index), "scan F3D unit record payloads")?
            .filter_map(|at| at.checked_add(HEADER_LEN)))
    };
    for count_at in collection_counts(ctx, bytes)? {
        let Some(systems) = references::<{ index_from_u32(UNIT_SYSTEM_COUNT) }>(bytes, count_at)
        else {
            continue;
        };
        for system in ctx.admit_iter(&systems, "scan F3D unit systems")? {
            for system_at in payloads(*system)? {
                let Some((key, entries)) = unit_system(ctx, bytes, system_at)? else {
                    continue;
                };
                if key != CUSTOM_SYSTEM {
                    continue;
                }
                for entry in ctx.admit_iter(&entries, "scan F3D unit entries")? {
                    for entry_at in payloads(*entry)? {
                        let Some((property, value)) = unit_entry(ctx, bytes, entry_at)? else {
                            continue;
                        };
                        if property == MODELING_LENGTH_PROPERTY {
                            let value = ctx.copy_retained_text(value, "f3d document length unit")?;
                            match ctx.validate_utf8(
                                value.as_bytes(),
                                "validate F3D document length unit",
                            )? {
                                Ok(_) => return Ok(Some(value)),
                                Err(_) => {
                                    return Err(CodecError::malformed(
                                        "validated length unit is not UTF-8",
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(None)
}

/// The document's modelling length unit, read from the first design
/// `BulkStream` that carries it.
///
/// An entry whose bytes cannot be read is skipped rather than failing the
/// decode: the unit is presentation metadata, and no geometry depends on it.
pub(crate) fn decode_document_length_unit(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Option<String>, CodecError> {
    for entry in ctx.admit_iter(&scan.entries, "scan F3D unit design streams")?
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        if let Ok(bytes) = scan.entry_bytes(&entry.name) {
            if let Some(unit) = decode_modeling_length_unit(ctx, bytes)? {
                return Ok(Some(unit));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{
        CUSTOM_SYSTEM, ENTRY_NAMESPACE, LENGTH_UNIT_NAMES, SYSTEM_NAMESPACE, UNIT_ENTRY_COUNT,
        UNIT_SYSTEM_COUNT,
    };
    use crate::test_support::{lp_ascii, lp_utf16};

    fn decode_modeling_length_unit(bytes: &[u8]) -> Option<String> {
        crate::design::test_support::with_test_decode_context(|ctx| {
            super::decode_modeling_length_unit(ctx, bytes).unwrap()
        })
    }

    /// The six systems in collection order.
    const SYSTEMS: [&str; 6] = [
        "CmMKS",
        "MmMKS",
        "MMKS",
        "InchImperial",
        "Imperial",
        CUSTOM_SYSTEM,
    ];
    /// The nineteen unit entries a system stores, `ModelingLength` last but one.
    const ENTRIES: [&str; 19] = [
        "Length",
        "Mass",
        "Time",
        "Temperature",
        "Speed",
        "Volume",
        "Pressure",
        "Force",
        "Power",
        "Energy",
        "Current",
        "Substance",
        "Luminosity",
        "Angle",
        "Currency",
        "Percentage",
        "Pieces",
        "ModelingLength",
        "ModelingMass",
    ];

    fn header(out: &mut Vec<u8>, record_index: u32) {
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(b"001");
        out.extend_from_slice(&record_index.to_le_bytes());
    }

    fn reference(out: &mut Vec<u8>, record_index: u32) {
        out.push(1);
        out.extend_from_slice(&record_index.to_le_bytes());
        out.extend_from_slice(&[0u8; 6]);
    }

    /// The first entry record index belonging to system `slot`.
    fn entry_base(slot: usize) -> u32 {
        100 + u32::try_from(slot).unwrap() * 19
    }

    /// A design stream carrying the collection, its six systems, and every
    /// system's nineteen unit entries. `lengths[slot]` is the `ModelingLength`
    /// name that system stores.
    pub(crate) fn stream(lengths: [&str; 6]) -> Vec<u8> {
        let mut out = Vec::new();
        header(&mut out, 1);
        lp_ascii(&mut out, "UnitSystems");
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&UNIT_SYSTEM_COUNT.to_le_bytes());
        for slot in 0..SYSTEMS.len() {
            reference(&mut out, 10 + u32::try_from(slot).unwrap());
        }
        for (slot, key) in SYSTEMS.iter().enumerate() {
            header(&mut out, 10 + u32::try_from(slot).unwrap());
            lp_ascii(&mut out, key);
            lp_ascii(&mut out, &format!("{key} label"));
            out.push(1);
            lp_ascii(&mut out, &format!("{key}UnitSystemName"));
            lp_ascii(&mut out, SYSTEM_NAMESPACE);
            out.extend_from_slice(&[0u8; 4]);
            out.extend_from_slice(&UNIT_ENTRY_COUNT.to_le_bytes());
            for offset in 0..ENTRIES.len() {
                reference(&mut out, entry_base(slot) + u32::try_from(offset).unwrap());
            }
        }
        for (slot, length) in lengths.iter().enumerate() {
            for (offset, entry) in ENTRIES.iter().enumerate() {
                header(&mut out, entry_base(slot) + u32::try_from(offset).unwrap());
                lp_ascii(&mut out, entry);
                lp_ascii(&mut out, &format!("{entry} label"));
                out.push(1);
                lp_ascii(&mut out, &format!("{}Name", lower_camel(entry)));
                lp_ascii(&mut out, ENTRY_NAMESPACE);
                out.extend_from_slice(&[0u8; 4]);
                lp_utf16(
                    &mut out,
                    if *entry == "ModelingLength" {
                        length
                    } else {
                        "unit"
                    },
                );
            }
        }
        out
    }

    fn lower_camel(value: &str) -> String {
        let mut chars = value.chars();
        chars
            .next()
            .map(|first| first.to_ascii_lowercase().to_string() + chars.as_str())
            .unwrap_or_default()
    }

    #[test]
    fn reads_every_stored_length_unit_name_from_the_custom_system() {
        for unit in LENGTH_UNIT_NAMES {
            let bytes = stream(["centimeter", "millimeter", "meter", "inch", "foot", unit]);
            assert_eq!(
                decode_modeling_length_unit(&bytes).as_deref(),
                Some(unit),
                "expected the Custom system's {unit}"
            );
        }
    }

    #[test]
    fn document_length_unit_utf8_validation_refuses_work() {
        let bytes = stream(["centimeter", "millimeter", "meter", "inch", "foot", "inch"]);
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "validate F3D document length unit",
            0,
            |ctx| super::decode_modeling_length_unit(ctx, &bytes).map(|_| ()),
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                    && limit.operation == "validate F3D document length unit"
                    && limit.additional == 4
        ));
    }

    #[test]
    fn preset_systems_do_not_supply_the_document_unit() {
        // Every preset holds a different fixed name; only `Custom` carries the
        // document's active setting.
        let bytes = stream(["centimeter", "millimeter", "meter", "foot", "foot", "inch"]);
        assert_eq!(decode_modeling_length_unit(&bytes).as_deref(), Some("inch"));
    }

    #[test]
    fn a_name_outside_the_stored_set_is_rejected() {
        let bytes = stream([
            "centimeter",
            "millimeter",
            "meter",
            "inch",
            "foot",
            "furlong",
        ]);
        assert_eq!(decode_modeling_length_unit(&bytes), None);
    }

    #[test]
    fn a_stream_without_the_collection_yields_no_unit() {
        let mut bytes = Vec::new();
        header(&mut bytes, 1);
        lp_ascii(&mut bytes, "BodiesRoot");
        assert_eq!(decode_modeling_length_unit(&bytes), None);
    }

    #[test]
    fn a_truncated_collection_yields_no_unit() {
        let full = stream(["centimeter", "millimeter", "meter", "inch", "foot", "inch"]);
        let truncated = &full[..full.len() / 2];
        assert_eq!(decode_modeling_length_unit(truncated), None);
    }
    fn unit_text_refusal(unit: &str) {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        let bytes = stream(["centimeter", "millimeter", "meter", "inch", "foot", unit]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = u64::try_from(unit.len() - 1).unwrap();
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "f3d document length unit",
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match ResourceDimension::RetainedBytes {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
                (super::decode_modeling_length_unit(&ctx, &bytes)).map(|_| ())
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match ResourceDimension::RetainedBytes {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        assert!(matches!(super::decode_modeling_length_unit(&ctx, &bytes),
            Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::RetainedBytes
                && failure.operation == "f3d document length unit"
                && failure.additional == u64::try_from(unit.len()).unwrap()));
    }
    #[test]
    fn millimeter_unit_refuses_retained_limit() {
        unit_text_refusal("millimeter");
    }
    #[test]
    fn centimeter_unit_refuses_retained_limit() {
        unit_text_refusal("centimeter");
    }
    #[test]
    fn meter_unit_refuses_retained_limit() {
        unit_text_refusal("meter");
    }
    #[test]
    fn inch_unit_refuses_retained_limit() {
        unit_text_refusal("inch");
    }
    #[test]
    fn foot_unit_refuses_retained_limit() {
        unit_text_refusal("foot");
    }
    #[test]
    fn unit_ascii_field_refuses_work_before_validation() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, "Custom");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(super::ascii_at(&ctx, &bytes, 0),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "scan F3D unit ASCII field"
                    && limit.additional == 6));
    }

    #[test]
    fn unit_system_parser_preserves_ascii_work_refusal() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, "Custom");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(super::unit_system(&ctx, &bytes, 0),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "scan F3D unit ASCII field"
                    && limit.additional == 6));
    }

    #[test]
    fn unit_entry_parser_preserves_ascii_work_refusal() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, "ModelingLength");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(super::unit_entry(&ctx, &bytes, 0),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "scan F3D unit ASCII field"
                    && limit.additional == 14));
    }

    #[test]
    fn unit_collection_search_refuses_work_before_marker_scan() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let bytes = b"\x0b\x00\x00\x00UnitSystems\x00\x00";
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(super::collection_counts(&ctx, bytes),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "scan F3D unit collection markers"
                    && limit.additional == u64::try_from(bytes.len()).unwrap()));
    }

    #[test]
    fn unit_name_literal_iterator_refusal_propagates() {
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, "Length");
        lp_ascii(&mut bytes, "Length Label");
        bytes.push(1);
        lp_ascii(&mut bytes, "modelingLengthName");
        lp_ascii(&mut bytes, ENTRY_NAMESPACE);
        bytes.extend_from_slice(&[0; 4]);
        lp_utf16(&mut bytes, "inch");
        let ctx = cadmpeg_test_support::service_decode_context();
        assert_eq!(super::unit_entry(&ctx, &bytes, 0).unwrap(), Some(("modelingLengthName", "inch")));
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "scan F3D unit name literal bytes", 0,
            |ctx| super::unit_entry(ctx, &bytes, 0),
        );
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "scan F3D unit name literal bytes"
                && limit.additional == 4));
    }

    #[test]
    fn unit_ascii_utf8_validation_preserves_result_and_refusal() {
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, "Custom");
        let ctx = cadmpeg_test_support::service_decode_context();
        assert_eq!(super::ascii_at(&ctx, &bytes, 0).unwrap(), Some(("Custom", 10)));
        assert_eq!(super::ascii_at(&ctx, &[1, 0, 0, 0, 0xff], 0).unwrap(), None);
        assert_eq!(super::ascii_at(&ctx, &[0, 0, 0, 0], 0).unwrap(), Some(("", 4)));
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "validate F3D unit UTF-8 field", 0,
            |ctx| super::ascii_at(ctx, &bytes, 0),
        );
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "validate F3D unit UTF-8 field"
                && limit.additional == 6));
    }

}
