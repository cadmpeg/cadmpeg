// SPDX-License-Identifier: Apache-2.0
//! Component occurrences from the `NeuAsmSld` component table
//! ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#9-assembly-component-table)).

use std::collections::BTreeMap;

use cadmpeg_core::decode::{u64_from_index, ArenaClass, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{ExternalDocument, Occurrence, ProductDefinition, Storage};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::unknown::part::{DecodeParts, GivenUpPart, GivenUpRecord, Part};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::assembly::{Member, MemberTable};
use crate::container::ContainerScan;
use crate::loss::CreoLossCode;

use super::native::annotate;
use super::source_carriers::SourceUnitCarriers;

/// Whether the `#UGC:2` header line declares an assembly model.
pub(super) fn declares_assembly(version_line: &str) -> bool {
    version_line.split_ascii_whitespace().nth(1) == Some("ASSEMBLY")
}

/// Place every admitted component row in the root definition as an
/// occurrence of its child model file, a document the decode does not read.
pub(super) fn transfer_assembly_members(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    transfer_losses: &mut Vec<LossNote>,
    source_carriers: &SourceUnitCarriers,
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
    let root = crate::identity::compose_checked::<ProductDefinitionId>(
        &crate::identity::MODEL_PRODUCT_DEFINITION,
        "root",
    )?;
    if !ir
        .model
        .product_definitions
        .iter()
        .any(|row| row.id == root)
    {
        ctx.charge_entities(1, "admit Creo model product_definitions")?;
        ir.model.product_definitions.push(ProductDefinition::root(
            root.clone(),
            ExternalDocument::default(),
        ));
    }
    let mut external_documents = BTreeMap::<String, ProductDefinitionId>::new();
    let mut definitions = Vec::new();
    let mut owned = Vec::new();
    let mut external_occurrences = 0_usize;
    let mut unresolved_placements = Vec::new();
    let mut unresolved_references = 0_usize;
    let mut transferred = 0_usize;
    for (row, member) in table.members.iter().enumerate() {
        let id = crate::identity::compose_checked::<OccurrenceId>(
            &crate::identity::MODEL_OCCURRENCE,
            format_args!("member-{row}"),
        )?;
        let Some(transform) = member_transform(&member.frame) else {
            unresolved_placements.push(id);
            continue;
        };
        let document = member_document(member);
        let prototype = match document
            .as_ref()
            .and_then(|path| external_documents.get(path))
        {
            Some(definition) => definition.clone(),
            None => {
                let definition = crate::identity::compose_checked::<ProductDefinitionId>(
                    &crate::identity::MODEL_PRODUCT_DEFINITION,
                    format_args!("member-{row}"),
                )?;
                ctx.charge_entities(1, "admit Creo assembly member definitions")?;
                let mut external = ProductDefinition::new(
                    definition.clone(),
                    Storage::External(
                        document
                            .as_ref()
                            .map_or_else(ExternalDocument::default, ExternalDocument::path),
                    ),
                );
                external.source_name = Some(member.name.clone());
                definitions.push(external);
                if let Some(path) = &document {
                    external_documents.insert(path.clone(), definition.clone());
                }
                definition
            }
        };
        if document.is_some() {
            external_occurrences += 1;
        } else {
            unresolved_references += 1;
        }
        annotate(
            annotations,
            &id,
            "NeuAsmSld",
            u64_from_index(member.offset),
            "assembly_member",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo assembly member occurrences")?;
        let mut occurrence = Occurrence::new(id.clone(), prototype, Some(transform));
        occurrence.name = Some(member.name.clone());
        occurrence.native_ref = Some(member_record(member));
        source_carriers.admit_occurrence(ir, occurrence)?;
        owned.push(id);
        transferred += 1;
    }
    ir.model.product_definitions.extend(definitions);
    if let Some(root) = ir
        .model
        .product_definitions
        .iter_mut()
        .find(|row| row.id == root)
    {
        root.occurrences.extend(owned);
    }
    if external_occurrences > 0 {
        transfer_losses.push(CreoLossCode::AssemblyComponentsExternal.note(format!(
            "{external_occurrences} component occurrence(s) reference {} external model file(s); \
             the referenced files are not opened",
            external_documents.len()
        )));
    }
    if !unresolved_placements.is_empty() {
        transfer_losses.push(CreoLossCode::AssemblyPlacementUnresolved.note(format!(
            "{} component row(s) store a frame that is not a proper rigid \
             placement; those occurrences did not transfer",
            unresolved_placements.len()
        )));
        let code = CreoLossCode::AssemblyPlacementUnresolved.kind();
        transfer_losses.extend(unresolved_placements.iter().map(|occurrence| {
            GivenUpRecord::new(
                occurrence,
                format_args!("{code}: the row stores a frame that is not a proper rigid placement"),
            )
            .loss()
        }));
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
        let note = CreoLossCode::AssemblyMemberTableIncomplete.note(format!(
            "component table at offset {} declares {} row(s); {} row(s) decoded before offset {}: {}",
            table.offset,
            table.declared,
            table.members.len(),
            stop.offset,
            stop.reason
        ));
        transfer_losses.push(note);
    }
}

/// Gives up each component table of a `NeuAsmSld` section that the decode
/// reads and does not transfer, from its `members` field to the end of the
/// section, as a `component-table` part. For the transferred table, gives up
/// the rows from the first row the grammar does not admit to the end of the
/// section, as a `component-rows` part. The decode retains no other copy of
/// those bytes.
pub(super) fn give_up_unread_tables(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<GivenUpPart>, CodecError> {
    let mut given_up = Vec::new();
    for section in &scan.framing.sections {
        if section.name() != "NeuAsmSld" {
            continue;
        }
        let Some(region) = crate::container::section_region(&scan.framing.data, section) else {
            continue;
        };
        let fields = crate::assembly::member_fields(region).collect::<Vec<_>>();
        let Some(&first) = fields.first() else {
            continue;
        };
        let field = section.offset() + first;
        let (kind, offset, reason) =
            match scan.assembly.as_ref().filter(|table| table.offset == field) {
                Some(table) => match table
                    .stop
                    .as_ref()
                    .filter(|stop| stop.row_unread && stop.offset < section.end())
                {
                    Some(stop) => ("component-rows", stop.offset, stop.reason),
                    None => continue,
                },
                None if fields.len() > 1 => (
                    "component-table",
                    field,
                    "the section holds more than one members field",
                ),
                None if crate::assembly::member_table(region).is_some() => (
                    "component-table",
                    field,
                    "another NeuAsmSld section also holds a component table",
                ),
                None => (
                    "component-table",
                    field,
                    "the members field header is not the table array form the reader reads",
                ),
            };
        let Some(bytes) = scan.framing.data.get(offset..section.end()) else {
            continue;
        };
        let part = Part::at(
            "creo",
            kind,
            u64_from_index(offset),
            bytes,
            [ArenaClass::Model],
        );
        given_up.push(ctx.give_up_part(
            part,
            CodecError::malformed(format_args!(
                "{}: {reason}",
                CreoLossCode::AssemblyMemberTableIncomplete.kind()
            )),
        )?);
    }
    Ok(given_up)
}

/// The native address of a component row.
fn member_record(member: &Member) -> String {
    format!("NeuAsmSld:members:{}", member.id)
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
