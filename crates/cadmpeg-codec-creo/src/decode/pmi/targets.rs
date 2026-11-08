// SPDX-License-Identifier: Apache-2.0
//! Geometric targets selected by typed tolerance placements.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::pmi::PmiTarget;

use crate::container::ContainerScan;
use crate::feature::pmi_framing::{prefixes_for_annotation_slots, AttachmentPrefix};

use super::{elements::Element, Tolerance};

const DISPLAY_VALUE_MATCH_TOLERANCE: f64 = 1.0e-9;

type Owner = (u32, u32);

pub(super) fn flatness_targets(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    owners: &BTreeMap<Owner, Option<(Element, &str)>>,
    tolerances: &BTreeMap<Owner, Tolerance<'_>>,
    ir: &CadIr,
) -> Result<BTreeMap<Owner, PmiTarget>, CodecError> {
    let mut targets = BTreeMap::new();
    for name in ["BasFullData", "FullMData"] {
        let Some(section) = super::section(scan, name) else {
            continue;
        };
        let matching = || {
            owners.iter().filter_map(|(owner, element)| {
                let (element, section_name) = element.as_ref()?;
                (*section_name == name).then_some((*owner, element.annotation?))
            })
        };
        let count = matching().count();
        let mut slots = ctx.alloc_filled(count, 0_u32, "Creo tolerance annotation slots")?;
        for (slot, (_, annotation)) in slots.iter_mut().zip(matching()) {
            *slot = annotation;
        }
        let Some(prefixes) = prefixes_for_annotation_slots(ctx, section, &slots)? else {
            continue;
        };
        for ((owner, _), prefix) in matching().zip(prefixes) {
            let Some(prefix) = prefix else { continue };
            let Some(tolerance) = tolerances.get(&owner) else {
                continue;
            };
            if tolerance.kind != Some(b"FLATNESS") || prefix.tolerance_type != 2 {
                continue;
            }
            let (Some(stored), Some(displayed)) = (
                tolerance.magnitude,
                super::text_magnitude(prefix.value_string),
            ) else {
                continue;
            };
            if (stored - displayed).abs() > DISPLAY_VALUE_MATCH_TOLERANCE {
                continue;
            }
            let Some(placement) = prefix.placement else {
                continue;
            };
            let Some(AttachmentPrefix::Inline {
                reference_type: 0x25,
                id,
                ..
            }) = placement.attachment
            else {
                continue;
            };
            if placement.placement_type != 2 || placement.id != id {
                continue;
            }
            let Ok(surface) = crate::identity::compose_checked::<SurfaceId>(
                &crate::identity::VISIBGEOM_SURFACE,
                id,
            ) else {
                continue;
            };
            let mut faces = ir.model.faces.iter().filter(|face| face.surface == surface);
            if let (Some(face), None) = (faces.next(), faces.next()) {
                targets.insert(
                    owner,
                    PmiTarget::Face {
                        face: face.id.clone(),
                    },
                );
            }
        }
    }
    Ok(targets)
}
