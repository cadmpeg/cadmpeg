// SPDX-License-Identifier: Apache-2.0
//! Neutral assembly occurrences from UUID-resolved placement groups.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{
    ExternalDocument, Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind,
    PrototypeReference,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::transform::Transform;

use crate::decode::ids::IdScope;
use crate::loss::NxLossCode;
use crate::native::assembly_placement::AssemblyPlacement;
use crate::native::model::NativeModel;
use crate::native::om::ExternalReference;
use crate::native::structure::{FastLoadComponentOccurrence, FastLoadComponentUuid};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrototypeJoin {
    EqualCount {
        prototype_index: usize,
    },
    CountAssumed {
        prototype_index: usize,
        placement_count: usize,
        use_count: usize,
    },
}

impl PrototypeJoin {
    fn prototype_index(self) -> usize {
        match self {
            Self::EqualCount { prototype_index }
            | Self::CountAssumed {
                prototype_index, ..
            } => prototype_index,
        }
    }
}

fn unique_prototype_join(
    placement_count: usize,
    prototype_indices: impl IntoIterator<Item = usize>,
    prototype_count: usize,
) -> Option<PrototypeJoin> {
    let mut prototype = None;
    let mut use_count = 0_usize;
    for index in prototype_indices {
        use_count = use_count.checked_add(1)?;
        if index >= prototype_count || prototype.is_some_and(|known| known != index) {
            return None;
        }
        prototype = Some(index);
    }
    let prototype_index = prototype?;
    Some(if use_count == placement_count {
        PrototypeJoin::EqualCount { prototype_index }
    } else {
        PrototypeJoin::CountAssumed {
            prototype_index,
            placement_count,
            use_count,
        }
    })
}

fn matching_external_filename<'a>(prototype_name: &str, path: &'a str) -> Option<&'a str> {
    let filename = path.rsplit(['/', '\\']).next()?;
    let (stem, extension) = filename.rsplit_once('.')?;
    (extension.eq_ignore_ascii_case("prt") && stem.eq_ignore_ascii_case(prototype_name))
        .then_some(filename)
}

fn unique_external_filename<'a>(
    prototype_name: &str,
    paths: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let mut matching_filename = None;
    for path in paths {
        let Some(filename) = matching_external_filename(prototype_name, path) else {
            continue;
        };
        if matching_filename.is_some_and(|known| known != filename) {
            return None;
        }
        matching_filename = Some(filename);
    }
    matching_filename
}

fn external_filename<'a>(
    prototype_name: &str,
    references: &'a [ExternalReference],
) -> Option<&'a str> {
    unique_external_filename(
        prototype_name,
        references.iter().map(|reference| reference.path.as_str()),
    )
}

fn external_prototype(filename: Option<&str>) -> PrototypeReference {
    let document = filename.map_or(ExternalDocument::Missing {}, |filename| {
        ExternalDocument::path(filename.to_owned())
    });
    PrototypeReference::External {
        document,
        object: None,
    }
}

fn placement_transform(placement: &AssemblyPlacement) -> Result<Transform, CodecError> {
    let m = placement.matrix;
    Transform::affine([
        [m[0], m[1], m[2], m[3]],
        [m[4], m[5], m[6], m[7]],
        [m[8], m[9], m[10], m[11]],
    ])
    .ok_or_else(|| CodecError::malformed("NX assembly placement transform is not finite"))
}

fn product_definition_id(scope: &IdScope, key: &str) -> Result<ProductDefinitionId, CodecError> {
    scope.id_from_display(&cadmpeg_ir::identity_component!("product-definition"), key)
}

fn occurrence_id(scope: &IdScope, key: &str) -> Result<OccurrenceId, CodecError> {
    scope.id_from_display(&cadmpeg_ir::identity_component!("occurrence"), key)
}

