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
use crate::design::decode::meta::DesignPrimaryFrame;
use crate::layout::indexed_design_record_header as indexed_header;
use cadmpeg_core::decode::View;

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
fn ascii_at(bytes: &[u8], at: usize) -> Option<(&str, usize)> {
    let length = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
    if length > 256 {
        return None;
    }
    let start = at.checked_add(4)?;
    let end = start.checked_add(length)?;
    let raw = bytes.get(start..end)?;
    if !raw
        .iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        return None;
    }
    Some((std::str::from_utf8(raw).ok()?, end))
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
fn unit_system(bytes: &[u8], at: usize) -> Option<(&str, [u32; index_from_u32(UNIT_ENTRY_COUNT)])> {
    let (key, position) = ascii_at(bytes, at)?;
    let (_label, position) = ascii_at(bytes, position)?;
    (bytes.get(position) == Some(&1)).then_some(())?;
    let (name, position) = ascii_at(bytes, position + 1)?;
    (name.strip_prefix(key) == Some("UnitSystemName")).then_some(())?;
    let (namespace, position) = ascii_at(bytes, position)?;
    (namespace == SYSTEM_NAMESPACE).then_some(())?;
    let position = expect_zero_quad(bytes, position)?;
    Some((key, references(bytes, position)?))
}

/// The property name and unit name of one unit-entry record. The record stores
/// a key, a label, byte `01`, the property name, the `NsCommonData` namespace,
/// four zero bytes, and the UTF-16 unit name.
fn unit_entry(bytes: &[u8], at: usize) -> Option<(&str, &'static str)> {
    let (_key, position) = ascii_at(bytes, at)?;
    let (_label, position) = ascii_at(bytes, position)?;
    (bytes.get(position) == Some(&1)).then_some(())?;
    let (property, position) = ascii_at(bytes, position + 1)?;
    let (namespace, position) = ascii_at(bytes, position)?;
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
    let value = LENGTH_UNIT_NAMES.iter().copied().find(|name| {
        name.len() == count
            && raw
                .chunks_exact(2)
                .zip(name.as_bytes())
                .all(|(unit, byte)| unit == [*byte, 0])
    })?;
    Some((property, value))
}

/// Offsets of the unit-system reference count following each `UnitSystems`
/// collection name. The name is the LP-ASCII string followed by two zero bytes.
fn collection_counts(bytes: &[u8]) -> impl Iterator<Item = usize> + '_ {
    const PREFIX: &[u8] = b"\x0b\x00\x00\x00UnitSystems\x00\x00";
    [HEADER_LEN, HEADER_LEN + 4]
        .into_iter()
        .filter_map(|start| {
            bytes
                .get(start..start + PREFIX.len())
                .filter(|candidate| *candidate == PREFIX)
                .map(|_| start + PREFIX.len())
        })
}

