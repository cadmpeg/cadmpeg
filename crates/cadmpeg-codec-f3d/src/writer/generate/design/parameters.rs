// SPDX-License-Identifier: Apache-2.0
//! The parameters of a design scope, written as Fusion user parameters.
//!
//! Fusion holds a parameter outside a written feature only as a user
//! parameter, so a model parameter whose feature is not written is written as
//! one, and its feature's `export:evaluation.missing` names it. A parameter is
//! written in dependency order, under a legal and unique Fusion name, with its
//! expression where the Fusion form of every construct is established and as
//! its cached value otherwise (`export:parameter.flattened`). A parameter
//! owned by a container or an attribute is written without that binding
//! (`export:value.unwritable`), and an integer parameter as a real
//! (`f3d:writer.integer-as-real`).

use std::collections::{BTreeSet, HashMap};

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{parameter_flattened, parameter_renamed, value_unwritable};
use cadmpeg_ir::design::dimension::ParameterRef;
use cadmpeg_ir::design::expr::{Expression, ExpressionDialect, ExpressionToken};
use cadmpeg_ir::design::parameter::{
    DesignParameter, ParameterDefinition, ParameterOwner, ParameterValue,
};
use cadmpeg_ir::design::units::{Quantity, Unit, UnitSymbol, ValueKind};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::report::loss::LossNote;

use super::expression;
use crate::loss::F3dLossCode;
use crate::native::F3dNative;
use crate::records::identity::{Located, RecordedValue};
use crate::records::parameters::{
    DesignParameterDiscriminator, DesignParameterDraft, DesignParameterSource,
};
use crate::writer::generate::presentation::GeneratedDesignRegistry;
use crate::writer::generate::records::encode_document_parameters;
use crate::writer::generate::reduce::named_loss;

/// The parameter collection the native path's parameter tails name.
const NATIVE_PARAMETER_COLLECTION: u32 = 19;

/// The parameters written as Fusion user parameters, and the losses taken.
pub(in crate::writer::generate) struct ParameterPlan {
    written: Vec<PlannedParameter>,
    losses: Vec<LossNote>,
}

/// One user parameter as the record holds it.
struct PlannedParameter {
    source: ParameterId,
    name: String,
    expression: String,
    unit: Option<UnitSymbol>,
    /// The value in Fusion's database units: centimetres for a length,
    /// radians for an angle.
    evaluated: f64,
    comment: Option<String>,
}

/// Where the native path takes its parameter records from.
pub(in crate::writer::generate) enum ParameterSource {
    /// The native records: the neutral parameters equal their projection.
    Native,
    /// The design scope's parameters: the neutral parameters were edited.
    Neutral(ParameterPlan),
}

/// The quantity a Fusion user parameter holds.
#[derive(Clone, Copy)]
enum Form {
    Length,
    Angle,
    Real,
}

impl Form {
    fn of(kind: &ValueKind) -> Option<Self> {
        match kind {
            ValueKind::Quantity(quantity) if *quantity == Quantity::LENGTH => Some(Self::Length),
            ValueKind::Quantity(quantity) if *quantity == Quantity::ANGLE => Some(Self::Angle),
            ValueKind::Quantity(quantity) if *quantity == Quantity::REAL => Some(Self::Real),
            ValueKind::Integer => Some(Self::Real),
            ValueKind::Quantity(_)
            | ValueKind::Boolean
            | ValueKind::Text
            | ValueKind::Native(_) => None,
        }
    }

    /// The declared unit where Fusion spells it, else Fusion's default.
    fn unit(self, declared: Option<&Unit>) -> Option<UnitSymbol> {
        let (quantity, default) = match self {
            Self::Length => (Quantity::LENGTH, UnitSymbol::Mm),
            Self::Angle => (Quantity::ANGLE, UnitSymbol::Deg),
            Self::Real => return None,
        };
        Some(
            declared
                .and_then(|unit| expression::fusion_unit(unit, quantity))
                .unwrap_or(default),
        )
    }

