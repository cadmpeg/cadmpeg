// SPDX-License-Identifier: Apache-2.0
//! Neutral assembly occurrences from placement frames whose prototype and
//! translation unit the part establishes.

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
use crate::native::om::{ExpressionUnit, ExternalReference, ParameterFormula};
use crate::native::structure::FastLoadComponentOccurrence;

/// Length unit of the part's numeric expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartLengthUnit {
    Millimeter,
    Inch,
    Mixed,
    Undeclared,
}

fn part_length_unit<'a>(units: impl IntoIterator<Item = &'a ExpressionUnit>) -> PartLengthUnit {
    let (mut millimeter, mut inch) = (false, false);
    for unit in units {
        match unit {
            ExpressionUnit::Millimeter => millimeter = true,
            ExpressionUnit::Inch => inch = true,
            ExpressionUnit::Degree | ExpressionUnit::Native(_) => {}
        }
    }
    match (millimeter, inch) {
        (true, false) => PartLengthUnit::Millimeter,
        (false, true) => PartLengthUnit::Inch,
        (true, true) => PartLengthUnit::Mixed,
        (false, false) => PartLengthUnit::Undeclared,
    }
}

/// The one prototype named by every use of a placement UUID group whose use
/// count equals its placement count.
fn unique_prototype(
    placement_count: usize,
    prototype_indices: impl IntoIterator<Item = usize>,
    prototype_count: usize,
) -> Option<usize> {
    let mut prototype = None;
    let mut use_count = 0_usize;
    for index in prototype_indices {
        use_count += 1;
        if index >= prototype_count || prototype.is_some_and(|known| known != index) {
            return None;
        }
        prototype = Some(index);
    }
    (use_count == placement_count)
        .then_some(prototype)
        .flatten()
}

fn unique_external_filename<'a>(
    prototype_name: &str,
    paths: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let mut matching = None;
    for path in paths {
        let Some(filename) = path.rsplit(['/', '\\']).next() else {
            continue;
        };
        let Some((stem, extension)) = filename.rsplit_once('.') else {
            continue;
        };
        if !extension.eq_ignore_ascii_case("prt") || !stem.eq_ignore_ascii_case(prototype_name) {
            continue;
        }
        if matching.is_some_and(|known| known != filename) {
            return None;
        }
        matching = Some(filename);
    }
    matching
}