/// The `Custom` system's `modelingLengthName` value, when one design
/// `BulkStream` carries a well-formed `UnitSystems` collection.
///
/// The collection name follows its indexed header. A value outside the five
/// stored length unit names is rejected.
fn decode_modeling_length_unit(bytes: &[u8], frames: &[DesignPrimaryFrame<'_>]) -> Option<String> {
    let payload = |frame: &DesignPrimaryFrame<'_>| bytes.get(frame.start..frame.member_end);
    for collection in frames {
        let Some(collection) = payload(collection) else {
            continue;
        };
        for count_at in collection_counts(collection) {
            let Some(systems) =
                references::<{ index_from_u32(UNIT_SYSTEM_COUNT) }>(collection, count_at)
            else {
                continue;
            };
            for system in systems {
                for system_frame in frames
                    .iter()
                    .filter(|frame| frame.entity_id == u64::from(system))
                {
                    let Some(system_bytes) = payload(system_frame) else {
                        continue;
                    };
                    let Some((key, entries)) = unit_system(system_bytes, HEADER_LEN) else {
                        continue;
                    };
                    if key != CUSTOM_SYSTEM {
                        continue;
                    }
                    for entry in &entries {
                        for entry_frame in frames
                            .iter()
                            .filter(|frame| frame.entity_id == u64::from(*entry))
                        {
                            let Some(entry_bytes) = payload(entry_frame) else {
                                continue;
                            };
                            let Some((property, value)) = unit_entry(entry_bytes, HEADER_LEN)
                            else {
                                continue;
                            };
                            if property == MODELING_LENGTH_PROPERTY {
                                return Some(value.to_owned());
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// The document's modelling length unit, read from the first design
/// `BulkStream` that carries it.
///
/// An entry whose bytes cannot be read is skipped rather than failing the
/// decode: the unit is presentation metadata, and no geometry depends on it.
pub(crate) fn decode_document_length_unit(scan: &ContainerScan) -> Option<String> {
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        if let (Ok(bytes), Ok(Some(meta))) = (
            scan.entry_bytes(&entry.name),
            crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry.name),
        ) {
            if let Ok(frames) = crate::design::decode::meta::design_primary_frames(bytes, &meta) {
                if let Some(unit) = decode_modeling_length_unit(bytes, &frames) {
                    return Some(unit);
                }
            }
        }
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{
        CUSTOM_SYSTEM, ENTRY_NAMESPACE, LENGTH_UNIT_NAMES, SYSTEM_NAMESPACE, UNIT_ENTRY_COUNT,
        UNIT_SYSTEM_COUNT,
    };
    use crate::test_support::{lp_ascii, lp_utf16};

    use super::decode_modeling_length_unit;

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
        out.extend_from_slice(b"256");
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
        fixture(lengths).0
    }

    pub(crate) fn stream_with_metastream(lengths: [&str; 6]) -> (Vec<u8>, Vec<u8>) {
        let (bulk, records) = fixture(lengths);
        let ids: Vec<_> = records.iter().map(|(id, _)| u64::from(*id)).collect();
        let offsets: Vec<_> = records
            .iter()
            .map(|(id, at)| (u64::from(*id), u64::try_from(*at).unwrap()))
            .collect();
        let meta = crate::test_support::streams_test::design_metastream_with_records(
            &[(
                "00000000-0000-0000-0000-000000000001",
                "",
                1,
                "Fusion",
                &ids,
            )],
            &offsets,
        );
        (bulk, meta)
    }

    fn fixture(lengths: [&str; 6]) -> (Vec<u8>, Vec<(u32, usize)>) {
        let mut out = Vec::new();
        let mut records = vec![(1, 0)];
        header(&mut out, 1);
        lp_ascii(&mut out, "UnitSystems");
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&UNIT_SYSTEM_COUNT.to_le_bytes());
        for slot in 0..SYSTEMS.len() {
            reference(&mut out, 10 + u32::try_from(slot).unwrap());
        }
        for (slot, key) in SYSTEMS.iter().enumerate() {
            records.push((10 + u32::try_from(slot).unwrap(), out.len()));
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
                records.push((entry_base(slot) + u32::try_from(offset).unwrap(), out.len()));
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
        (out, records)
    }

    fn read_unit(bytes: &[u8], records: &[(u32, usize)]) -> Option<String> {
        let ty = crate::design::test_support::design_type(
            256,
            "00000000-0000-0000-0000-000000000001",
            None,
            1,
            "Fusion",
            Vec::new(),
        );
        let class_tag =
            crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap();
        let frames: Vec<_> = records
            .iter()
            .enumerate()
            .filter(|(_, (_, start))| *start < bytes.len())
            .map(|(ordinal, (id, start))| {
                let end = records
                    .get(ordinal + 1)
                    .map_or(bytes.len(), |(_, end)| (*end).min(bytes.len()));
                crate::design::decode::meta::DesignPrimaryFrame {
                    entity_id: u64::from(*id),
                    class_tag: class_tag.clone(),
                    start: *start,
                    member_end: end,
                    end,
                    design_type: &ty,
                }
            })
            .collect();
        decode_modeling_length_unit(bytes, &frames)
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
            let (bytes, records) =
                fixture(["centimeter", "millimeter", "meter", "inch", "foot", unit]);
            assert_eq!(
                read_unit(&bytes, &records).as_deref(),
                Some(unit),
                "expected the Custom system's {unit}"
            );
        }
    }

    #[test]
    fn preset_systems_do_not_supply_the_document_unit() {
        // Every preset holds a different fixed name; only `Custom` carries the
        // document's active setting.
        let (bytes, records) =
            fixture(["centimeter", "millimeter", "meter", "foot", "foot", "inch"]);
        assert_eq!(read_unit(&bytes, &records).as_deref(), Some("inch"));
    }

    #[test]
    fn a_name_outside_the_stored_set_is_rejected() {
        let (bytes, records) = fixture([
            "centimeter",
            "millimeter",
            "meter",
            "inch",
            "foot",
            "furlong",
        ]);
        assert_eq!(read_unit(&bytes, &records), None);
    }

    #[test]
    fn a_stream_without_the_collection_yields_no_unit() {
        let mut bytes = Vec::new();
        header(&mut bytes, 1);
        lp_ascii(&mut bytes, "BodiesRoot");
        assert_eq!(read_unit(&bytes, &[(1, 0)]), None);
    }

    #[test]
    fn collection_shaped_payload_does_not_create_a_collection() {
        let mut bytes = Vec::new();
        header(&mut bytes, 1);
        lp_ascii(&mut bytes, "BodiesRoot");
        bytes.extend_from_slice(&stream([
            "centimeter",
            "millimeter",
            "meter",
            "inch",
            "foot",
            "inch",
        ]));
        assert_eq!(read_unit(&bytes, &[(1, 0)]), None);
    }

    #[test]
    fn a_truncated_collection_yields_no_unit() {
        let (full, records) =
            fixture(["centimeter", "millimeter", "meter", "inch", "foot", "inch"]);
        let truncated = &full[..full.len() / 2];
        assert_eq!(read_unit(truncated, &records), None);
    }
}