    /// A canonical value in Fusion's database units (f3d.md §3.1: lengths in
    /// centimetres, angles in radians).
    fn database(self, canonical: f64) -> f64 {
        match self {
            Self::Length => canonical / 10.0,
            Self::Angle | Self::Real => canonical,
        }
    }
}

/// The plan for the parameters of the scope of `definition`.
pub(in crate::writer::generate) fn plan(
    target: &CadIr,
    definition: &ProductDefinitionId,
) -> ParameterPlan {
    let parameters = target
        .design
        .parameters
        .iter()
        .filter(|parameter| parameter.scope == *definition)
        .collect::<Vec<_>>();
    let mut omitted_kind = Vec::new();
    let mut omitted_driven = Vec::new();
    let mut candidates = Vec::new();
    for parameter in parameters {
        match (Form::of(&parameter.kind), &parameter.definition) {
            (None, _) => omitted_kind.push(parameter.id.to_string()),
            (Some(_), ParameterDefinition::Driven) => {
                omitted_driven.push(parameter.id.to_string());
            }
            (Some(form), _) => candidates.push((parameter, form)),
        }
    }
    let names = Names::new(&candidates);
    let (order, forced) = dependency_order(&candidates);
    let mut written_names: HashMap<&ParameterId, &str> = HashMap::new();
    let mut written_values: HashMap<&ParameterId, f64> = HashMap::new();
    let mut written = Vec::new();
    let mut flattened = Vec::new();
    let mut unwritten = Vec::new();
    let mut renamed = Vec::new();
    let mut unbound = Vec::new();
    let mut displayed = Vec::new();
    let mut integers = Vec::new();
    for index in order {
        let (parameter, form) = candidates[index];
        let unit = form.unit(parameter.unit.as_ref());
        let name_of = |id: &ParameterId| written_names.get(id).copied();
        let printed = if forced.contains(&index) {
            None
        } else {
            printed_expression(&parameter.definition, &name_of)
        };
        let canonical = cached(parameter).or_else(|| match (&printed, &parameter.definition) {
            (Some(_), ParameterDefinition::Expression(Expression::Neutral(neutral))) => {
                expression::evaluate(&neutral.0, &|id| written_values.get(id).copied())
            }
            _ => None,
        });
        let Some(canonical) = canonical else {
            unwritten.push(parameter.id.to_string());
            continue;
        };
        let text = match printed {
            Some(text) => text,
            None => {
                let Some(text) = expression::quantity_literal(canonical, unit) else {
                    unwritten.push(parameter.id.to_string());
                    continue;
                };
                flattened.push(parameter.id.to_string());
                text
            }
        };
        let name = names.of(index);
        let source_name = parameter
            .name
            .as_ref()
            .map(cadmpeg_core::text::NonBlankString::as_str);
        if source_name != Some(name) {
            renamed.push(format!(
                "{} ({} -> `{name}`)",
                parameter.id,
                source_name.map_or_else(|| "no name".to_owned(), |name| format!("`{name}`")),
            ));
        }
        match &parameter.owner {
            Some(ParameterOwner::Container(container)) => {
                unbound.push(format!("{} (owner container {container})", parameter.id));
            }
            Some(ParameterOwner::Attribute(attribute)) => {
                unbound.push(format!("{} (owner attribute {attribute})", parameter.id));
            }
            Some(ParameterOwner::Feature(_)) | None => {}
        }
        if let Some(display) = parameter.display {
            displayed.push(format!("{} ({display:?})", parameter.id));
        }
        if matches!(parameter.kind, ValueKind::Integer) {
            integers.push(parameter.id.to_string());
        }
        written_names.insert(&parameter.id, name);
        written_values.insert(&parameter.id, canonical);
        written.push(PlannedParameter {
            source: parameter.id.clone(),
            name: name.to_owned(),
            expression: text,
            unit,
            evaluated: form.database(canonical),
            comment: parameter
                .comment
                .clone()
                .filter(|comment| !comment.is_empty()),
        });
    }
    let mut losses = Vec::new();
    losses.extend(named_loss(
        F3dLossCode::WriterParameterOmitted,
        "parameter",
        "a Fusion user parameter holds only a length, an angle or a number",
        omitted_kind,
    ));
    losses.extend(named_loss(
        F3dLossCode::WriterParameterOmitted,
        "parameter",
        "a Fusion user parameter is never driven by the geometry",
        omitted_driven,
    ));
    losses.extend(parameter_flattened(
        "F3D",
        "states these parameters by their cached values: Fusion has no established form for their expressions",
        flattened,
    ));
    losses.extend(parameter_flattened(
        "F3D",
        "omits these parameters: their expressions have no established Fusion form and they hold no cached value",
        unwritten,
    ));
    losses.extend(parameter_renamed(
        "F3D",
        "renames these parameters to legal, unique Fusion names",
        renamed,
    ));
    losses.extend(value_unwritable(
        "F3D",
        "writes these parameters as user parameters without their owners: a Fusion user parameter is bound to no container or attribute",
        unbound,
    ));
    losses.extend(value_unwritable(
        "F3D",
        "writes these parameters without their diameter or radius display presentation: a Fusion user parameter holds no display flag",
        displayed,
    ));
    if !integers.is_empty() {
        integers.sort_unstable();
        losses.push(F3dLossCode::WriterIntegerAsReal.note(format!(
            "{} integer parameter(s) were written as Fusion user parameters, which hold a real: {}",
            integers.len(),
            integers.join(", ")
        )));
    }
    ParameterPlan { written, losses }
}

