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
//! (`f3d:writer.integer-as-real`). A parameter that closes a cycle of
//! references is written as its cached value
//! (`export:dependency.cycle-broken`). A Fusion user parameter holds no
//! tolerance, so a fit is written as none (`export:fit.dropped`), and so is
//! the general tolerance of a `General` parameter
//! (`export:general_tolerance.dropped`).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    dependency_cycle_broken, fit_dropped, general_tolerance_dropped, parameter_flattened,
    parameter_renamed, record_residual_unavailable, value_unwritable,
};
use cadmpeg_ir::design::dimension::{Dimension, ParameterRef};
use cadmpeg_ir::design::expr::{Expression, ExpressionDialect, ExpressionToken};
use cadmpeg_ir::design::options::NativeOption;
use cadmpeg_ir::design::parameter::{
    DesignParameter, ParameterDefinition, ParameterOwner, ParameterValue, Tolerance,
};
use cadmpeg_ir::design::units::{Quantity, Unit, UnitSymbol, ValueKind};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::report::loss::LossNote;

use super::expression;
use crate::loss::F3dLossCode;
use crate::native::F3dNative;
use crate::records::configuration::DesignConfigurationKind;
use crate::records::identity::{Located, RecordedValue};
use crate::records::parameters::{
    DesignParameter as NativeParameter, DesignParameterDiscriminator, DesignParameterDraft,
    DesignParameterSource,
};
use crate::writer::generate::configuration_table::{DestinationParameter, DestinationParameters};
use crate::writer::generate::presentation::GeneratedDesignRegistry;
use crate::writer::generate::records::{
    encode_document_parameters, ParameterRecords, UserParameter,
};
use crate::writer::generate::reduce::named_loss;

/// The parameter collection the native path's parameter tails name.
const NATIVE_PARAMETER_COLLECTION: u32 = 19;
/// The name stem of a new owned parameter (f3d.md §3.1, "Owned
/// parameters": `d<n>`, with `n` its source ordinal).
const OWNED_STEM: &str = "d";

/// The parameters written as Fusion user parameters, and the losses taken.
#[derive(Clone)]
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
#[derive(Clone)]
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
        let canonical = expression::evaluate(&node, &|_| None).filter(|value| value.is_finite())?;
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
#[derive(Clone)]
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
#[derive(Clone)]
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
    /// The native records: the neutral parameters equal their projection,
    /// up to the edited expressions and values.
    Native(NativeParameters),
    /// The design scope's parameters: the neutral parameters were edited.
    Neutral(ParameterPlan),
}

/// The expression and the value in database units that a neutral parameter
/// gives each edited native record, keyed by record id.
type ValueEdits = BTreeMap<String, (String, f64)>;

/// The native parameter records the native path writes.
pub(in crate::writer::generate) struct NativeParameters {
    /// The records in their user form, encoded.
    records: ParameterRecords,
    edits: ValueEdits,
    /// The losses of the user form.
    losses: Vec<LossNote>,
}