pub(super) fn attach(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    model: &NativeModel,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let placements = &model.om.assembly_placements;
    let prototypes = &model.structure.prototypes;
    let uuids = &model.structure.uuids;
    let uses = model.structure.occurrences.as_slice();
    let external_references = &model.om.external_references;

    let mut placements_by_uuid = BTreeMap::<&str, Vec<usize>>::new();
    for (index, placement) in placements.iter().enumerate() {
        placements_by_uuid
            .entry(placement.uuid.as_str())
            .or_default()
            .push(index);
    }

    let mut uuid_records_by_text = BTreeMap::<&str, Vec<&FastLoadComponentUuid>>::new();
    let mut uuid_text_by_id = BTreeMap::<&str, &str>::new();
    for uuid in uuids {
        uuid_records_by_text
            .entry(uuid.uuid.as_str())
            .or_default()
            .push(uuid);
        uuid_text_by_id.insert(uuid.id.as_str(), uuid.uuid.as_str());
    }

    let mut uses_by_uuid = BTreeMap::<&str, Vec<&FastLoadComponentOccurrence>>::new();
    for use_record in uses {
        if let Some(uuid) = uuid_text_by_id.get(use_record.component_uuid.as_str()) {
            uses_by_uuid.entry(uuid).or_default().push(use_record);
        }
    }

    let mut prototype_by_placement = ctx.alloc_filled(
        placements.len(),
        None,
        "NX assembly placement prototype joins",
    )?;
    let mut resolved_use_count = 0_usize;
    let mut count_assumed_groups = 0_usize;
    let mut count_assumed_placements = 0_usize;
    let mut count_assumed_uses = 0_usize;
    let mut uuid_join_groups = 0_usize;
    let mut uuid_join_placements = 0_usize;
    for (uuid_text, placement_indices) in &placements_by_uuid {
        let Some([uuid_record]) = uuid_records_by_text.get(uuid_text).map(Vec::as_slice) else {
            continue;
        };
        let group_uses = uses_by_uuid
            .get(uuid_record.uuid.as_str())
            .map_or(&[][..], Vec::as_slice);
        let Some(join) = unique_prototype_join(
            placement_indices.len(),
            group_uses
                .iter()
                .map(|use_record| use_record.prototype_index()),
            prototypes.len(),
        ) else {
            continue;
        };
        let prototype_index = join.prototype_index();
        uuid_join_groups += 1;
        uuid_join_placements += placement_indices.len();
        if let PrototypeJoin::CountAssumed {
            placement_count,
            use_count,
            ..
        } = join
        {
            count_assumed_groups += 1;
            count_assumed_placements += placement_count;
            count_assumed_uses += use_count;
        }
        for &placement_index in placement_indices {
            prototype_by_placement[placement_index] = Some(prototype_index);
        }
        resolved_use_count += group_uses.len().min(placement_indices.len());
    }

    let resolved_placement_count = prototype_by_placement
        .iter()
        .filter(|prototype| prototype.is_some())
        .count();
    let mut filename_join_count = 0_usize;
    let mut unresolved_external_count = 0_usize;
    if !placements.is_empty() {
        let scope = IdScope::native(cadmpeg_ir::identity_component!("assembly"));
        let root_definition = product_definition_id(&scope, "root")?;
        let root_occurrence = occurrence_id(&scope, "root")?;
        let admitted = 2_usize
            .checked_add(placements.len())
            .ok_or_else(|| CodecError::malformed("NX assembly entity count overflows"))?;
        ctx.charge_entities(u64_from_index(admitted), "NX assembly occurrence graph")?;

        ir.model.product_definitions.push(ProductDefinition {
            id: root_definition.clone(),
            kind: ProductDefinitionKind::Group,
            source_name: None,
            label: None,
            description: None,
            part_number: None,
            bom_properties: BTreeMap::new(),
            bodies: Vec::new(),
            sketches: Vec::new(),
            native_ref: None,
        });
        ir.model.occurrences.push(Occurrence {
            id: root_occurrence.clone(),
            prototype: PrototypeReference::Local {
                definition: root_definition,
            },
            parent: OccurrenceParent::Root {},
            ordinal: 0,
            transform: Transform::identity(),
            linked_prototype: None,
            scale: [FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        });

        let mut external_filenames_by_prototype = BTreeMap::<usize, Option<&str>>::new();
        for (index, (placement, prototype_index)) in placements
            .iter()
            .zip(prototype_by_placement.iter())
            .enumerate()
        {
            let prototype = (*prototype_index)
                .map(|prototype_index| {
                    prototypes
                        .get(prototype_index)
                        .map(|prototype| (prototype_index, prototype))
                        .ok_or_else(|| CodecError::malformed("NX assembly prototype index changed"))
                })
                .transpose()?;
            let id = occurrence_id(&scope, &format!("placement-{}", placement.source_offset))?;
            let (prototype_reference, occurrence_name) = if let Some((prototype_index, prototype)) =
                prototype
            {
                let filename = *external_filenames_by_prototype
                    .entry(prototype_index)
                    .or_insert_with(|| external_filename(&prototype.name, external_references));
                if let Some(filename) = filename {
                    filename_join_count += 1;
                    ctx.retain_bytes(
                        u64_from_index(filename.len()),
                        "NX external component filename",
                    )?;
                } else {
                    unresolved_external_count += 1;
                }
                let name = (!prototype.name.trim().is_empty())
                    .then_some(prototype.name.as_str())
                    .or_else(|| {
                        (!placement.label.trim().is_empty()).then_some(placement.label.as_str())
                    });
                if let Some(name) = name {
                    ctx.retain_bytes(u64_from_index(name.len()), "NX occurrence prototype name")?;
                }
                (external_prototype(filename), name.map(str::to_owned))
            } else {
                let name = (!placement.label.trim().is_empty()).then_some(placement.label.as_str());
                if let Some(name) = name {
                    ctx.retain_bytes(u64_from_index(name.len()), "NX unresolved occurrence label")?;
                }
                (PrototypeReference::Unresolved {}, name.map(str::to_owned))
            };
            ir.model.occurrences.push(Occurrence {
                id,
                prototype: prototype_reference,
                parent: OccurrenceParent::Occurrence {
                    occurrence: root_occurrence.clone(),
                },
                ordinal: u32::try_from(index).map_err(|_| {
                    CodecError::malformed("NX assembly placement ordinal exceeds u32")
                })?,
                transform: placement_transform(placement)?,
                linked_prototype: None,
                scale: [FiniteReal::ONE; 3],
                name: occurrence_name,
                visible: None,
                link: None,
                native_ref: Some(placement.id.clone()),
            });
        }

        ir.model
            .product_definitions
            .sort_by(|left, right| left.id.cmp(&right.id));
        ir.model
            .occurrences
            .sort_by(|left, right| left.id.cmp(&right.id));
        losses.push(NxLossCode::AssemblyPlacementUnitsAssumed.note(format!(
            "The {} emitted NX assembly placement translation(s) use millimeters.",
            placements.len()
        )));
        losses.push(NxLossCode::AssemblyHierarchyAssumed.note(format!(
            "The {} NX placement(s) are direct children of one synthetic root in native source order.",
            placements.len()
        )));
        if filename_join_count != 0 {
            losses.push(NxLossCode::AssemblyExternalFilenameAssumed.note(format!(
                "{filename_join_count} NX component occurrence(s) join external references by a unique case-insensitive filename stem; the source directory remains unresolved."
            )));
        }
        if unresolved_external_count != 0 {
            losses.push(NxLossCode::AssemblyExternalDocumentUnresolved.note(format!(
                "{unresolved_external_count} NX component occurrence(s) retain their part names in occurrence names with unresolved external documents."
            )));
        }
        if count_assumed_groups != 0 {
            losses.push(NxLossCode::AssemblyPrototypeCountAssumed.note(format!(
                "{count_assumed_groups} NX UUID group(s) assign one unique FastLoad prototype across {count_assumed_placements} placement(s) and {count_assumed_uses} use(s) despite unequal counts."
            )));
        }
        if uuid_join_groups != 0 {
            losses.push(NxLossCode::AssemblyUuidPrototypeAssumed.note(format!(
                "UUID grouping assigns {uuid_join_placements} NX placement(s) across {uuid_join_groups} group(s) to the sole FastLoad prototype named by their uses."
            )));
        }
    }

    let unresolved_placement_count = placements.len() - resolved_placement_count;
    let unresolved_use_count = uses.len() - resolved_use_count;
    if unresolved_placement_count != 0 || unresolved_use_count != 0 {
        losses.push(NxLossCode::AssemblyOccurrencesUnresolved.note(format!(
            "No unique prototype reading is available for {unresolved_placement_count} of {} NX placement(s); {unresolved_use_count} of {} FastLoad use(s) have no transferred placement slot. Native placement transforms remain as occurrences when available.",
            placements.len(),
            uses.len()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        external_prototype, unique_external_filename, unique_prototype_join, PrototypeJoin,
    };
    use cadmpeg_ir::products::{ExternalDocument, PrototypeReference};

    #[test]
    fn repeated_uses_of_one_prototype_resolve_without_an_ordinal_join() {
        assert_eq!(
            unique_prototype_join(3, [0, 0, 0], 1),
            Some(PrototypeJoin::EqualCount { prototype_index: 0 })
        );
    }

    #[test]
    fn unequal_counts_with_one_prototype_use_the_count_assumption() {
        assert_eq!(
            unique_prototype_join(3, [0, 0], 1),
            Some(PrototypeJoin::CountAssumed {
                prototype_index: 0,
                placement_count: 3,
                use_count: 2,
            })
        );
    }

    #[test]
    fn multiple_prototypes_remain_unresolved() {
        assert_eq!(unique_prototype_join(2, [0, 1], 2), None);
    }

    #[test]
    fn a_roster_index_without_a_prototype_remains_unresolved() {
        assert_eq!(unique_prototype_join(1, [1], 1), None);
        assert_eq!(unique_prototype_join(1, [], 1), None);
    }

    #[test]
    fn repeated_directories_resolve_by_one_exact_basename() {
        let paths = [
            "C:\\source\\widget.prt",
            "D:\\backup\\widget.prt",
            "C:\\source\\other.prt",
        ];
        assert_eq!(
            unique_external_filename("Widget", paths),
            Some("widget.prt")
        );
        assert_eq!(
            unique_external_filename("Widget", ["one/widget.prt", "two/WIDGET.prt"]),
            None
        );
        assert_eq!(unique_external_filename("Widget", ["widget.step"]), None);
    }

    #[test]
    fn missing_external_document_has_no_object_identity() {
        assert_eq!(
            external_prototype(None),
            PrototypeReference::External {
                document: ExternalDocument::Missing {},
                object: None,
            }
        );
    }
}
