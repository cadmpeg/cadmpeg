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
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
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
/// The suffix a unit-system name appends to the system key.
const SYSTEM_NAME_SUFFIX: &str = "UnitSystemName";
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
fn ascii_at<'bytes>(
    ctx: &DecodeContext<'_>,
    bytes: &'bytes [u8],
    at: usize,
) -> Result<Option<(&'bytes str, usize)>, CodecError> {
    let Some((raw, end)) = (|| {
        let length = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
        if length > 256 {
            return None;
        }
        let start = at.checked_add(4)?;
        let end = start.checked_add(length)?;
        Some((bytes.get(start..end)?, end))
    })() else {
        return Ok(None);
    };
    if !ctx.all_by(
        raw,
        |byte| Ok(byte.is_ascii_graphic() || *byte == b' '),
        "scan F3D unit ASCII field",
    )? {
        return Ok(None);
    }
    Ok(ctx
        .validate_utf8(raw, "validate F3D unit UTF-8 field")?
        .ok()
        .map(|text| (text, end)))
}

/// Read the `u32` field at `at` and check it equals `expected`, returning the
/// offset past it.
fn expect_u32(bytes: &[u8], at: usize, expected: u32) -> Option<usize> {
    (View::u32_le_at(bytes, at)? == expected).then(|| at + 4)
}

/// Check that the four bytes at `at` are zero, returning the offset past them.
fn expect_zero_quad(bytes: &[u8], at: usize) -> Option<usize> {
    zeros_at::<4>(bytes, at).then_some(at + 4)
}

/// Read the record index out of one `01 + u32 index + six zero bytes` slot.
fn reference_at(bytes: &[u8], at: usize) -> Option<u32> {
    if bytes.get(at) != Some(&1) || !zeros_at::<6>(bytes, at.checked_add(5)?) {
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
fn unit_system<'bytes>(
    ctx: &DecodeContext<'_>,
    bytes: &'bytes [u8],
    at: usize,
) -> Result<Option<(&'bytes str, [u32; index_from_u32(UNIT_ENTRY_COUNT)])>, CodecError> {
    let Some((key, position)) = ascii_at(ctx, bytes, at)? else {
        return Ok(None);
    };
    let Some((_label, position)) = ascii_at(ctx, bytes, position)? else {
        return Ok(None);
    };
    if bytes.get(position) != Some(&1) {
        return Ok(None);
    }
    let Some((name, position)) = ascii_at(ctx, bytes, position + 1)? else {
        return Ok(None);
    };
    // The name is the key followed by a fixed suffix; the suffix comparison
    // reads at most the literal's length.
    if name.as_bytes().get(key.len()..) != Some(SYSTEM_NAME_SUFFIX.as_bytes())
        || !ctx.starts_with(name, key, "match F3D unit system name")?
    {
        return Ok(None);
    }
    let Some((namespace, position)) = ascii_at(ctx, bytes, position)? else {
        return Ok(None);
    };
    if namespace.as_bytes() != SYSTEM_NAMESPACE.as_bytes() {
        return Ok(None);
    }
    Ok(expect_zero_quad(bytes, position)
        .and_then(|position| references(bytes, position))
        .map(|entries| (key, entries)))
}

/// The stored length unit name whose UTF-16LE code units open `bytes`, when
/// exactly `count` units name it. The test reads at most one name's length.
fn length_unit_name(bytes: &[u8], count: usize) -> Option<&'static str> {
    LENGTH_UNIT_NAMES.into_iter().find(|name| {
        name.len() == count
            && name
                .bytes()
                .enumerate()
                .all(|(ordinal, byte)| bytes_at::<2>(bytes, ordinal * 2) == Some(&[byte, 0]))
    })
}

