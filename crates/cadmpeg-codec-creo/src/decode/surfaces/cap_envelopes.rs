// SPDX-License-Identifier: Apache-2.0
//! Preserve cap-envelope bodies whose chart and scalar roles are unresolved.

use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::container::ContainerScan;
use crate::layout::type24_cap_envelope_control_prefix as cap;
use crate::scalar::{self, ScalarCache};
use crate::surface::{
    SurfaceKind, SurfaceParameterCarrier, SurfaceParameterOpaqueSpan, SurfaceRow,
};

use super::super::sketch_transfer::recipe::FeatureSchemaClasses;
use super::super::uniqueness::UniqueById;

const CAP_ENVELOPE_SCHEMA_CLASS: u32 = 932;
const CAP_SUPPORT_IMAGES: [[u8; 7]; 4] = [
    [0x0f, 0x18, 0xe5, 0x10, 0x18, 0xe5, 0x10],
    [0x18, 0x0f, 0x18, 0x0f, 0x18, 0xe6, 0x10],
    [0x10, 0x18, 0xe5, 0x0f, 0x18, 0xe5, 0x10],
    [0x18, 0x10, 0x18, 0x10, 0x18, 0xe6, 0x10],
];

fn complete_cap_envelope_body(body: &[u8]) -> Option<()> {
    let cache = ScalarCache::default();
    let next_scalar = |cursor| {
        scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, &cache).map(|(_, end)| end)
    };
    (body.get(cap::SEPARATOR) == Some(&cap::SEPARATOR_VALUE)).then_some(())?;
    for (start, end) in [
        (cap::FIRST_CONTROL, cap::SEPARATOR),
        (cap::SECOND_CONTROL, cap::THIRD_CONTROL),
        (cap::THIRD_CONTROL, cap::LEN),
    ] {
        (next_scalar(start) == Some(end)).then_some(())?;
    }
    let mut cursor = cap::LEN;
    for _ in 0..6 {
        cursor = next_scalar(cursor)?;
    }
    if cursor == body.len() {
        return Some(());
    }
    (body.get(cursor) == Some(&0xe3)).then_some(())?;
    cursor += 1;
    let support = body.get(cursor..cursor.checked_add(7)?)?;
    CAP_SUPPORT_IMAGES
        .contains(&support.try_into().ok()?)
        .then_some(())?;
    cursor += support.len();
    for field in 0..4 {
        if field == 0 && body.get(cursor) == Some(&0x0d) {
            cursor += 1;
        } else if field == 1 && body.get(cursor..cursor.checked_add(3)?) == Some(&[0, 4, 0xdc]) {
            cursor += 3;
        } else if field == 3 && body.get(cursor..) == Some(&[0x18]) {
            cursor += 1;
        } else {
            cursor = next_scalar(cursor)?;
        }
    }
    (cursor == body.len()).then_some(())
}

pub(in crate::decode) fn withhold_unresolved_cap_envelopes(
    ctx: &DecodeContext<'_>,
    scan: &mut ContainerScan<'_>,
) -> Result<(), CodecError> {
    let classes = FeatureSchemaClasses::new(scan);
    let owned = |rows: &[SurfaceRow]| {
        let unique = UniqueById::new(rows, |row| row.id);
        rows.iter()
            .filter(|row| {
                row.kind == SurfaceKind::Cylinder
                    && unique.get(row.id).is_some()
                    && classes
                        .get(row.feature_id)
                        .map(crate::feature::schema::SchemaClass::code)
                        == Some(CAP_ENVELOPE_SCHEMA_CLASS)
            })
            .map(|row| row.id)
            .collect::<BTreeSet<_>>()
    };
    let visible = owned(&scan.surfaces.rows);
    let nonvisible = owned(&scan.surfaces.nonvisible_rows);
    let cross_section = owned(&scan.surfaces.cross_section_rows);
    drop(classes);
    let mut withheld_visible = BTreeSet::new();
    for (parameters, ids, visible_namespace) in [
        (&mut scan.surfaces.parameters, visible, true),
        (&mut scan.surfaces.nonvisible_parameters, nonvisible, false),
        (
            &mut scan.surfaces.cross_section_parameters,
            cross_section,
            false,
        ),
    ] {
        for record in parameters {
            if !ids.contains(&record.surface_id)
                || complete_cap_envelope_body(&record.body).is_none()
            {
                continue;
            }
            ctx.retain_bytes(
                cadmpeg_core::decode::u64_from_index(record.body.len()),
                "retain unresolved cap-envelope body",
            )?;
            if visible_namespace {
                withheld_visible.insert(record.surface_id);
            }
            record.carrier = SurfaceParameterCarrier::Unresolved(SurfaceKind::Cylinder);
            record.scalar_tokens.clear();
            record.scalar_frames.clear();
            record.opaque_spans = vec![SurfaceParameterOpaqueSpan {
                raw: record.body.clone().into(),
                offset: 0,
            }];
        }
    }
    scan.surfaces.withheld_cap_curve_ids = scan
        .curves
        .topology_rows
        .iter()
        .filter(|row| {
            row.faces
                .iter()
                .flatten()
                .any(|face| withheld_visible.contains(&face.get()))
        })
        .map(|row| row.id)
        .collect();
    scan.surfaces.withheld_cap_surface_ids = withheld_visible;
    Ok(())
}

#[cfg(test)]
mod tests;
