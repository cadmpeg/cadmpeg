// SPDX-License-Identifier: Apache-2.0
//! Shape payloads retain their genuine GUI property destination when rewritten.

use super::*;
use crate::native::{EntryRecord, GuiPropertyRecord};
use std::collections::{BTreeSet, HashSet};

pub(in crate::writer) fn write_property(
    ir: &CadIr,
    property: &mut GuiPropertyRecord,
    entries: &mut BTreeMap<String, Vec<u8>>,
    dispositions: &mut NativeDispositions,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    has_geometry: &mut bool,
    native_only: bool,
) -> Result<(BTreeSet<String>, HashSet<String>), CodecError> {
    let mut members = BTreeSet::new();
    let mut emitted = HashSet::new();
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok((members, emitted));
    };
    let payloads = crate::brep::records::payloads_untracked(namespace)?;
    let archive_entries = namespace.arena_as::<EntryRecord>("entries")?;
    let rows = crate::brep::records::polygons::polygon_rows(namespace, &payloads)?;
    for payload in payloads
        .iter()
        .filter(|payload| payload.property == property.id)
    {
        let file = archive_entries
            .iter()
            .find(|entry| entry.id() == payload.entry)
            .map(|entry| entry.name().to_owned())
            .ok_or_else(|| CodecError::malformed("GUI shape entry reference is absent"))?;
        let mut payload_has_geometry = false;
        let mut polygon_homes = HashMap::new();
        let mut bytes = if !native_only {
            if let Some(serialized) = super::super::serialize_payload(ir, payload)? {
                emitted.extend(serialized.meshes);
                polygon_homes = serialized.polygon_homes;
                serialized.bytes
            } else {
                if ir
                    .model
                    .bodies
                    .iter()
                    .any(|body| body.native_ref.as_deref() == Some(payload.id.as_str()))
                {
                    losses.push(crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!("GUI shape payload {} has a current body without an OCCT encoding and is omitted",payload.id)));
                }
                super::empty_carrier(payload)
            }
        } else {
            super::empty_carrier(payload)
        };
        for id in payload.polygon_references() {
            if !rows
                .iter()
                .any(|row| row.id == id && row.owner == payload.id)
            {
                losses.push(
                    crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                        "GUI B-rep payload {} omits missing polygon row {id}",
                        payload.id
                    )),
                );
            }
        }
        emitted.extend(merge::merge(
            ir,
            payload,
            &rows,
            &mut bytes,
            &polygon_homes,
            native_only,
            &mut payload_has_geometry,
            dispositions,
            losses,
        )?);
        entries.insert(file.clone(), bytes);
        *has_geometry |= payload_has_geometry;
        members.insert(file);
    }
    Ok((members, emitted))
}
