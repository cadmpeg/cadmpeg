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
use cadmpeg_ir::design::dimension::{Dimension, ParameterRef};
use cadmpeg_ir::design::expr::{Expression, ExpressionDialect, ExpressionToken};
use cadmpeg_ir::design::options::NativeOption;
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
/// The name stem of a new owned parameter (f3d.md §3.1, "Owned
/// parameters": `d<n>`, with `n` its source ordinal).
const OWNED_STEM: &str = "d";

/// The parameters written as Fusion user parameters, and the losses taken.
pub(in crate::writer::generate) struct ParameterPlan {
    written: Vec<PlannedParameter>,
    /// The parameters taken out of the user parameters by the feature that
    /// owns them.
    claimed: Vec<PlannedParameter>,
    /// The names of the new parameters written features own.
    owned: Vec<String>,
    /// The driven parameters, which only the dimension that binds them can
    /// write.
    driven: Vec<DrivenParameter>,
    losses: Vec<LossNote>,
}

/// A `Driven` parameter (the parameter of a driven dimension).
struct DrivenParameter {
    source: ParameterId,
    name: String,
    form: Form,
    unit: Option<UnitSymbol>,
    /// The cached value in Fusion's database units.
    evaluated: Option<f64>,
    literal: Option<RecordLiteral>,
    comment: Option<String>,
    unheld: Unheld,
    /// Whether a written dimension owns it.
    taken: bool,
}

/// The literal the record of a driven dimension's parameter stores (the
/// source setting `f3d:literal`), which can differ from its measured value
/// (f3d.md §3.1, "Sketch dimensions").
#[derive(Clone)]
pub(super) struct RecordLiteral {
    pub(super) text: String,
    /// Its value in Fusion's database units.
    pub(super) value: f64,
}

impl RecordLiteral {
    /// The literal of `parameter`'s source setting `f3d:literal`, where it is
    /// one number with a unit of the parameter's quantity.
    fn of(parameter: &DesignParameter, form: Form) -> Option<Self> {
        let Some(NativeOption::Text(Dimension::Literal(text))) = parameter
            .source_settings
            .get(crate::design_v11::parameters::LITERAL)
        else {
            return None;
        };
        let node = crate::design_v11::parameters::literal(text.as_str(), &parameter.kind)?;
        let canonical = expression::evaluate(&node, &|_| None)?;
        Some(Self {
            text: text.as_str().to_owned(),
            value: form.database(canonical),
        })
    }
}

/// A driven parameter that a dimension can own.
pub(super) struct Driven {
    index: usize,
    /// The cached value in Fusion's database units.
    pub(super) evaluated: Option<f64>,
    /// The literal its source record stores, which its record keeps.
    pub(super) literal: Option<RecordLiteral>,
    pub(super) unit: UnitSymbol,
}

/// What the record of a written parameter does not hold of its source, each
/// an entry of its loss: a source name other than the written one, a
/// container or attribute owner, and a display presentation.
struct Unheld {
    renamed: Option<String>,
    unbound: Option<String>,
    displayed: Option<String>,
}

impl Unheld {
    fn of(parameter: &DesignParameter, name: &str) -> Self {
        let source_name = parameter
            .name
            .as_ref()
            .map(cadmpeg_core::text::NonBlankString::as_str);
        Self {
            renamed: (source_name != Some(name)).then(|| {
                format!(
                    "{} ({} -> `{name}`)",
                    parameter.id,
                    source_name.map_or_else(|| "no name".to_owned(), |name| format!("`{name}`")),
                )
            }),
            unbound: match &parameter.owner {
                Some(ParameterOwner::Container(container)) => {
                    Some(format!("{} (owner container {container})", parameter.id))
                }
                Some(ParameterOwner::Attribute(attribute)) => {
                    Some(format!("{} (owner attribute {attribute})", parameter.id))
                }
                Some(ParameterOwner::Feature(_)) | None => None,
            },
            displayed: parameter
                .display
                .map(|display| format!("{} ({display:?})", parameter.id)),
        }
    }
}