/// The property name and unit name of one unit-entry record. The record stores
/// a key, a label, byte `01`, the property name, the `NsCommonData` namespace,
/// four zero bytes, and the UTF-16 unit name.
fn unit_entry<'bytes>(
    ctx: &DecodeContext<'_>,
    bytes: &'bytes [u8],
    at: usize,
) -> Result<Option<(&'bytes str, &'static str)>, CodecError> {
    let Some((_key, position)) = ascii_at(ctx, bytes, at)? else {
        return Ok(None);
    };
    let Some((_label, position)) = ascii_at(ctx, bytes, position)? else {
        return Ok(None);
    };
    if bytes.get(position) != Some(&1) {
        return Ok(None);
    }
    let Some((property, position)) = ascii_at(ctx, bytes, position + 1)? else {
        return Ok(None);
    };
    let Some((namespace, position)) = ascii_at(ctx, bytes, position)? else {
        return Ok(None);
    };
    if namespace.as_bytes() != ENTRY_NAMESPACE.as_bytes() {
        return Ok(None);
    }
    let Some(position) = expect_zero_quad(bytes, position) else {
        return Ok(None);
    };
    let value = View::u32_le_at(bytes, position)
        .and_then(|count| usize::try_from(count).ok())
        .zip(bytes.get(position + 4..))
        .and_then(|(count, units)| length_unit_name(units, count));
    Ok(value.map(|value| (property, value)))
}

/// The offset of the unit-system reference count after the first `UnitSystems`
/// collection name at or after `position`. The name is the LP-ASCII string
/// followed by two zero bytes. Each byte the search visits is admitted once
/// before its test, so a scan that resumes after each result pays for the
/// bytes it reads and no more.
fn next_collection_count(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: usize,
) -> Result<Option<usize>, CodecError> {
    const PREFIX: &[u8; 17] = b"\x0b\x00\x00\x00UnitSystems\x00\x00";
    let mut cursor = position;
    loop {
        let Some(tail) = bytes.get(cursor..) else {
            return Ok(None);
        };
        let Some(relative) = ctx.position_by(
            tail,
            |byte| Ok(*byte == PREFIX[0]),
            "find F3D unit collection marker",
        )?
        else {
            return Ok(None);
        };
        // `relative` indexes `tail`, so the sum stays within `bytes.len()`.
        let at = cursor + relative;
        if bytes_at::<17>(bytes, at) == Some(PREFIX) {
            return Ok(Some(at + PREFIX.len()));
        }
        cursor = at + 1;
    }
}

/// The `Custom` system's `modelingLengthName` value among the six unit
/// systems `systems` names. Each referenced record index may carry several
/// headers; every one is offered to the record grammar until a value is found.
fn custom_length_unit(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    offsets: &IndexedRecordOffsets,
    systems: [u32; index_from_u32(UNIT_SYSTEM_COUNT)],
) -> Result<Option<&'static str>, CodecError> {
    for system in systems {
        let unit = ctx.find_map(
            offsets.offsets(system),
            |at| {
                let Some((key, entries)) = at
                    .checked_add(HEADER_LEN)
                    .map(|at| unit_system(ctx, bytes, at))
                    .transpose()?
                    .flatten()
                else {
                    return Ok(None);
                };
                if key.as_bytes() != CUSTOM_SYSTEM.as_bytes() {
                    return Ok(None);
                }
                for entry in entries {
                    let unit = ctx.find_map(
                        offsets.offsets(entry),
                        |at| {
                            let Some((property, value)) = at
                                .checked_add(HEADER_LEN)
                                .map(|at| unit_entry(ctx, bytes, at))
                                .transpose()?
                                .flatten()
                            else {
                                return Ok(None);
                            };
                            Ok((property.as_bytes() == MODELING_LENGTH_PROPERTY.as_bytes())
                                .then_some(value))
                        },
                        "scan F3D unit entry records",
                    )?;
                    if unit.is_some() {
                        return Ok(unit);
                    }
                }
                Ok(None)
            },
            "scan F3D unit system records",
        )?;
        if unit.is_some() {
            return Ok(unit);
        }
    }
    Ok(None)
}