/// The native records when the neutral parameters equal their projection;
/// otherwise the root scope's parameters, and a loss naming each neutral
/// parameter that no scope parameter restates. It never refuses.
pub(in crate::writer::generate) fn native_or_neutral(
    target: &CadIr,
    native: &F3dNative,
) -> Result<ParameterSource, CodecError> {
    if target.model.parameters == native_projection(native)? {
        return Ok(ParameterSource::Native);
    }
    let mut plan = super::root_scope(target).map_or_else(
        || ParameterPlan {
            written: Vec::new(),
            losses: Vec::new(),
        },
        |scope| plan(target, &scope.definition),
    );
    let restated = super::restated_parameters(target);
    plan.losses.extend(named_loss(
        F3dLossCode::WriterParameterOmitted,
        "parameter",
        "the neutral parameters differ from the native Design records and no design parameter restates them",
        target
            .model
            .parameters
            .iter()
            .map(|row| row.id.to_string())
            .filter(|id| !restated.contains(id.as_str())),
    ));
    Ok(ParameterSource::Neutral(plan))
}

/// The native records' parameters as the decoder projects them.
fn native_projection(
    native: &F3dNative,
) -> Result<Vec<cadmpeg_ir::features::DesignParameter>, CodecError> {
    let decode_arena = cadmpeg_core::decode::DecodeArena::new();
    let (decode_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &decode_arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )?;
    Ok(
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            &decode_ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &native.design_parameters,
                source_parameters: &native.design_parameters,
                owners: &native.design_parameter_owners,
                scopes: &native.design_parameter_scopes,
                timelines: &native.design_feature_timelines,
                construction_groups: &native.design_construction_operand_groups,
                fillet_radius_groups: &native.design_fillet_radius_groups,
                edge_operands: &native.design_edge_operands,
                edge_identity_operands: &native.design_edge_identity_operands,
                edge_treatment_vertex_operands: &native.design_edge_treatment_vertex_operands,
                entity_selection_operands: &native.design_entity_selection_operands,
                design_types: &native.design_types,
                curve_identities: &native.sketch_curve_identities,
                face_operands: &native.design_face_operands,
                face_source_groups: &native.design_face_source_groups,
                body_recipe_operands: &native.design_body_recipe_operands,
                placements: &native.design_sketch_placements,
                placement_constructions: &native.design_placement_constructions,
                body_bindings: &native.design_body_bindings,
                component_naming_spaces: &native.design_component_naming_spaces,
                histories: &native.asm_histories,
                face_boundaries: &native.asm_face_boundaries,
                sheet_metal_settings: &native.design_sheet_metal_settings,
                construction_recipes: &native.construction_recipes,
                persistent_subentity_tags: &native.persistent_subentity_tags,
            },
        )?
        .parameters,
    )
}