/// A parameter a written feature owns, as its record holds it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct OwnedParameter {
    pub(super) name: String,
    pub(super) expression: String,
    pub(super) unit: UnitSymbol,
    /// The value in Fusion's database units: cm for a length, rad for an
    /// angle.
    pub(super) value: f64,
    /// The source ordinal, which the owner also stores as its parameter
    /// number.
    pub(super) ordinal: u32,
    /// Whether the expression names another parameter.
    pub(super) names_parameter: bool,
    pub(super) comment: Option<String>,
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
    /// The parameters its definition names.
    references: Vec<ParameterId>,
    /// Whether the written expression names a parameter.
    names_parameter: bool,
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

    /// A value in Fusion's database units in canonical units.
    fn canonical(self, database: f64) -> f64 {
        match self {
            Self::Length => database * 10.0,
            Self::Angle | Self::Real => database,
        }
    }

    fn quantity(self) -> Quantity {
        match self {
            Self::Length => Quantity::LENGTH,
            Self::Angle => Quantity::ANGLE,
            Self::Real => Quantity::REAL,
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
    let mut driven_candidates = Vec::new();
    let mut candidates = Vec::new();
    for parameter in parameters {
        match (Form::of(&parameter.kind), &parameter.definition) {
            (None, _) => omitted_kind.push(parameter.id.to_string()),
            (Some(form), ParameterDefinition::Driven) => driven_candidates.push((parameter, form)),
            (Some(form), _) => candidates.push((parameter, form)),
        }
    }
    // The driven parameters are named after the others, so that their names
    // change none of those.
    let names = Names::new(
        &candidates
            .iter()
            .chain(&driven_candidates)
            .copied()
            .collect::<Vec<_>>(),
    );
    let driven = driven_candidates
        .iter()
        .enumerate()
        .map(|(position, (parameter, form))| {
            let name = names.of(candidates.len() + position);
            DrivenParameter {
                source: parameter.id.clone(),
                name: name.to_owned(),
                form: *form,
                unit: form.unit(parameter.unit.as_ref()),
                evaluated: cached(parameter).map(|value| form.database(value)),
                literal: RecordLiteral::of(parameter, *form),
                comment: parameter
                    .comment
                    .clone()
                    .filter(|comment| !comment.is_empty()),
                unheld: Unheld::of(parameter, name),
                taken: false,
            }
        })
        .collect();
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
        let printed_expression = printed.is_some();
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
        let unheld = Unheld::of(parameter, name);
        renamed.extend(unheld.renamed);
        unbound.extend(unheld.unbound);
        displayed.extend(unheld.displayed);
        if matches!(parameter.kind, ValueKind::Integer) {
            integers.push(parameter.id.to_string());
        }
        let references = references(&parameter.definition)
            .into_iter()
            .filter_map(expression::referenced)
            .cloned()
            .collect::<Vec<_>>();
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
            names_parameter: printed_expression && !references.is_empty(),
            references,
        });
    }
    let mut losses = Vec::new();
    losses.extend(named_loss(
        F3dLossCode::WriterParameterOmitted,
        "parameter",
        "a Fusion user parameter holds only a length, an angle or a number",
        omitted_kind,
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
    ParameterPlan {
        written,
        claimed: Vec::new(),
        owned: Vec::new(),
        driven,
        losses,
    }
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
            claimed: Vec::new(),
            owned: Vec::new(),
            driven: Vec::new(),
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
) -> Result<Vec<cadmpeg_ir::features::parameters::DesignParameter>, CodecError> {
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
                parameter.record(
                    class_tag.clone(),
                    record_index,
                    ordinal,
                    DesignParameterDiscriminator::Code0,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        encode_document_parameters(&records, collection)
    }

    /// The user parameter records of class `class_tag` at `indices`, each
    /// with family discriminator `discriminator` and a tail that names
    /// `collection`, as records of `out`.
    pub(in crate::writer::generate) fn roster_records(
        &self,
        class_tag: &crate::records::references::DesignClassTag,
        indices: &[u32],
        collection: u32,
        discriminator: DesignParameterDiscriminator,
        out: &mut super::frames::DesignRecords,
    ) -> Result<(), CodecError> {
        for (ordinal, (parameter, record_index)) in
            self.written.iter().zip(indices.iter().copied()).enumerate()
        {
            let record =
                parameter.record(class_tag.clone(), record_index, ordinal, discriminator)?;
            out.primaries.push((record_index, out.bytes.len()));
            out.bytes
                .extend(encode_document_parameters(&[record], collection)?);
        }
        Ok(())
    }

    /// The number of written parameters.
    pub(in crate::writer::generate) fn written_count(&self) -> usize {
        self.written.len()
    }

    /// The ids of the written parameters.
    pub(in crate::writer::generate) fn written(&self) -> impl Iterator<Item = &ParameterId> {
        self.written.iter().map(|parameter| &parameter.source)
    }

    pub(in crate::writer::generate) fn into_losses(self) -> Vec<LossNote> {
        let mut losses = self.losses;
        let (taken, untaken): (Vec<_>, Vec<_>) = self
            .driven
            .into_iter()
            .partition(|parameter| parameter.taken);
        losses.extend(named_loss(
            F3dLossCode::WriterParameterOmitted,
            "parameter",
            "a driven parameter is written as the parameter of the dimension it drives, and no written dimension binds these",
            untaken.iter().map(|parameter| parameter.source.to_string()),
        ));
        losses.extend(parameter_renamed(
            "F3D",
            "renames these driven parameters to legal, unique Fusion names",
            taken
                .iter()
                .filter_map(|parameter| parameter.unheld.renamed.clone()),
        ));
        losses.extend(value_unwritable(
            "F3D",
            "writes these driven parameters as parameters of their dimensions without their owners: the parameter of a Fusion dimension is bound to no container or attribute",
            taken
                .iter()
                .filter_map(|parameter| parameter.unheld.unbound.clone()),
        ));
        losses.extend(value_unwritable(
            "F3D",
            "writes these driven parameters without their diameter or radius display presentation: the record of a Fusion parameter holds no display flag",
            taken
                .iter()
                .filter_map(|parameter| parameter.unheld.displayed.clone()),
        ));
        losses
    }

    /// The names of the written parameters, user, owned and driven.
    pub(super) fn names(&self) -> impl Iterator<Item = &str> {
        self.written
            .iter()
            .chain(&self.claimed)
            .map(|parameter| parameter.name.as_str())
            .chain(self.owned.iter().map(String::as_str))
            .chain(self.driven.iter().map(|parameter| parameter.name.as_str()))
    }

    /// The driven parameter `id` for a dimension of `quantity` to own;
    /// `None` where `id` is no driven parameter, and the reason where the
    /// dimension cannot own it.
    pub(super) fn driven(
        &self,
        id: &ParameterId,
        quantity: Quantity,
    ) -> Result<Option<Driven>, &'static str> {
        let Some(index) = self
            .driven
            .iter()
            .position(|parameter| &parameter.source == id)
        else {
            return Ok(None);
        };
        let parameter = &self.driven[index];
        if parameter.taken {
            return Err("an earlier dimension owns its driven parameter");
        }
        let unit = parameter
            .unit
            .filter(|_| parameter.form.quantity() == quantity)
            .ok_or("its driven parameter is of another quantity")?;
        Ok(Some(Driven {
            index,
            evaluated: parameter.evaluated,
            literal: parameter.literal.clone(),
            unit,
        }))
    }

    /// `driven` as the parameter of the dimension that owns it, with source
    /// ordinal `ordinal`, the literal `expression` and `value` in Fusion's
    /// database units.
    pub(super) fn take_driven(
        &mut self,
        driven: &Driven,
        expression: String,
        value: f64,
        ordinal: u32,
    ) -> OwnedParameter {
        let parameter = &mut self.driven[driven.index];
        parameter.taken = true;
        OwnedParameter {
            name: parameter.name.clone(),
            expression,
            unit: driven.unit,
            value,
            ordinal,
            names_parameter: false,
            comment: parameter.comment.clone(),
        }
    }

    /// The written parameter `id`, user or claimed.
    fn planned(&self, id: &ParameterId) -> Option<&PlannedParameter> {
        self.written
            .iter()
            .chain(&self.claimed)
            .find(|parameter| &parameter.source == id)
    }

    /// The written name of parameter `id`.
    pub(super) fn name(&self, id: &ParameterId) -> Option<&str> {
        self.planned(id).map(|parameter| parameter.name.as_str())
    }

    /// The value of the written parameter `id` of `quantity`, in Fusion's
    /// database units.
    pub(super) fn value(&self, id: &ParameterId, quantity: Quantity) -> Option<f64> {
        self.planned(id)
            .filter(|parameter| {
                parameter
                    .unit
                    .is_some_and(|unit| unit.canonical().1 == quantity)
            })
            .map(|parameter| parameter.evaluated)
    }

    /// The value of the written length parameter `id`, in mm.
    pub(super) fn length(&self, id: &ParameterId) -> Option<f64> {
        self.value(id, Quantity::LENGTH)
            .map(|value| Form::Length.canonical(value))
    }

    /// The user parameter `id` of `quantity` as a parameter of the feature
    /// that owns it, with source ordinal `ordinal`, taken out of the user
    /// parameters; `None` where it is no user parameter of `quantity` or a
    /// written parameter names it.
    pub(super) fn claim(
        &mut self,
        id: &ParameterId,
        quantity: Quantity,
        ordinal: u32,
    ) -> Option<OwnedParameter> {
        let position = self
            .written
            .iter()
            .position(|parameter| &parameter.source == id)?;
        let unit = self.written[position]
            .unit
            .filter(|unit| unit.canonical().1 == quantity)?;
        if self
            .written
            .iter()
            .chain(&self.claimed)
            .any(|parameter| parameter.references.contains(id))
        {
            return None;
        }
        let parameter = self.written.remove(position);
        let owned = OwnedParameter {
            name: parameter.name.clone(),
            expression: parameter.expression.clone(),
            unit,
            value: parameter.evaluated,
            ordinal,
            names_parameter: parameter.names_parameter,
            comment: parameter.comment.clone(),
        };
        self.claimed.push(parameter);
        Some(owned)
    }

    /// A new parameter a feature owns, named `d<ordinal>` made unique among
    /// the written parameters.
    pub(super) fn owned(
        &mut self,
        ordinal: u32,
        expression: String,
        unit: UnitSymbol,
        value: f64,
        names_parameter: bool,
    ) -> OwnedParameter {
        let base = format!("{OWNED_STEM}{ordinal}");
        let taken = self.names().map(str::to_owned).collect::<BTreeSet<_>>();
        let name = std::iter::once(base.clone())
            .chain((2..=taken.len() + 2).map(|suffix| format!("{base}_{suffix}")))
            .find(|name| !taken.contains(name))
            .unwrap_or(base);
        self.owned.push(name.clone());
        OwnedParameter {
            name,
            expression,
            unit,
            value,
            ordinal,
            names_parameter,
            comment: None,
        }
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
        discriminator: DesignParameterDiscriminator,
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
                    value: discriminator,
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
                offset: 31,
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
        | ParameterDefinition::Assigned(_)
        | ParameterDefinition::Unset => None,
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
        ParameterDefinition::Driven
        | ParameterDefinition::Assigned(_)
        | ParameterDefinition::Unset => {}
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
