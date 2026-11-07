// SPDX-License-Identifier: Apache-2.0
//! Datum tags and their bounded leader references.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{PmiId, SurfaceId};
use cadmpeg_ir::pmi::{PmiAnnotation, PmiDefinition, PmiTarget};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::container::ContainerScan;
use crate::feature::pmi_datums::{datum_tags, DatumTagOutcome};
use crate::loss::CreoLossCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DatumIndex {
    Unique(usize),
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DatumJoin {
    Existing(usize),
    New,
    Separate,
}

fn datum_join(
    label_roster_complete: bool,
    tag_label_count: usize,
    datum_indices: &BTreeMap<String, DatumIndex>,
    identification: &str,
) -> DatumJoin {
    if !label_roster_complete || tag_label_count != 1 {
        return DatumJoin::Separate;
    }
    match datum_indices.get(identification) {
        Some(DatumIndex::Unique(index)) => DatumJoin::Existing(*index),
        Some(DatumIndex::Ambiguous) => DatumJoin::Separate,
        None => DatumJoin::New,
    }
}

#[cfg(test)]
mod tests;

pub(in crate::decode) fn transfer(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut sections = Vec::new();
    for name in ["BasFullData", "FullMData"] {
        let Some(section) = super::section(scan, name) else {
            continue;
        };
        sections.push((name, datum_tags(ctx, section)?));
    }

    let label_roster_complete = sections
        .iter()
        .all(|(_, outcome)| outcome.label_roster_complete());
    let mut tag_label_counts = BTreeMap::<String, usize>::new();
    for (_, outcome) in &sections {
        let DatumTagOutcome::Decoded { tags, .. } = outcome else {
            continue;
        };
        for tag in tags {
            *tag_label_counts
                .entry(tag.identification.clone())
                .or_default() += 1;
        }
    }

    let mut datum_indices = BTreeMap::<String, DatumIndex>::new();
    for (index, item) in ir.model.pmi.iter().enumerate() {
        if let PmiDefinition::Datum { identification } = &item.definition {
            datum_indices
                .entry(identification.clone())
                .and_modify(|value| *value = DatumIndex::Ambiguous)
                .or_insert(DatumIndex::Unique(index));
        }
    }

    let mut decoded_tags = 0usize;
    let mut definition_slot_joins = 0usize;
    let mut inferred_targets = 0usize;
    let mut unresolved_targets = 0usize;
    let mut unresolved_label_joins = 0usize;
    for (name, outcome) in sections {
        match outcome {
            DatumTagOutcome::AbsentOrEmpty => {}
            DatumTagOutcome::Unresolved { populated_slots } => {
                let message = match populated_slots {
                    Some(count) => format!(
                        "The datum-tag array has {count} populated slot(s) with unresolved contents."
                    ),
                    None => "The populated datum-tag array could not be bounded.".to_owned(),
                };
                losses.push(CreoLossCode::PmiDatumTargetUnresolved.note(message));
            }
            DatumTagOutcome::Decoded {
                tags,
                unresolved_populated_slots,
                ..
            } => {
                if unresolved_populated_slots != 0 {
                    losses.push(CreoLossCode::PmiDatumTargetUnresolved.note(format!(
                        "{unresolved_populated_slots} populated datum-tag slot(s) have unresolved definition joins."
                    )));
                }
                for tag in &tags {
                    definition_slot_joins += usize::from(tag.definition_slot.is_some());
                    let target = if tag.definition_id.is_some() && tag.definition_slot.is_some() {
                        tag.attachment
                            .filter(|attachment| attachment.reference_type == 0x25)
                            .and_then(|attachment| {
                                let surface = crate::identity::compose_checked::<SurfaceId>(
                                    &crate::identity::VISIBGEOM_SURFACE,
                                    attachment.id,
                                )
                                .ok()?;
                                let mut faces =
                                    ir.model.faces.iter().filter(|face| face.surface == surface);
                                let (Some(face), None) = (faces.next(), faces.next()) else {
                                    return None;
                                };
                                Some(PmiTarget::Face {
                                    face: face.id.clone(),
                                })
                            })
                    } else {
                        None
                    };
                    let join = datum_join(
                        label_roster_complete,
                        tag_label_counts
                            .get(&tag.identification)
                            .copied()
                            .unwrap_or_default(),
                        &datum_indices,
                        &tag.identification,
                    );
                    unresolved_label_joins += usize::from(join == DatumJoin::Separate);
                    let id = if let DatumJoin::Existing(index) = join {
                        let item = &mut ir.model.pmi[index];
                        if let Some(target) = target.clone() {
                            if !item.targets.contains(&target) {
                                item.targets.push(target);
                            }
                        }
                        item.id.clone()
                    } else {
                        let id = PmiId::try_from(format!(
                            "creo:annotation:pmi#tag:{name}:{}",
                            tag.source_offset
                        ))
                        .map_err(|_| CodecError::malformed("invalid Creo datum-tag identity"))?;
                        ctx.charge_entities(1, "Creo datum tag")?;
                        let index = ir.model.pmi.len();
                        datum_indices
                            .entry(tag.identification.clone())
                            .and_modify(|value| *value = DatumIndex::Ambiguous)
                            .or_insert(DatumIndex::Unique(index));
                        ir.model.pmi.push(PmiAnnotation {
                            id: id.clone(),
                            name: None,
                            visible: None,
                            targets: target.clone().into_iter().collect(),
                            definition: PmiDefinition::Datum {
                                identification: tag.identification.clone(),
                            },
                        });
                        id
                    };
                    super::super::native::annotate(
                        annotations,
                        &id,
                        name,
                        cadmpeg_core::decode::u64_from_index(tag.source_offset),
                        "datum tag",
                        Exactness::Inferred,
                    )?;
                    if target.is_some() {
                        inferred_targets += 1;
                    } else {
                        unresolved_targets += 1;
                    }
                    decoded_tags += 1;
                }
            }
        }
    }
    if decoded_tags != 0 {
        losses.push(CreoLossCode::PmiDatumTagBoundaryAssumed.note(format!(
            "{decoded_tags} datum-tag row(s) use a class-qualified prototype boundary."
        )));
    }
    if definition_slot_joins != 0 {
        losses.push(CreoLossCode::PmiDatumDefinitionSlotAssumed.note(format!(
            "{definition_slot_joins} datum-tag definition-slot join(s) use a candidate class-bounded row sequence."
        )));
    }
    if inferred_targets != 0 {
        losses.push(CreoLossCode::PmiDatumTargetInferred.note(format!(
            "{inferred_targets} datum leader(s) use uniquely bounded surfaces as datum-feature targets."
        )));
    }
    if unresolved_targets != 0 {
        losses.push(CreoLossCode::PmiDatumTargetUnresolved.note(format!(
            "{unresolved_targets} decoded datum-tag row(s) have unresolved targets."
        )));
    }
    if unresolved_label_joins != 0 {
        losses.push(CreoLossCode::PmiDatumTargetUnresolved.note(format!(
            "{unresolved_label_joins} datum-tag label join(s) are ambiguous."
        )));
    }
    ir.model.pmi.sort_unstable_by(|a, b| a.id.cmp(&b.id));
    Ok(())
}