/// The `Custom` system's `modelingLengthName` value, when one design
/// `BulkStream` carries a well-formed `UnitSystems` collection.
///
/// The collection is located by name rather than by offset, so every candidate
/// match is parsed and the first that yields the property wins. A value outside
/// the five stored length unit names is rejected: the search is a byte scan,
/// and the closed name set is what separates the collection from bytes that
/// merely read like one. The stream's indexed headers are indexed once, when
/// the first candidate names its six systems, and held under a scoped
/// reservation until the stream is done.
fn decode_modeling_length_unit(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<String>, CodecError> {
    let mut index = None;
    let mut position = 0;
    while let Some(count_at) = next_collection_count(ctx, bytes, position)? {
        position = count_at;
        let Some(systems) = references::<{ index_from_u32(UNIT_SYSTEM_COUNT) }>(bytes, count_at)
        else {
            continue;
        };
        let (offsets, _storage) = match &mut index {
            Some(index) => index,
            None => index.insert(IndexedRecordOffsets::build_scoped(ctx, bytes)?),
        };
        if let Some(unit) = custom_length_unit(ctx, bytes, offsets, systems)? {
            return Ok(Some(
                ctx.copy_retained_text(unit, "f3d document length unit")?,
            ));
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
    ctx.find_map(
        &scan.entries,
        |entry| {
            if !scan.is_design_stream(entry, ContainerRole::Bulkstream) {
                return Ok(None);
            }
            let Ok(bytes) = scan.entry_bytes(&entry.name) else {
                return Ok(None);
            };
            decode_modeling_length_unit(ctx, bytes)
        },
        "scan F3D unit design streams",
    )
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
                    && limit.additional == 1));
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
                    && limit.additional == 1));
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
                    && limit.additional == 1));
    }

    #[test]
    fn unit_collection_search_charges_each_visited_byte() {
        let mut bytes = vec![0x0b, 0x00];
        bytes.extend_from_slice(b"\x0b\x00\x00\x00UnitSystems\x00\x00");
        bytes.extend_from_slice(&[0x0b; 4]);
        let ctx = cadmpeg_test_support::service_decode_context();
        assert_eq!(
            super::next_collection_count(&ctx, &bytes, 0).unwrap(),
            Some(19)
        );
        assert_eq!(
            super::next_collection_count(&ctx, &bytes, 19).unwrap(),
            None
        );
        // The search admits the bytes up to and including the marker's first
        // byte, then stops.
        for skip in [0, 2] {
            let error = crate::test_support::resource_refusal_at(
                cadmpeg_core::decode::ResourceDimension::WorkUnits,
                "find F3D unit collection marker",
                skip,
                |ctx| super::next_collection_count(ctx, &bytes, 0),
            );
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "find F3D unit collection marker"
                    && limit.additional == 1)
            );
        }
    }

    #[test]
    fn unit_entry_reads_the_utf16_length_unit_name() {
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, "Length");
        lp_ascii(&mut bytes, "Length Label");
        bytes.push(1);
        lp_ascii(&mut bytes, "modelingLengthName");
        lp_ascii(&mut bytes, ENTRY_NAMESPACE);
        bytes.extend_from_slice(&[0; 4]);
        lp_utf16(&mut bytes, "inch");
        let ctx = cadmpeg_test_support::service_decode_context();
        assert_eq!(
            super::unit_entry(&ctx, &bytes, 0).unwrap(),
            Some(("modelingLengthName", "inch"))
        );
        let mut other = bytes.clone();
        let last = other.len() - 2;
        other[last] = b'k';
        assert_eq!(super::unit_entry(&ctx, &other, 0).unwrap(), None);
    }

    #[test]
    fn unit_ascii_utf8_validation_preserves_result_and_refusal() {
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, "Custom");
        let ctx = cadmpeg_test_support::service_decode_context();
        assert_eq!(
            super::ascii_at(&ctx, &bytes, 0).unwrap(),
            Some(("Custom", 10))
        );
        assert_eq!(super::ascii_at(&ctx, &[1, 0, 0, 0, 0xff], 0).unwrap(), None);
        assert_eq!(
            super::ascii_at(&ctx, &[0, 0, 0, 0], 0).unwrap(),
            Some(("", 4))
        );
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "validate F3D unit UTF-8 field",
            0,
            |ctx| super::ascii_at(ctx, &bytes, 0),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "validate F3D unit UTF-8 field"
                && limit.additional == 6)
        );
    }
}