fn external_prototype(filename: Option<&str>) -> PrototypeReference {
    PrototypeReference::External {
        document: filename.map_or(ExternalDocument::Missing {}, |filename| {
            ExternalDocument::path(filename.to_owned())
        }),
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

/// Prototype index of each placement whose UUID group names one prototype.
fn placement_prototypes(
    ctx: &DecodeContext<'_>,
    model: &NativeModel,
) -> Result<Vec<Option<usize>>, CodecError> {
    let placements = &model.om.assembly_placements;
    let mut placements_by_uuid = BTreeMap::<&str, Vec<usize>>::new();
    for (index, placement) in placements.iter().enumerate() {
        placements_by_uuid
            .entry(placement.uuid.as_str())
            .or_default()
            .push(index);
    }
    let mut uuid_ids_by_text = BTreeMap::<&str, Vec<&str>>::new();
    for uuid in &model.structure.uuids {
        uuid_ids_by_text
            .entry(uuid.uuid.as_str())
            .or_default()
            .push(uuid.id.as_str());
    }
    let mut uses_by_uuid_id = BTreeMap::<&str, Vec<&FastLoadComponentOccurrence>>::new();
    for use_record in model.structure.occurrences.as_slice() {
        uses_by_uuid_id
            .entry(use_record.component_uuid.as_str())
            .or_default()
            .push(use_record);
    }
    let mut prototypes = ctx.alloc_filled(
        placements.len(),
        None,
        "NX assembly placement prototype joins",
    )?;
    for (uuid_text, placement_indices) in &placements_by_uuid {
        let Some([uuid_id]) = uuid_ids_by_text.get(uuid_text).map(Vec::as_slice) else {
            continue;
        };
        let uses = uses_by_uuid_id.get(uuid_id).map_or(&[][..], Vec::as_slice);
        let Some(prototype) = unique_prototype(
            placement_indices.len(),
            uses.iter().map(|use_record| use_record.prototype_index()),
            model.structure.prototypes.len(),
        ) else {
            continue;
        };
        for &index in placement_indices {
            prototypes[index] = Some(prototype);
        }
    }
    Ok(prototypes)
}

pub(super) fn attach(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    model: &NativeModel,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let placements = &model.om.assembly_placements;
    let use_count = model.structure.occurrences.as_slice().len();
    if placements.is_empty() {
        if use_count != 0 {
            losses.push(NxLossCode::AssemblyOccurrencesUnresolved.note(format!(
                "No NX placement frame is available for {use_count} FastLoad component use(s); their native roster records remain in the NX namespace."
            )));
        }
        return Ok(());
    }
    let unit = part_length_unit(
        model
            .om
            .expressions
            .iter()
            .map(|expression: &ParameterFormula| &expression.unit),
    );
    if unit != PartLengthUnit::Millimeter {
        let reason = match unit {
            PartLengthUnit::Inch => "its length expressions declare inches",
            PartLengthUnit::Mixed => "its length expressions declare both millimeters and inches",
            PartLengthUnit::Undeclared | PartLengthUnit::Millimeter => {
                "it declares no length expression"
            }
        };
        losses.push(NxLossCode::AssemblyPlacementFrameUnresolved.note(format!(
            "Withheld {} NX placement(s) and {use_count} FastLoad component use(s) from neutral output because the part does not establish the placement translation unit: {reason}. Their placement frames and component uses remain in the NX native namespace.",
            placements.len()
        )));
        return Ok(());
    }

    let prototype_by_placement = placement_prototypes(ctx, model)?;
    let resolved = prototype_by_placement.iter().flatten().count();
    let unresolved = placements.len() - resolved;
    if unresolved != 0 {
        losses.push(NxLossCode::AssemblyPlacementFrameUnresolved.note(format!(
            "Withheld {unresolved} of {} NX placement(s) because their UUID group does not name exactly one FastLoad prototype with equal placement and use counts. Their placement frames remain in the NX native namespace.",
            placements.len()
        )));
    }
    if resolved == 0 {
        return Ok(());
    }

    let scope = IdScope::native(cadmpeg_ir::identity_component!("assembly"));
    let root_definition = product_definition_id(&scope, "root")?;
    let root_occurrence = occurrence_id(&scope, "root")?;
    ctx.charge_entities(
        u64_from_index(
            resolved
                .checked_add(2)
                .ok_or_else(|| CodecError::malformed("NX assembly occurrence count overflows"))?,
        ),
        "NX assembly occurrence graph",
    )?;
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

    let external_references: &[ExternalReference] = &model.om.external_references;
    let mut filenames = BTreeMap::<usize, Option<&str>>::new();
    let (mut joined_filenames, mut missing_documents) = (0_usize, 0_usize);
    let mut ordinal = 0_u32;
    for (placement, prototype_index) in placements.iter().zip(&prototype_by_placement) {
        let Some(prototype_index) = *prototype_index else {
            continue;
        };
        let prototype = &model.structure.prototypes[prototype_index];
        let filename = *filenames.entry(prototype_index).or_insert_with(|| {
            unique_external_filename(
                &prototype.name,
                external_references
                    .iter()
                    .map(|reference| reference.path.as_str()),
            )
        });
        match filename {
            Some(filename) => {
                joined_filenames += 1;
                ctx.retain_bytes(
                    u64_from_index(filename.len()),
                    "NX external component filename",
                )?;
            }
            None => missing_documents += 1,
        }
        let name = (!prototype.name.trim().is_empty()).then(|| prototype.name.clone());
        if let Some(name) = &name {
            ctx.retain_bytes(u64_from_index(name.len()), "NX occurrence prototype name")?;
        }
        ir.model.occurrences.push(Occurrence {
            id: occurrence_id(&scope, &format!("placement-{}", placement.source_offset))?,
            prototype: external_prototype(filename),
            parent: OccurrenceParent::Occurrence {
                occurrence: root_occurrence.clone(),
            },
            ordinal,
            transform: placement_transform(placement)?,
            linked_prototype: None,
            scale: [FiniteReal::ONE; 3],
            name,
            visible: None,
            link: None,
            native_ref: Some(placement.id.clone()),
        });
        ordinal += 1;
    }
    ir.model
        .product_definitions
        .sort_by(|left, right| left.id.cmp(&right.id));
    ir.model
        .occurrences
        .sort_by(|left, right| left.id.cmp(&right.id));

    losses.push(NxLossCode::AssemblyPlacementUnitFromExpressions.note(format!(
        "The {resolved} emitted NX placement translation(s) use millimeters, the length unit that every numeric expression of the part declares."
    )));
    losses.push(NxLossCode::AssemblyPlacementParentAssumed.note(format!(
        "The {resolved} emitted NX placement(s) are attached to the assembly root by assumption; the serialized parent relation of a placement is unresolved."
    )));
    if joined_filenames != 0 {
        losses.push(NxLossCode::AssemblyExternalFilenameAssumed.note(format!(
            "{joined_filenames} NX component occurrence(s) join an external reference by a unique case-insensitive filename stem; the source directory remains unresolved."
        )));
    }
    if missing_documents != 0 {
        losses.push(NxLossCode::AssemblyExternalDocumentUnresolved.note(format!(
            "{missing_documents} NX component occurrence(s) keep their part names with no uniquely named external document."
        )));
    }
    let unplaced_uses = use_count
        .checked_sub(resolved)
        .ok_or_else(|| CodecError::malformed("NX emitted placements exceed their FastLoad uses"))?;
    if unplaced_uses != 0 {
        losses.push(NxLossCode::AssemblyOccurrencesUnresolved.note(format!(
            "{unplaced_uses} of {use_count} FastLoad component use(s) have no emitted placement."
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        external_prototype, part_length_unit, unique_external_filename, unique_prototype,
        PartLengthUnit,
    };
    use crate::native::om::ExpressionUnit;
    use cadmpeg_ir::products::{ExternalDocument, PrototypeReference};

    #[test]
    fn repeated_uses_of_one_prototype_resolve_without_an_ordinal_join() {
        assert_eq!(unique_prototype(3, [0, 0, 0], 1), Some(0));
    }

    #[test]
    fn unequal_counts_or_several_prototypes_remain_unresolved() {
        assert_eq!(unique_prototype(3, [0, 0], 1), None);
        assert_eq!(unique_prototype(2, [0, 1], 2), None);
        assert_eq!(unique_prototype(1, [1], 1), None);
        assert_eq!(unique_prototype(1, [], 1), None);
    }

    #[test]
    fn only_millimeter_length_expressions_establish_millimeters() {
        let mm = ExpressionUnit::Millimeter;
        let inch = ExpressionUnit::Inch;
        let degree = ExpressionUnit::Degree;
        assert_eq!(part_length_unit([&mm, &degree]), PartLengthUnit::Millimeter);
        assert_eq!(part_length_unit([&inch]), PartLengthUnit::Inch);
        assert_eq!(part_length_unit([&mm, &inch]), PartLengthUnit::Mixed);
        assert_eq!(part_length_unit([&degree]), PartLengthUnit::Undeclared);
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
