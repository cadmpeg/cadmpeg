// SPDX-License-Identifier: Apache-2.0
//! IGES level numbers and definition-level sets as neutral layer membership.

use super::{
    BTreeMap, BTreeSet, CadIr, CodecError, DecodeContext, DirectoryEntry, IgesLossCode,
    ParameterRecord,
};
use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};

fn projection_loss(
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    entry: &DirectoryEntry,
    reason: &str,
) {
    losses.push(
        IgesLossCode::NeutralProjectionUnavailable
            .note(format!(
                "iges:entity:directory#{}: {reason}",
                entry.sequence
            ))
            .with_provenance(entry.loss_provenance()),
    );
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    records: &BTreeMap<u32, &ParameterRecord>,
    sequences: &super::super::geometry::SourceSequences,
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<BTreeSet<u32>, CodecError> {
    let mut definitions = BTreeMap::new();
    let mut layers: BTreeMap<i64, Vec<PresentationItem>> = BTreeMap::new();
    let mut decoded = BTreeSet::new();
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 406 && entry.form == 1)
    {
        let levels = records.get(&entry.sequence).and_then(|record| {
            let count = record.count(1)?;
            (0..count)
                .map(|index| record.integer(index + 2).filter(|level| *level >= 0))
                .collect::<Option<BTreeSet<_>>>()
        });
        if let Some(levels) = levels {
            for level in &levels {
                layers.entry(*level).or_default();
            }
            definitions.insert(entry.sequence, levels);
            decoded.insert(entry.sequence);
        } else {
            projection_loss(
                losses,
                entry,
                "definition-level list is not a complete nonnegative level set",
            );
        }
    }

    let mut items: BTreeMap<u32, Vec<PresentationItem>> = BTreeMap::new();
    for body in &ir.model.bodies {
        if let Some(sequence) = sequences.body(&body.id) {
            items
                .entry(sequence)
                .or_default()
                .push(PresentationItem::Body {
                    body: body.id.clone(),
                });
        }
    }
    for face in &ir.model.faces {
        if let Some(sequence) = sequences.face(&face.id) {
            items
                .entry(sequence)
                .or_default()
                .push(PresentationItem::Face {
                    face: face.id.clone(),
                });
        }
    }
    for curve in &ir.model.curves {
        if let Some(sequence) = sequences.curve(&curve.id) {
            items
                .entry(sequence)
                .or_default()
                .push(PresentationItem::Curve {
                    curve: curve.id.clone(),
                });
        }
    }
    for surface in &ir.model.surfaces {
        if let Some(sequence) = sequences.surface(&surface.id) {
            items
                .entry(sequence)
                .or_default()
                .push(PresentationItem::Surface {
                    surface: surface.id.clone(),
                });
        }
    }
    for point in &ir.model.points {
        if let Some(sequence) = sequences.point(&point.id) {
            items
                .entry(sequence)
                .or_default()
                .push(PresentationItem::Point {
                    point: point.id.clone(),
                });
        }
    }
    for entry in directory {
        let Some(level) = entry.level else { continue };
        let assigned = if level >= 0 {
            BTreeSet::from([level])
        } else if let Some(levels) = level
            .checked_neg()
            .and_then(|pointer| u32::try_from(pointer).ok())
            .and_then(|pointer| definitions.get(&pointer))
        {
            levels.clone()
        } else {
            projection_loss(
                losses,
                entry,
                "directory level pointer has no valid definition-level set",
            );
            continue;
        };
        let Some(members) = items.get(&entry.sequence) else {
            continue;
        };
        for level in assigned {
            layers
                .entry(level)
                .or_default()
                .extend(members.iter().cloned());
        }
    }
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(layers.len()),
        "iges_presentation_layers",
    )?;
    for (level, items) in layers {
        ir.model.presentation_layers.push(PresentationLayer {
            id: crate::ids::layer(&crate::ids::Stem::number(level)),
            name: level.to_string(),
            parent: None,
            description: None,
            visible: None,
            items,
        });
    }
    Ok(decoded)
}