/// The parameter records of `source`, encoded.
pub(in crate::writer::generate) fn encode(
    source: &ParameterSource,
    native: &F3dNative,
    registry: &mut GeneratedDesignRegistry,
) -> Result<Vec<u8>, CodecError> {
    match source {
        ParameterSource::Native => {
            encode_document_parameters(&native.design_parameters, NATIVE_PARAMETER_COLLECTION)
        }
        ParameterSource::Neutral(plan) => {
            let indices = plan.allocate(registry)?;
            plan.encode(registry, &indices, None)
        }
    }
}

impl ParameterPlan {
    /// The record indices of the written parameters, allocated in
    /// `registry`, in written order.
    pub(in crate::writer::generate) fn allocate(
        &self,
        registry: &mut GeneratedDesignRegistry,
    ) -> Result<Vec<u32>, CodecError> {
        self.written.iter().map(|_| registry.allocate()).collect()
    }

    /// The user parameter records at `indices`, registered in `registry`,
    /// encoded with tails that name `collection`, or a collection index no
    /// other record uses.
    pub(in crate::writer::generate) fn encode(
        &self,
        registry: &mut GeneratedDesignRegistry,
        indices: &[u32],
        collection: Option<u32>,
    ) -> Result<Vec<u8>, CodecError> {
        if self.written.is_empty() {
            return Ok(Vec::new());
        }
        let class_tag = registry.register(super::types::PARAMETER, indices)?;
        let collection = match collection {
            Some(collection) => collection,
            None => registry.allocate()?,
        };
        let records = self
            .written
            .iter()
            .zip(indices.iter().copied())
            .enumerate()
            .map(|(ordinal, (parameter, record_index))| {
                parameter.record(class_tag.clone(), record_index, ordinal)
            })
            .collect::<Result<Vec<_>, _>>()?;
        encode_document_parameters(&records, collection)
    }

    /// The ids of the written parameters.
    pub(in crate::writer::generate) fn written(&self) -> impl Iterator<Item = &ParameterId> {
        self.written.iter().map(|parameter| &parameter.source)
    }

    pub(in crate::writer::generate) fn into_losses(self) -> Vec<LossNote> {
        self.losses
    }
}

impl PlannedParameter {
    /// The native record. Its offsets are synthetic and in field order: the
    /// encoder does not read them.
    fn record(
        &self,
        class_tag: crate::records::references::DesignClassTag,
        record_index: u32,
        ordinal: usize,
    ) -> Result<crate::records::parameters::DesignParameter, CodecError> {
        let source_ordinal = u32::try_from(ordinal)
            .ok()
            .and_then(|ordinal| ordinal.checked_add(1))
            .ok_or_else(|| CodecError::NotImplemented("F3D parameter count exceeds u32".into()))?;
        crate::records::parameters::DesignParameter::try_from(DesignParameterDraft {
            id: format!("f3d:generated:design-parameter#{record_index}"),
            byte_offset: 0,
            class_tag,
            record_index,
            source_ordinal,
            source: DesignParameterSource::User {
                source_kind: cadmpeg_core::nonblank_literal!("User Parameter"),
                family_discriminator: Some(Located {
                    value: DesignParameterDiscriminator::Code0,
                    offset: 22,
                }),
            },
            expression: self.expression.clone(),
            expression_offset: 36,
            source_kind_offset: 70,
            unit: self.unit.map(|unit| RecordedValue {
                value: unit.name().to_owned(),
                offset: 110,
            }),
            name: self.name.clone(),
            name_offset: 120,
            evaluated_value: self.evaluated,
            evaluated_value_offset: 150,
        })
        .and_then(|record| {
            record.with_comment(self.comment.clone().map(|comment| RecordedValue {
                value: comment,
                offset: 30,
            }))
        })
        .map_err(|error| {
            CodecError::malformed(format_args!("F3D parameter {}: {error}", self.source))
        })
    }
}

/// The parameter's cached value in canonical units.
fn cached(parameter: &DesignParameter) -> Option<f64> {
    match parameter.value.as_ref()? {
        ParameterValue::Number(value) => Some(value.get()),
        ParameterValue::Integer(value) => i32::try_from(*value).ok().map(f64::from),
        ParameterValue::Boolean(_) | ParameterValue::Text(_) => None,
    }
}

