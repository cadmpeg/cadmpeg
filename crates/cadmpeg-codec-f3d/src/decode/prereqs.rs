//! Phase 1 of the Design graph: the Design records that geometry and product
//! projection read.
//!
//! Design-intent decoding reads [`ProductPrereqs`] and never writes it.

use cadmpeg_core::{decode::DecodeContext, CodecError};

use crate::container::{self, BrepFacts, ContainerScan};
use crate::native::F3dNative;
use crate::records::{
    bodies::{DesignBodyBinding, DesignBodyMember},
    decal::DesignRecordHeader,
    entity_header::{DesignEntityHeader, SegmentType},
    feature::{
        assembly_features::DesignComponentOccurrence, scope::DesignParameterScope,
        surface_ops::DesignSurfaceTrimOperation,
    },
    parameters::{DesignParameter, DesignParameterOwner},
    recipes::{ConstructionRecipe, DesignComponentNamingSpace},
    references::DesignMaterialAssignment,
};

/// Design records that geometry and product projection read.
#[derive(Debug)]
pub(super) struct ProductPrereqs {
    pub(super) construction_recipes: Vec<ConstructionRecipe>,
    pub(super) design_material_assignments: Vec<DesignMaterialAssignment>,
    pub(super) design_types: Vec<SegmentType>,
    pub(super) design_parameters: Vec<DesignParameter>,
    pub(super) design_entity_headers: Vec<DesignEntityHeader>,
    /// Headers of the records that entity headers index. Parameter-owner
    /// headers are not in this set: design-intent decoding adds them with the
    /// sketch-relation headers.
    pub(super) design_record_headers: Vec<DesignRecordHeader>,
    pub(super) design_parameter_owners: Vec<DesignParameterOwner>,
    pub(super) design_component_occurrences: Vec<DesignComponentOccurrence>,
    /// Scopes after history-bound variant admission, before design-intent
    /// binding.
    pub(super) design_parameter_scopes: Vec<DesignParameterScope>,
    /// Surface-trim operations decoded from the scopes before variant
    /// admission.
    pub(super) design_surface_trim_operations: Vec<DesignSurfaceTrimOperation>,
    pub(super) design_component_naming_spaces: Vec<DesignComponentNamingSpace>,
    pub(super) design_body_bindings: Vec<DesignBodyBinding>,
    pub(super) design_body_members: Vec<DesignBodyMember>,
    /// Retained-byte allowance for ASM history binding, taken before phase 1.
    pub(super) history_retained_allowance: u64,
}

impl ProductPrereqs {
    /// Move every record into native storage.
    pub(super) fn store(self, native: &mut F3dNative) {
        let Self {
            construction_recipes,
            design_material_assignments,
            design_types,
            design_parameters,
            design_entity_headers,
            design_record_headers,
            design_parameter_owners,
            design_component_occurrences,
            design_parameter_scopes,
            design_surface_trim_operations,
            design_component_naming_spaces,
            design_body_bindings,
            design_body_members,
            history_retained_allowance: _,
        } = self;
        native.construction_recipes = construction_recipes;
        native.design_material_assignments = design_material_assignments;
        native.design_types = design_types;
        native.design_parameters = design_parameters;
        native.design_entity_headers = design_entity_headers;
        native.design_record_headers = design_record_headers;
        native.design_parameter_owners = design_parameter_owners;
        native.design_component_occurrences = design_component_occurrences;
        native.design_parameter_scopes = design_parameter_scopes;
        native.design_surface_trim_operations = design_surface_trim_operations;
        native.design_component_naming_spaces = design_component_naming_spaces;
        native.design_body_bindings = design_body_bindings;
        native.design_body_members = design_body_members;
    }
}

