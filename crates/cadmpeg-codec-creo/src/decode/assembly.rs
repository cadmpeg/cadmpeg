// SPDX-License-Identifier: Apache-2.0
//! Component occurrences from the `NeuAsmSld` component table
//! ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#9-assembly-component-table)).

use std::collections::BTreeSet;

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{ExternalDocument, Occurrence, OccurrenceParent, PrototypeReference};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::assembly::{Member, MemberTable};
use crate::container::ContainerScan;
use crate::loss::CreoLossCode;

use super::native::annotate;
use super::source_carriers::SourceUnitCarriers;

/// Whether the `#UGC:2` header line declares an assembly model.
fn declares_assembly(version_line: &str) -> bool {
    version_line.split_ascii_whitespace().nth(1) == Some("ASSEMBLY")
}

/// Place every admitted component row as an occurrence of its child model
/// file. Rows go under the root occurrence when `root` names one.
pub(super) fn transfer_assembly_members(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    transfer_losses: &mut Vec<LossNote>,
    source_carriers: &SourceUnitCarriers,
    root: Option<&OccurrenceId>,
) -> Result<(), CodecError> {
    let Some(table) = scan.assembly.as_ref() else {
        if declares_assembly(&scan.framing.version_line) {
            transfer_losses.push(CreoLossCode::AssemblyMemberTableIncomplete.note(
                "the header declares an assembly, but no NeuAsmSld component table was read; \
                 no component occurrence transferred",
            ));
        }
        return Ok(());
    };
    record_table_stop(table, transfer_losses);
    let parent = root.map_or(OccurrenceParent::Root {}, |occurrence| {
        OccurrenceParent::Occurrence {
            occurrence: occurrence.clone(),
        }
    });
    let mut external_documents = BTreeSet::new();
    let mut external_occurrences = 0_usize;
    let mut unresolved_placements = 0_usize;
    let mut unresolved_references = 0_usize;
    let mut transferred = 0_usize;
    for (row, member) in table.members.iter().enumerate() {
        let Some(transform) = member_transform(&member.frame) else {
            unresolved_placements += 1;
            continue;
        };
        let prototype = match member_document(member) {
            Some(path) => {
                external_documents.insert(path.clone());
                external_occurrences += 1;
                PrototypeReference::External {
                    document: ExternalDocument::path(path),
                    object: None,
                }
            }
            None => {
                unresolved_references += 1;
                PrototypeReference::Unresolved {}
            }
        };
        let id = crate::identity::compose_checked::<OccurrenceId>(
            &crate::identity::MODEL_OCCURRENCE,
            format_args!("member-{row}"),
        )?;
        annotate(
            annotations,
            &id,
            "NeuAsmSld",
            u64_from_index(member.offset),
            "assembly_member",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo assembly member occurrences")?;
        source_carriers.admit_occurrence(
            ir,
            Occurrence {
                id,
                prototype,
                parent: parent.clone(),
                ordinal: u32::try_from(row)
                    .map_err(|_| CodecError::malformed("Creo component row index exceeds u32"))?,
                transform,
                linked_prototype: None,
                scale: [FiniteReal::ONE; 3],
                name: Some(member.name.clone()),
                visible: None,
                link: None,
                native_ref: Some(format!("NeuAsmSld:members:{}", member.id)),
            },
        )?;
        transferred += 1;
    }
    if external_occurrences > 0 {
        transfer_losses.push(CreoLossCode::AssemblyComponentsExternal.note(format!(
            "{external_occurrences} component occurrence(s) reference {} external model file(s); \
             the referenced files are not opened",
            external_documents.len()
        )));
    }
    if unresolved_placements > 0 {
        transfer_losses.push(CreoLossCode::AssemblyPlacementUnresolved.note(format!(
            "{unresolved_placements} component row(s) store a frame that is not a proper rigid \
             placement; those occurrences did not transfer"
        )));
    }
    if unresolved_references > 0 {
        transfer_losses.push(
            CreoLossCode::AssemblyMemberReferenceUnresolved.note(format!(
            "{unresolved_references} component row(s) have an unrecognized type or a family name \
             that differs from the model name; their occurrences have unresolved prototypes"
        )),
        );
    }
    if transferred > 0 {
        transfer_losses.push(
            CreoLossCode::AssemblyConstraintsNotTransferred.note(format!(
            "component placement constraints are not decoded; {transferred} occurrence(s) carry \
             their stored placements"
        )),
        );
    }
    Ok(())
}

fn record_table_stop(table: &MemberTable, transfer_losses: &mut Vec<LossNote>) {
    if let Some(stop) = &table.stop {
        transfer_losses.push(CreoLossCode::AssemblyMemberTableIncomplete.note(format!(
            "component table at offset {} declares {} row(s); {} row(s) decoded before offset {}: {}",
            table.offset,
            table.declared,
            table.members.len(),
            stop.offset,
            stop.reason
        )));
    }
}

/// The child model file a row names: the model name with the extension its
/// type selects. A family name that differs from the model name names a
/// family-table instance, which the row does not resolve to a file.
fn member_document(member: &Member) -> Option<String> {
    let extension = member.kind.extension()?;
    (member.name == member.family_name).then(|| format!("{}.{extension}", member.name))
}

/// The placement whose columns are the child axes and origin.
fn member_transform(frame: &[f64; 12]) -> Option<Transform> {
    let [x0, x1, x2, y0, y1, y2, z0, z1, z2, o0, o1, o2] = *frame;
    let transform = Transform::affine([[x0, y0, z0, o0], [x1, y1, z1, o1], [x2, y2, z2, o2]])?;
    transform.is_proper_rigid().then_some(transform)
}

#[cfg(test)]
mod tests;