/// The Fusion text of a definition, with each reference written as `name`
/// gives it.
fn printed_expression<'a>(
    definition: &ParameterDefinition,
    name: &impl Fn(&ParameterId) -> Option<&'a str>,
) -> Option<String> {
    match definition {
        ParameterDefinition::Expression(Expression::Neutral(neutral)) => {
            expression::print(&neutral.0, name)
        }
        ParameterDefinition::Expression(Expression::Source {
            dialect: ExpressionDialect::Fusion,
            tokens,
        }) => expression::print_tokens(tokens, name),
        ParameterDefinition::Expression(Expression::Source { .. })
        | ParameterDefinition::Driven
        | ParameterDefinition::Assigned(_) => None,
    }
}

/// The parameters a definition references.
fn references(definition: &ParameterDefinition) -> Vec<&ParameterRef> {
    let mut out = Vec::new();
    match definition {
        ParameterDefinition::Expression(Expression::Neutral(neutral)) => {
            expression::references(&neutral.0, &mut out);
        }
        ParameterDefinition::Expression(Expression::Source { tokens, .. }) => {
            out.extend(tokens.iter().filter_map(|token| match token {
                ExpressionToken::Reference(reference) => Some(reference),
                ExpressionToken::Text(_) => None,
            }));
        }
        ParameterDefinition::Driven | ParameterDefinition::Assigned(_) => {}
    }
    out
}

/// The candidates in the stable topological order of their references, ties
/// by arena order. A cycle is broken at its member of lowest arena order,
/// which is written as its cached value (the indices in the second set).
fn dependency_order(candidates: &[(&DesignParameter, Form)]) -> (Vec<usize>, BTreeSet<usize>) {
    let index = candidates
        .iter()
        .enumerate()
        .map(|(position, (parameter, _))| (&parameter.id, position))
        .collect::<HashMap<_, _>>();
    let mut pending = candidates
        .iter()
        .enumerate()
        .map(|(position, (parameter, _))| {
            references(&parameter.definition)
                .into_iter()
                .filter_map(expression::referenced)
                .filter_map(|id| index.get(id).copied())
                .filter(|dependency| *dependency != position)
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    let mut done = vec![false; candidates.len()];
    let mut order = Vec::with_capacity(candidates.len());
    let mut forced = BTreeSet::new();
    while order.len() < candidates.len() {
        let ready =
            (0..candidates.len()).find(|&position| !done[position] && pending[position].is_empty());
        let Some(next) = ready else {
            let Some(broken) = (0..candidates.len()).find(|&position| !done[position]) else {
                break;
            };
            forced.insert(broken);
            pending[broken].clear();
            continue;
        };
        done[next] = true;
        order.push(next);
        for dependencies in &mut pending {
            dependencies.remove(&next);
        }
    }
    (order, forced)
}

/// Legal and unique Fusion names of the candidates, in arena order.
struct Names(Vec<String>);

impl Names {
    fn new(candidates: &[(&DesignParameter, Form)]) -> Self {
        let mut taken = BTreeSet::new();
        let names = candidates
            .iter()
            .enumerate()
            .map(|(position, (parameter, _))| {
                let base = parameter.name.as_ref().map_or_else(
                    || format!("p{}", position + 1),
                    |name| legal_name(name.as_str()),
                );
                let name = std::iter::once(base.clone())
                    .chain((2..=candidates.len() + 1).map(|suffix| format!("{base}_{suffix}")))
                    .find(|name| !taken.contains(name))
                    .unwrap_or(base);
                taken.insert(name.clone());
                name
            })
            .collect();
        Self(names)
    }

    fn of(&self, position: usize) -> &str {
        &self.0[position]
    }
}

/// `name` as a Fusion identifier: ASCII letters, digits and `_`, not
/// starting with a digit.
fn legal_name(name: &str) -> String {
    let mut legal = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if legal.starts_with(|character: char| character.is_ascii_digit()) {
        legal.insert(0, '_');
    }
    legal
}