/// Decode the product prerequisites of one Design document.
pub(super) fn decode(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    design_body_bindings: Vec<DesignBodyBinding>,
) -> Result<ProductPrereqs, CodecError> {
    let history_retained_allowance = ctx.retained_bytes_available();
    // Scope-variant admission reads state identities and chain links only.
    let mut history_index = Vec::new();
    for history_brep in container::history_breps(scan) {
        let bytes = scan.entry_bytes(&history_brep.name)?;
        if let Some(history) = crate::history::decode_header_index(
            bytes,
            &history_brep.name,
            history_ref_width(history_brep),
        ) {
            history_index.push(history);
        }
    }
    let construction_recipes = crate::design::decode::parameters::decode_recipes(scan)?;
    let design_material_assignments = crate::materials::decode_design_assignments(scan)?;
    let design_types = crate::design::decode::meta::decode_types(scan)?;
    let design_parameters = crate::design::decode::parameters::decode_parameters(scan)?;
    let design_entity_headers = crate::design::decode::sketch::decode_entity_headers(scan)?;
    let design_record_headers =
        crate::design::decode::sketch::decode_record_headers(scan, &design_entity_headers)?;
    let mut owner_headers = design_record_headers.clone();
    append_related_record_headers(
        scan,
        &mut owner_headers,
        &parameter_owner_indices(&design_parameters),
    )?;
    let design_parameter_owners = crate::design::decode::parameters::decode_parameter_owners(
        scan,
        &design_parameters,
        &owner_headers,
    )?;
    drop(owner_headers);
    let design_component_occurrences =
        crate::design::decode::components::decode_component_occurrences(scan)?;
    let mut design_parameter_scopes =
        crate::design::decode::scopes::parameter_scope::decode_parameter_scopes(
            ctx,
            scan,
            &crate::design::decode::scopes::parameter_scope::ParameterScopeSources {
                entities: &design_entity_headers,
                types: &design_types,
                parameters: &design_parameters,
                parameter_owners: &design_parameter_owners,
                component_occurrences: &design_component_occurrences,
                recipes: &construction_recipes,
            },
        )?;
    let design_surface_trim_operations =
        crate::design::decode::surface_trim::decode_surface_trim_operations(
            scan,
            &design_parameter_scopes,
        )?;
    crate::design::decode::scopes::parameter_scope::admit_history_bound_scope_variants(
        ctx,
        &mut design_parameter_scopes,
        &history_index,
    )?;
    let design_component_naming_spaces =
        crate::design::decode::meta::decode_component_naming_spaces(scan)?;
    let design_body_members = crate::design::decode::body::decode_body_members(scan)?;
    Ok(ProductPrereqs {
        construction_recipes,
        design_material_assignments,
        design_types,
        design_parameters,
        design_entity_headers,
        design_record_headers,
        design_parameter_owners,
        design_component_occurrences,
        design_parameter_scopes,
        design_surface_trim_operations,
        design_component_naming_spaces,
        design_body_bindings,
        design_body_members,
        history_retained_allowance,
    })
}

/// Reference width of one ASM history stream.
pub(super) fn history_ref_width(history_brep: &BrepFacts) -> cadmpeg_asm::kernel_header::RefWidth {
    history_brep
        .kernel
        .as_ref()
        .and_then(crate::container::KernelFraming::asm_header)
        .map_or(cadmpeg_asm::kernel_header::RefWidth::Eight, |header| {
            header.width
        })
}

/// Owner record of every Design parameter.
pub(super) fn parameter_owner_indices(parameters: &[DesignParameter]) -> Vec<(String, u32)> {
    collect_related_indices(parameters.iter().filter_map(|parameter| {
        Some((
            crate::ids::native_stream(&parameter.id)?,
            parameter.owner_record_index()?,
        ))
    }))
}

pub(super) fn collect_related_indices<'a>(
    indices: impl IntoIterator<Item = (&'a str, u32)>,
) -> Vec<(String, u32)> {
    let mut collected = Vec::new();
    for (stream, index) in indices {
        let stream = stream.to_owned();
        collected.push((stream, index));
    }
    collected
}

/// Add the headers of related records that `headers` does not hold yet.
pub(super) fn append_related_record_headers(
    scan: &ContainerScan,
    headers: &mut Vec<DesignRecordHeader>,
    indices: &[(String, u32)],
) -> Result<(), CodecError> {
    let existing = headers
        .iter()
        .filter_map(|record| Some((crate::ids::native_stream(&record.id)?, record.record_index)))
        .collect::<std::collections::HashSet<_>>();
    let mut related = crate::design::decode::sketch::decode_related_record_headers(scan, indices)?;
    related.retain(|record| {
        crate::ids::native_stream(&record.id)
            .is_none_or(|stream| !existing.contains(&(stream, record.record_index)))
    });
    drop(existing);
    headers.extend(related);
    headers.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(())
}