impl NativeParameters {
    pub(in crate::writer::generate) fn destinations<'a>(
        &'a self,
        native: &'a F3dNative,
    ) -> DestinationParameters<'a> {
        native
            .design_parameters
            .iter()
            .map(|record| {
                let (expression, evaluated) = written_value(record, &self.edits);
                (
                    crate::ids::neutral_parameter_id(record),
                    DestinationParameter {
                        index: record.record_index,
                        name: record.name(),
                        expression,
                        evaluated,
                    },
                )
            })
            .collect()
    }

    pub(in crate::writer::generate) fn into_losses(self) -> Vec<LossNote> {
        self.losses
    }
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

    /// The unit of the canonical value, in which every finite value has a
    /// finite literal.
    fn canonical_unit(self) -> Option<UnitSymbol> {
        match self {
            Self::Length => Some(UnitSymbol::Mm),
            Self::Angle => Some(UnitSymbol::Rad),
            Self::Real => None,
        }
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
    in_owner_order(&mut candidates);
    in_owner_order(&mut driven_candidates);
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
    let mut flattened_dependents = Vec::new();
    let mut unwritten_dependents = Vec::new();
    let mut unbounded = Vec::new();
    let mut restated = Vec::new();
    let mut renamed = Vec::new();
    let mut unbound = Vec::new();
    let mut displayed = Vec::new();
    let mut integers = Vec::new();
    let mut broken = Vec::new();
    let mut fits = Vec::new();
    let mut general = false;
    for index in order {
        let (parameter, form) = candidates[index];
        let mut unit = form.unit(parameter.unit.as_ref());
        let name_of = |id: &ParameterId| written_names.get(id).copied();
        let printed = if forced.contains(&index) {
            broken.push(parameter.id.to_string());
            None
        } else {
            printed_expression(&parameter.definition, &name_of)
        };
        let dependent = printed.is_none()
            && !forced.contains(&index)
            && references(&parameter.definition)
                .into_iter()
                .filter_map(expression::referenced)
                .any(|id| !written_names.contains_key(id));
        let canonical = cached(parameter).or_else(|| match (&printed, &parameter.definition) {
            (Some(_), ParameterDefinition::Expression(Expression::Neutral(neutral))) => {
                expression::evaluate(&neutral.0, &|id| written_values.get(id).copied())
            }
            _ => None,
        });
        let Some(canonical) = canonical else {
            if dependent {
                unwritten_dependents.push(parameter.id.to_string());
            } else {
                unwritten.push(parameter.id.to_string());
            }
            continue;
        };
        if !canonical.is_finite() {
            unbounded.push(parameter.id.to_string());
            continue;
        }
        let printed_expression = printed.is_some();
        let text = match printed {
            Some(text) => text,
            None => {
                let text = if let Some(text) = expression::quantity_literal(canonical, unit) {
                    text
                } else {
                    let Some(text) = expression::quantity_literal(canonical, form.canonical_unit())
                    else {
                        unwritten.push(parameter.id.to_string());
                        continue;
                    };
                    unit = form.canonical_unit();
                    restated.push(parameter.id.to_string());
                    text
                };
                if dependent {
                    flattened_dependents.push(parameter.id.to_string());
                } else {
                    flattened.push(parameter.id.to_string());
                }
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
        match parameter.tolerance {
            Some(Tolerance::Fit { .. }) => fits.push(parameter.id.to_string()),
            Some(Tolerance::General) => general = true,
            _ => {}
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
    losses.extend(parameter_flattened(
        "F3D",
        "states these parameters by their cached values: their expressions reference parameters the file does not write",
        flattened_dependents,
    ));
    losses.extend(parameter_flattened(
        "F3D",
        "omits these parameters: their expressions reference parameters the file does not write, and they hold no cached value",
        unwritten_dependents,
    ));
    losses.extend(value_unwritable(
        "F3D",
        "omits these parameters: their expressions evaluate to no finite number, and a Fusion parameter holds its value",
        unbounded,
    ));
    losses.extend(value_unwritable(
        "F3D",
        "writes these parameters in their canonical unit: their value in their own unit is no finite number",
        restated,
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
    losses.extend(dependency_cycle_broken(
        "F3D",
        "writes these parameters as their cached values, each the edge that closes a cycle of parameter references, which Fusion cannot hold",
        broken,
    ));
    losses.extend(fit_dropped(
        "F3D",
        "writes these parameters without their fits: a Fusion user parameter holds no tolerance",
        fits,
    ));
    losses.extend(general_tolerance_dropped(
        "F3D",
        "writes no general tolerance for the General parameters of this definition: a Fusion user parameter holds no tolerance",
        general.then(|| definition.to_string()),
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

/// The native records when they cover the configured parameters and the
/// neutral parameters match their projection up to expression and value edits.
/// Otherwise write the root scope's parameters.
pub(in crate::writer::generate) fn native_or_neutral(
    target: &CadIr,
    native: &F3dNative,
    neutral: &[cadmpeg_ir::features::parameters::DesignParameter],
    projected: &[cadmpeg_ir::features::parameters::DesignParameter],
) -> Result<ParameterSource, CodecError> {
    let root = super::root_scope(target);
    let mut parameter_plan = if let Some(edits) = value_edits(neutral, projected, native) {
        if let Some(configured) = root
            .filter(|_| {
                native
                    .design_configurations
                    .iter()
                    .any(|document| document.kind() == DesignConfigurationKind::Table)
            })
            .and_then(|root| configured_parameter_plan(target, &root.definition, projected))
        {
            configured
        } else {
            let (form, losses) = user_form(native, &edits)?;
            let records = encode_document_parameters(&form, NATIVE_PARAMETER_COLLECTION)?;
            return Ok(ParameterSource::Native(NativeParameters {
                records,
                edits,
                losses,
            }));
        }
    } else {
        root.map_or_else(
            || ParameterPlan {
                written: Vec::new(),
                claimed: Vec::new(),
                owned: Vec::new(),
                driven: Vec::new(),
                losses: Vec::new(),
            },
            |scope| plan(target, &scope.definition),
        )
    };
    let restated = super::restated_parameters(target);
    parameter_plan.losses.extend(named_loss(
        F3dLossCode::WriterParameterOmitted,
        "parameter",
        "the root scope states no design parameter that restates them",
        neutral
            .iter()
            .map(|row| row.id.to_string())
            .filter(|id| !restated.contains(id.as_str())),
    ));
    Ok(ParameterSource::Neutral(parameter_plan))
}

/// The neutral plan when it supplies a configured parameter with no native
/// placement.
fn configured_parameter_plan(
    target: &CadIr,
    scope: &ProductDefinitionId,
    projected: &[cadmpeg_ir::features::parameters::DesignParameter],
) -> Option<ParameterPlan> {
    let configured = target
        .design
        .configurations
        .iter()
        .filter(|row| row.scope == *scope)
        .flat_map(|row| row.overrides.parameters.keys())
        .collect::<BTreeSet<_>>();
    if configured.is_empty() {
        return None;
    }
    let native = projected
        .iter()
        .map(|parameter| &parameter.id)
        .collect::<BTreeSet<_>>();
    if configured.is_subset(&native) {
        return None;
    }
    let plan = plan(target, scope);
    plan.written
        .iter()
        .any(|parameter| {
            configured.contains(&parameter.source) && !native.contains(&parameter.source)
        })
        .then_some(plan)
}

/// The expression and the value in database units of each native record
/// whose neutral parameter `neutral` edits, keyed by record id, when
/// `neutral` equals the native records' projection in everything else.
fn value_edits(
    neutral: &[cadmpeg_ir::features::parameters::DesignParameter],
    projection: &[cadmpeg_ir::features::parameters::DesignParameter],
    native: &F3dNative,
) -> Option<ValueEdits> {
    use cadmpeg_ir::features::parameters::ParameterValue;

    if neutral == projection {
        return Some(ValueEdits::new());
    }
    if neutral.len() != projection.len() {
        return None;
    }
    let records = native
        .design_parameters
        .iter()
        .map(|record| (crate::ids::neutral_parameter_id(record), record))
        .collect::<HashMap<_, _>>();
    let mut edits = BTreeMap::new();
    for (stated, projected) in neutral.iter().zip(projection) {
        if stated == projected {
            continue;
        }
        let mut edited = projected.clone();
        edited.expression.clone_from(&stated.expression);
        edited.value.clone_from(&stated.value);
        let evaluated = match &stated.value {
            Some(ParameterValue::Length(length)) => {
                length.get() / crate::design::feature_project::DESIGN_LENGTH_TO_MILLIMETRE
            }
            Some(ParameterValue::Angle(angle)) => angle.get(),
            Some(ParameterValue::Real(real)) => real.get(),
            _ => return None,
        };
        let record = records.get(&stated.id)?;
        if edited != *stated || stated.expression.trim().is_empty() {
            return None;
        }
        edits.insert(record.id.clone(), (stated.expression.clone(), evaluated));
    }
    Some(edits)
}

/// The native records' features and parameters as the decoder projects them.
pub(in crate::writer::generate) fn native_projection(
    decode_ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    native: &F3dNative,
) -> Result<crate::design::feature_project::ProjectedParameterDesign, CodecError> {
    let foot_origin =
        crate::design::decode::placement_construction::work_planes::foot_origin_placements(
            &native.design_sketch_placements,
            &native.design_parameter_scopes,
            &native.design_placement_constructions,
        );
    crate::design::feature_project::project_parameter_design_with_edge_identities(
        decode_ctx,
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
            foot_origin: Some(&foot_origin),
            placement_constructions: &native.design_placement_constructions,
            body_bindings: &native.design_body_bindings,
            component_naming_spaces: &native.design_component_naming_spaces,
            histories: &native.asm_histories,
            face_boundaries: &native.asm_face_boundaries,
            sheet_metal_settings: &native.design_sheet_metal_settings,
            construction_recipes: &native.construction_recipes,
            persistent_subentity_tags: &native.persistent_subentity_tags,
        },
    )
}

/// The native parameter records in the user form the Design stream writes,
/// with the expressions and values of `edits`, and the losses of that form.
/// The stream writes no parameter owner, so an owned parameter becomes a user
/// parameter at the next source ordinal after those of the user parameters.
/// A record that stores no family discriminator takes the one the other
/// records share, or 0 where they share none.
fn user_form<'a>(
    native: &'a F3dNative,
    edits: &'a ValueEdits,
) -> Result<(Vec<UserParameter<'a>>, Vec<LossNote>), CodecError> {
    let parameters = &native.design_parameters;
    let mut stored = parameters.iter().filter_map(|parameter| {
        parameter
            .family_discriminator()
            .map(|located| located.value)
    });
    let first = stored.next();
    let shared = match first {
        Some(first) if stored.all(|value| value == first) => first,
        _ => DesignParameterDiscriminator::Code0,
    };
    let mut ordinal = parameters
        .iter()
        .filter(|parameter| matches!(parameter.source(), DesignParameterSource::User { .. }))
        .map(|parameter| parameter.source_ordinal)
        .max()
        .unwrap_or(0);
    let mut owned = Vec::new();
    let mut undiscriminated = Vec::new();
    let mut written = Vec::with_capacity(parameters.len());
    for record in parameters {
        let source_ordinal = match record.source() {
            DesignParameterSource::User { .. } => record.source_ordinal,
            DesignParameterSource::Owned(_) => {
                owned.push(record.id.clone());
                ordinal = ordinal.checked_add(1).ok_or_else(|| {
                    CodecError::NotImplemented("F3D parameter ordinal exceeds u32".into())
                })?;
                ordinal
            }
        };
        let family_discriminator = match record.family_discriminator() {
            Some(located) => located.value,
            None => {
                undiscriminated.push(record.id.clone());
                shared
            }
        };
        let (expression, evaluated) = written_value(record, edits);
        written.push(UserParameter {
            record,
            family_discriminator,
            source_ordinal,
            expression,
            evaluated,
        });
    }
    let losses = value_unwritable(
        "F3D",
        "writes these owned parameters as user parameters, each at the next source ordinal after those of the user parameters: the written Design stream holds no parameter owner",
        owned,
    )
    .into_iter()
    .chain(record_residual_unavailable(
        "F3D",
        &format!(
            "writes these parameters, whose records store no family discriminator, with family discriminator {}: the value the other parameter records share, or 0 where they share none",
            shared.code()
        ),
        undiscriminated,
    ))
    .collect();
    Ok((written, losses))
}

fn written_value<'a>(record: &'a NativeParameter, edits: &'a ValueEdits) -> (&'a str, f64) {
    edits.get(&record.id).map_or_else(
        || (record.expression(), record.evaluated_value().get()),
        |(expression, evaluated)| (expression.as_str(), *evaluated),
    )
}

/// The parameter records of `source`, encoded. It takes the encoded native
/// records out of `source`.
pub(in crate::writer::generate) fn encode(
    source: &mut ParameterSource,
    registry: &mut GeneratedDesignRegistry,
) -> Result<ParameterRecords, CodecError> {
    match source {
        ParameterSource::Native(parameters) => Ok(std::mem::take(&mut parameters.records)),
        ParameterSource::Neutral(plan) => {
            let indices = plan.allocate(registry)?;
            plan.encode(registry, &indices, None)
        }
    }
}

impl ParameterPlan {
    /// The records and names assigned to source parameters in the new stream.
    pub(in crate::writer::generate) fn destinations(
        &self,
        indices: &[u32],
    ) -> DestinationParameters<'_> {
        self.written
            .iter()
            .zip(indices)
            .map(|(parameter, &index)| {
                (
                    parameter.source.clone(),
                    DestinationParameter {
                        index,
                        name: &parameter.name,
                        expression: &parameter.expression,
                        evaluated: parameter.evaluated,
                    },
                )
            })
            .collect()
    }

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
    ) -> Result<ParameterRecords, CodecError> {
        if self.written.is_empty() {
            return Ok(ParameterRecords::default());
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
        let records = records
            .iter()
            .map(|record| UserParameter {
                record,
                family_discriminator: DesignParameterDiscriminator::Code0,
                source_ordinal: record.source_ordinal,
                expression: record.expression(),
                evaluated: record.evaluated_value().get(),
            })
            .collect::<Vec<_>>();
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
            let record = UserParameter {
                record: &record,
                family_discriminator: discriminator,
                source_ordinal: record.source_ordinal,
                expression: record.expression(),
                evaluated: record.evaluated_value().get(),
            };
            out.bytes
                .extend(encode_document_parameters(&[record], collection)?.bytes);
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

    /// The ids of the parameters a written record restates: the written
    /// user parameters, those written features own, and the driven
    /// parameters a written dimension binds.
    pub(in crate::writer::generate) fn restated(&self) -> BTreeSet<ParameterId> {
        self.written
            .iter()
            .chain(&self.claimed)
            .map(|parameter| parameter.source.clone())
            .chain(
                self.driven
                    .iter()
                    .filter(|parameter| parameter.taken)
                    .map(|parameter| parameter.source.clone()),
            )
            .collect()
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

/// Orders `candidates` by the first place of their owner in arena order, then
/// by their stated positions, then by id.
fn in_owner_order(candidates: &mut [(&DesignParameter, Form)]) {
    let mut first = HashMap::new();
    for (place, (parameter, _)) in candidates.iter().enumerate() {
        first.entry(owner_key(parameter)).or_insert(place);
    }
    candidates.sort_by_cached_key(|(parameter, _)| {
        (
            first[&owner_key(parameter)],
            parameter.position.is_none(),
            parameter.position,
            parameter.id.clone(),
        )
    });
}

/// The id of `parameter`'s owner.
fn owner_key(parameter: &DesignParameter) -> Option<&str> {
    parameter.owner.as_ref().map(|owner| match owner {
        ParameterOwner::Feature(id) => id.as_str(),
        ParameterOwner::Container(id) => id.as_str(),
        ParameterOwner::Attribute(id) => id.as_str(),
    })
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
