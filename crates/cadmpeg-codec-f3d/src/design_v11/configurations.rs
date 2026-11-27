// SPDX-License-Identifier: Apache-2.0
//! The configurations of a Fusion design (§5.11): one per row of its
//! configuration table.
//!
//! The model state the document stores is the state of the active row, so
//! the active row is the scope's active configuration and the base, and
//! states no overrides. Every other row states the expression of each
//! parameter its cells set, with the value the cell caches typed as the
//! parameter's value, and the suppression of each feature its cells toggle.
//! Its `Part Number` and `Description` property cells are the part number and
//! the description of the scope's definition (§5.11, Identity carriers); an
//! empty part-number cell is the configuration's name. A scope whose active
//! row does not resolve states no configurations, since a scope with
//! configurations names its active one (part 10). The rows of a document
//! with no scope of its own state no configuration. A loss names each other
//! property of a configuration the design states, each parameter cell whose
//! cached value the design does not state, and each cell whose target the
//! design does not state, the unresolved targets a row keeps as properties
//! among them.
//!
//! [`restate`] is the inverse: it writes what the configurations resolve
//! into the v10 rows of the configuration tables, from which the writer
//! writes the tables.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::codec::write::{edit_dropped, value_unwritable};
use cadmpeg_ir::design::configuration::{
    DefinitionOverride, DesignConfiguration, FeatureStateOverride, Overrides, ParameterOverride,
    PartNumber,
};
use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
use cadmpeg_ir::design::edit::{evaluate, Value};
use cadmpeg_ir::design::expr::{
    Expression, ExpressionDialect, ExpressionToken, Neutral, Node, UnaryOp,
};
use cadmpeg_ir::design::parameter::{DesignParameter, ParameterValue};
use cadmpeg_ir::design::units::{Quantity, Unit, ValueKind};
use cadmpeg_ir::document::Design;
use cadmpeg_ir::features::parameters::{
    ConfigurationEvaluation, ConfigurationFeatureState, DesignConfiguration as LegacyConfiguration,
    DesignParameter as LegacyParameter, ParameterValue as LegacyValue,
};
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{ConfigurationId, Feature as LegacyFeature, ParameterId};
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::products::ProductDefinition;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Angle, FiniteReal, Length};
use cadmpeg_ir::schema::EntityKind;

use super::parameters::{cache, expression};
use super::timeline::document_prefix;
use crate::loss::F3dLossCode;

/// The configurations of the scopes keyed by document prefix in `scopes`,
/// and each scope's active configuration.
pub(super) struct Configurations {
    pub(super) records: Vec<DesignConfiguration>,
    pub(super) active: BTreeMap<ProductDefinitionId, ConfigurationId>,
}

/// A document in `hosted`, or one `scopes` does not key, has no scope of its
/// own. `features` holds the ids of the scopes' features and `parameters`
/// their parameters. Each class of row or cell the design omits charges one
/// loss to `losses`.
pub(super) fn project(
    legacy: &[LegacyConfiguration],
    scopes: &BTreeMap<String, ProductDefinitionId>,
    hosted: &BTreeSet<String>,
    features: &BTreeSet<&str>,
    parameters: &[DesignParameter],
    losses: &mut Vec<LossNote>,
) -> Configurations {
    let mut out = Configurations {
        records: Vec::new(),
        active: BTreeMap::new(),
    };
    let mut unscoped = Vec::new();
    let mut placed = Vec::new();
    for row in legacy {
        let prefix = document_prefix(row.native_ref.as_deref().unwrap_or(row.id.as_str()));
        match scopes.get(prefix) {
            Some(scope) if !hosted.contains(prefix) => {
                if row.active {
                    out.active.insert(scope.clone(), row.id.clone());
                }
                placed.push((row, scope));
            }
            Some(_) | None => unscoped.push(row.id.as_str()),
        }
    }
    let mut inactive = Vec::new();
    let mut omitted = Omitted::default();
    for (row, scope) in placed {
        if !out.active.contains_key(scope) {
            inactive.push(row.id.as_str());
            continue;
        }
        let overrides = projected(row, scope, features, parameters, &mut omitted);
        out.records.push(DesignConfiguration {
            id: row.id.clone(),
            scope: scope.clone(),
            name: row.name.clone().unwrap_or_default(),
            parent: None,
            comment: None,
            overrides,
            active_arrangements: BTreeSet::new(),
            materials: Vec::new(),
            native_ref: row.native_ref.clone(),
        });
    }
    for (code, count, what, named) in [
        (
            F3dLossCode::ConfigurationReferencedDesignUnscoped,
            unscoped.len(),
            "configuration row(s) of documents with no design scope of their own stay v10 configurations",
            unscoped.join(" "),
        ),
        (
            F3dLossCode::ConfigurationScopeWithoutActiveRow,
            inactive.len(),
            "configuration row(s) of scopes whose active row does not resolve stay v10 configurations",
            inactive.join(" "),
        ),
        (
            F3dLossCode::ConfigurationOverrideUnstated,
            omitted.unstated.len(),
            "configuration cell(s) set a parameter or feature the design does not state, so their rows omit them",
            omitted.unstated.join("; "),
        ),
        (
            F3dLossCode::ConfigurationOverrideValueUnprojected,
            omitted.uncached.len(),
            "configuration cell(s) cache a value the design does not state, so their overrides state only the expression",
            omitted.uncached.join("; "),
        ),
        (
            F3dLossCode::ConfigurationPropertyUnprojected,
            omitted.properties.len(),
            "configuration propert(ies) the design does not state, so their rows omit them",
            omitted.properties.join("; "),
        ),
    ] {
        if count != 0 {
            losses.push(code.note(format!("{count} {what}: {named}")));
        }
    }
    out
}

/// The cells and properties of rows that the design omits: cells whose
/// target it does not state, parameter cells whose cached value it does not
/// state, and properties it does not state.
#[derive(Default)]
struct Omitted {
    unstated: Vec<String>,
    uncached: Vec<String>,
    properties: Vec<String>,
}

/// The overrides of `row` of `scope`: none for the active row, else those
/// of its cells and identity properties. Names what the design omits in
/// `omitted`.
fn projected(
    row: &LegacyConfiguration,
    scope: &ProductDefinitionId,
    features: &BTreeSet<&str>,
    parameters: &[DesignParameter],
    omitted: &mut Omitted,
) -> Overrides {
    let active = row.active;
    let mut overrides = if active {
        Overrides::default()
    } else {
        overrides(
            row,
            scope,
            features,
            parameters,
            &mut omitted.unstated,
            &mut omitted.uncached,
        )
    };
    for (key, text) in &row.properties {
        let named = format!("{key} in {}", row.id);
        if ["suppressed:", "parameter:"]
            .iter()
            .any(|prefix| key.as_str().starts_with(prefix))
        {
            omitted.unstated.push(named);
        } else if active || !identity(&mut overrides, scope, key.as_str(), text) {
            omitted.properties.push(named);
        }
    }
    overrides
}

/// States in `overrides` the identity field of `scope`'s definition that the
/// configuration property `key` sets to `text`; false where `key` sets none.
fn identity(overrides: &mut Overrides, scope: &ProductDefinitionId, key: &str, text: &str) -> bool {
    let definition = overrides.definitions.entry(scope.clone());
    match key {
        "property:Part Number" => {
            definition.or_default().part_number = Some(if text.is_empty() {
                PartNumber::ConfigurationName
            } else {
                PartNumber::Text(text.to_owned())
            });
        }
        "property:Description" => definition.or_default().description = Some(text.to_owned()),
        _ => return false,
    }
    true
}

/// The overrides of `row`'s cells; each cell that sets a parameter or feature
/// the design does not state is named in `unstated`, and each parameter cell
/// whose cached value the design does not state in `uncached`.
fn overrides(
    row: &LegacyConfiguration,
    scope: &ProductDefinitionId,
    features: &BTreeSet<&str>,
    parameters: &[DesignParameter],
    unstated: &mut Vec<String>,
    uncached: &mut Vec<String>,
) -> Overrides {
    let scoped = parameters
        .iter()
        .filter(|parameter| &parameter.scope == scope)
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    let vocabulary = scoped
        .values()
        .filter_map(|parameter| Some((parameter.name.as_ref()?.as_str(), &parameter.id)))
        .collect::<HashMap<&str, &ParameterId>>();
    let mut overrides = Overrides::default();
    for (id, text) in &row.parameter_overrides {
        let Some(parameter) = scoped.get(id) else {
            unstated.push(format!("{id} in {}", row.id));
            continue;
        };
        let value = row
            .parameter_values
            .get(id)
            .and_then(|value| cache(value).1);
        if value.is_none() {
            uncached.push(format!("{id} in {}", row.id));
        }
        overrides.parameters.insert(
            id.clone(),
            ParameterOverride {
                expression: Some(expression(text, &parameter.kind, &vocabulary)),
                value,
            },
        );
    }
    for (id, state) in &row.feature_states {
        if features.contains(id.as_str()) {
            overrides.features.insert(
                id.clone(),
                FeatureStateOverride {
                    suppressed: Some(state.evaluation.is_suppressed()),
                    definition: None,
                },
            );
        } else {
            unstated.push(format!("{id} in {}", row.id));
        }
    }
    overrides
}

/// The v10 rows that state a design's configurations, with what they cannot
/// state.
pub(crate) struct Restated {
    rows: Vec<LegacyConfiguration>,
    charges: Charges,
}

/// What restated rows cannot state.
#[derive(Default)]
struct Charges {
    /// One `<path>: <why>` per field that no row states.
    unstated: Vec<String>,
    /// One `<path>` per parameter cell whose expression has no value here,
    /// which states the parameter's own value as its evaluated value.
    uncached: Vec<String>,
}

impl Restated {
    /// The rows; charges to `losses` each field that no row states, and each
    /// cell whose evaluated value is the parameter's own.
    pub(crate) fn into_rows(self, losses: &mut Vec<LossNote>) -> Vec<LegacyConfiguration> {
        losses.extend(edit_dropped(
            EntityKind::DesignConfiguration,
            "F3D",
            "drops these configuration edits, which a row of the configuration table cannot state",
            self.charges.unstated,
        ));
        losses.extend(value_unwritable(
            "F3D",
            "writes these parameter cells with the parameter's own value as the evaluated value of their expression, which has no value here",
            self.charges.uncached,
        ));
        self.rows
    }
}

/// The inputs of one row's overlay.
struct Scoped<'a> {
    scope: &'a ProductDefinitionId,
    /// The ids of the design's features.
    features: &'a BTreeSet<&'a str>,
    parameters: &'a [DesignParameter],
    /// The v10 parameters: the expression and the value of each in a
    /// configuration that overrides neither.
    own: &'a [LegacyParameter],
    /// The v10 features whose states the rows set.
    history: &'a [LegacyFeature],
    active: &'a BTreeSet<&'a ConfigurationId>,
    records: &'a BTreeMap<&'a ConfigurationId, &'a DesignConfiguration>,
    definitions: &'a [ProductDefinition],
}

/// `rows`, the v10 configurations of the configuration tables, with what
/// the configurations of `design` state otherwise than [`project`] reads
/// from them written in. A scope of `design` restates the rows of its
/// documents where they name an active row or there are none. A row of
/// such a scope with no configuration of its id is removed. A configuration
/// with no row becomes a new row after the others, in arena order. The rows
/// are numbered again in their order. `history` holds the v10 features
/// whose states the rows set, and `own` the v10 parameters.
pub(crate) fn restate(
    rows: &[LegacyConfiguration],
    design: &Design,
    definitions: &[ProductDefinition],
    history: &[LegacyFeature],
    own: &[LegacyParameter],
) -> Restated {
    let scoped = design
        .scopes
        .iter()
        .map(|scope| &scope.definition)
        .collect::<BTreeSet<_>>();
    let active = design
        .scopes
        .iter()
        .filter_map(|scope| scope.active_configuration.as_ref())
        .collect::<BTreeSet<_>>();
    let features = design
        .features
        .iter()
        .map(|feature| feature.id.as_str())
        .collect::<BTreeSet<_>>();
    let scope_of = |row: &LegacyConfiguration| {
        super::scope_definition(
            definitions,
            document_prefix(row.native_ref.as_deref().unwrap_or(row.id.as_str())),
        )
        .filter(|definition| scoped.contains(definition))
    };
    let mut named = BTreeMap::<ProductDefinitionId, bool>::new();
    for row in rows {
        if let Some(scope) = scope_of(row) {
            *named.entry(scope).or_default() |= row.active;
        }
    }
    let restates = |scope: &ProductDefinitionId| {
        scoped.contains(scope) && named.get(scope).copied().unwrap_or(true)
    };
    let records = design
        .configurations
        .iter()
        .map(|configuration| (&configuration.id, configuration))
        .collect::<BTreeMap<_, _>>();
    let input = |scope| Scoped {
        scope,
        features: &features,
        parameters: &design.parameters,
        own,
        history,
        active: &active,
        records: &records,
        definitions,
    };
    let mut out = Restated {
        rows: Vec::new(),
        charges: Charges::default(),
    };
    let mut held = BTreeSet::new();
    for row in rows {
        let Some(scope) = scope_of(row).filter(|scope| restates(scope)) else {
            out.rows.push(row.clone());
            continue;
        };
        let Some(record) = records.get(&row.id) else {
            continue;
        };
        held.insert(&row.id);
        let mut restated = row.clone();
        if record.scope == scope {
            overlay(
                &mut restated,
                record,
                &input(&record.scope),
                &mut out.charges,
            );
        } else {
            out.charges.unstated.push(format!(
                "design.configurations[{}].scope: the configuration table of its row belongs to {scope}",
                record.id
            ));
        }
        out.rows.push(restated);
    }
    let mut added = Vec::new();
    for record in &design.configurations {
        if held.contains(&record.id) {
            continue;
        }
        if !restates(&record.scope) {
            out.charges.unstated.push(format!(
                "design.configurations[{}]: the design has no scope {}, or the configuration table of that scope names no active row",
                record.id, record.scope
            ));
            continue;
        }
        let mut row = LegacyConfiguration {
            id: record.id.clone(),
            ordinal: 0,
            active: false,
            source_index: None,
            name: Some(record.name.clone()),
            material: None,
            properties: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            bodies: None,
            native_ref: record.native_ref.clone(),
        };
        overlay(&mut row, record, &input(&record.scope), &mut out.charges);
        added.push(row);
    }
    out.rows
        .sort_by(|left, right| (left.ordinal, &left.id).cmp(&(right.ordinal, &right.id)));
    out.rows.extend(added);
    for (at, row) in (0_u32..).zip(&mut out.rows) {
        row.ordinal = at;
    }
    out.charges.unstated.sort();
    out.charges.uncached.sort();
    out
}

/// `record` and the configurations it inherits from, nearest first: its
/// parent, that one's parent, and so on, while each is a configuration of
/// `record`'s scope that the chain does not hold yet.
fn chain<'a>(
    record: &'a DesignConfiguration,
    records: &BTreeMap<&ConfigurationId, &'a DesignConfiguration>,
) -> Vec<&'a DesignConfiguration> {
    let mut chain = vec![record];
    let mut next = record.parent.as_ref();
    while let Some(parent) = next.and_then(|id| records.get(id).copied()) {
        if parent.scope != record.scope || chain.iter().any(|held| held.id == parent.id) {
            break;
        }
        chain.push(parent);
        next = parent.parent.as_ref();
    }
    chain
}

/// The overrides that the first configuration of `chain` resolves: each
/// value its own where it states one, else the nearest of the others'
/// (cadir-v11.md §5.11, Resolution).
fn resolved(chain: &[&DesignConfiguration]) -> Overrides {
    let mut out = Overrides::default();
    for link in chain.iter().rev() {
        let stated = &link.overrides;
        out.parameters.extend(stated.parameters.clone());
        out.occurrences.extend(stated.occurrences.clone());
        for (id, state) in &stated.features {
            let held = out
                .features
                .entry(id.clone())
                .or_insert(FeatureStateOverride {
                    suppressed: None,
                    definition: None,
                });
            if state.suppressed.is_some() {
                held.suppressed = state.suppressed;
            }
            if state.definition.is_some() {
                held.definition.clone_from(&state.definition);
            }
        }
        for (id, definition) in &stated.definitions {
            let held = out.definitions.entry(id.clone()).or_default();
            for (field, value) in [
                (&mut held.source_name, &definition.source_name),
                (&mut held.label, &definition.label),
                (&mut held.description, &definition.description),
            ] {
                if value.is_some() {
                    field.clone_from(value);
                }
            }
            if definition.part_number.is_some() {
                held.part_number.clone_from(&definition.part_number);
            }
            held.bom_properties
                .extend(definition.bom_properties.clone());
        }
    }
    out
}

/// Write into `row` what `record` resolves otherwise than the overrides the
/// decoder projects from `row`, under the row's own activity: the active
/// row projects none, so a removal applies to another row only. A row has
/// no parent, so it states the values `record` inherits. Charges each field
/// no row states, and each cell whose evaluated value is the parameter's
/// own, to `charges`.
fn overlay(
    row: &mut LegacyConfiguration,
    record: &DesignConfiguration,
    input: &Scoped<'_>,
    charges: &mut Charges,
) {
    let stored_active = row.active;
    let projected = projected(
        row,
        input.scope,
        input.features,
        input.parameters,
        &mut Omitted::default(),
    );
    let chain = chain(record, input.records);
    let resolved = resolved(&chain);
    let path = |field: &str| format!("design.configurations[{}].{field}", record.id);
    if row.name.as_deref().unwrap_or_default() != record.name {
        row.name = Some(record.name.clone());
    }
    row.active = input.active.contains(&record.id);
    let scoped = input
        .parameters
        .iter()
        .filter(|parameter| &parameter.scope == input.scope)
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    let vocabulary = scoped
        .values()
        .filter_map(|parameter| Some((parameter.name.as_ref()?.as_str(), &parameter.id)))
        .collect::<HashMap<&str, &ParameterId>>();
    let names = scoped
        .values()
        .filter_map(|parameter| Some((&parameter.id, parameter.name.as_ref()?.as_str())))
        .collect::<HashMap<_, _>>();
    for (id, stated) in &resolved.parameters {
        if projected.parameters.get(id) == Some(stated) {
            continue;
        }
        let field = path(&format!("overrides.parameters[{id}]"));
        let Some(parameter) = scoped.get(id) else {
            charges
                .unstated
                .push(format!("{field}: the scope holds no such parameter"));
            continue;
        };
        let own = input.own.iter().find(|own| own.id == *id);
        let text = match (&stated.expression, own) {
            (Some(expression), _) => {
                let text = spelled(expression, &names).filter(|text| {
                    &super::parameters::expression(text, &parameter.kind, &vocabulary) == expression
                });
                let Some(text) = text else {
                    charges.unstated.push(format!(
                        "{field}.expression: a parameter cell has no Fusion text that reads back as it"
                    ));
                    continue;
                };
                text
            }
            (None, Some(own)) => own.expression.clone(),
            (None, None) => {
                charges.unstated.push(format!(
                    "{field}.value: a parameter cell states an expression, and the history holds no expression of the parameter"
                ));
                continue;
            }
        };
        let value = stated
            .value
            .as_ref()
            .and_then(|value| legacy_value(value, &parameter.kind));
        if stated.value.is_some() && value.is_none() {
            charges.unstated.push(format!(
                "{field}.value: the value is not of the parameter's kind"
            ));
        }
        let value = value.or_else(|| {
            let evaluated = stated
                .expression
                .as_ref()
                .and_then(|expression| evaluated(expression, parameter, &resolved, &scoped));
            if evaluated.is_none() {
                charges.uncached.push(field.clone());
            }
            evaluated.or_else(|| own.and_then(|own| own.value.clone()))
        });
        row.parameter_overrides.insert(id.clone(), text);
        match value {
            Some(value) => {
                row.parameter_values.insert(id.clone(), value);
            }
            None => {
                row.parameter_values.remove(id);
            }
        }
    }
    if !stored_active {
        for id in projected
            .parameters
            .keys()
            .filter(|id| !resolved.parameters.contains_key(*id))
        {
            row.parameter_overrides.remove(id);
            row.parameter_values.remove(id);
        }
    }
    for (id, stated) in &resolved.features {
        if projected.features.get(id) == Some(stated) {
            continue;
        }
        let field = path(&format!("overrides.features[{id}]"));
        if stated.definition.is_some() {
            charges.unstated.push(format!(
                "{field}.definition: a feature cell states only whether the feature is computed"
            ));
        }
        let Some(suppressed) = stated.suppressed else {
            row.feature_states.remove(id);
            continue;
        };
        let feature = input
            .history
            .iter()
            .find(|feature| feature.id == *id)
            .filter(|_| input.features.contains(id.as_str()));
        let Some(feature) = feature else {
            charges.unstated.push(format!(
                "{field}.suppressed: the history holds no such feature"
            ));
            continue;
        };
        row.feature_states.insert(
            id.clone(),
            ConfigurationFeatureState {
                evaluation: if suppressed {
                    ConfigurationEvaluation::Suppressed {}
                } else {
                    ConfigurationEvaluation::Active {
                        outputs: DistinctMembers::default(),
                    }
                },
                dependencies: feature.dependencies.clone(),
                definition: feature.evaluation.definition().clone(),
            },
        );
    }
    if !stored_active {
        for id in projected
            .features
            .keys()
            .filter(|id| !resolved.features.contains_key(*id))
        {
            row.feature_states.remove(id);
        }
    }
    identities(
        row,
        &chain,
        &resolved,
        &projected,
        input,
        stored_active,
        &mut charges.unstated,
    );
    let theme = "theme tables (`childTables`), which f3d.md §1.2 does not read";
    for (stated, field, why) in [
        (
            record.parent.is_some(),
            "parent",
            "a row has no parent row, so the row states the values it inherits".to_owned(),
        ),
        (
            record.comment.is_some(),
            "comment",
            "a row stores no comment".to_owned(),
        ),
        (
            !record.active_arrangements.is_empty(),
            "active_arrangements",
            format!("a row selects its appearance theme from the {theme}"),
        ),
        (
            !record.materials.is_empty(),
            "materials",
            format!("a row selects its material theme from the {theme}"),
        ),
        (
            !resolved.occurrences.is_empty(),
            "overrides.occurrences",
            "a row has no cell for an occurrence".to_owned(),
        ),
    ] {
        if stated {
            charges.unstated.push(format!("{}: {why}", path(field)));
        }
    }
}

/// The v10 value of `expression`, the expression of `parameter` in a
/// configuration that resolves `resolved`. A reference takes the value that
/// configuration states for its parameter, else that parameter's own cache
/// among `scoped`. `None` where the expression has no value.
fn evaluated(
    expression: &Expression,
    parameter: &DesignParameter,
    resolved: &Overrides,
    scoped: &HashMap<&ParameterId, &DesignParameter>,
) -> Option<LegacyValue> {
    let Expression::Neutral(Neutral(node)) = expression else {
        return None;
    };
    let mut reference = |reference: &ParameterRef| {
        let (RefTarget::Parameter(id), None) = (&reference.target, &reference.through) else {
            return None;
        };
        let target = scoped.get(id)?;
        let value = resolved
            .parameters
            .get(id)
            .and_then(|stated| stated.value.as_ref())
            .or(target.value.as_ref())?;
        edit_value(value, &target.kind)
    };
    let value = match (evaluate(node, &mut reference).ok()?, &parameter.kind) {
        (Value::Number { value, quantity }, ValueKind::Quantity(kind)) if quantity == *kind => {
            ParameterValue::Number(FiniteReal::new(value)?)
        }
        (Value::Boolean(flag), ValueKind::Boolean) => ParameterValue::Boolean(flag),
        (Value::Text(text), ValueKind::Text) => ParameterValue::Text(text),
        _ => return None,
    };
    legacy_value(&value, &parameter.kind)
}

/// The value the evaluator takes for `value`, a value of a parameter of
/// `kind`.
fn edit_value(value: &ParameterValue, kind: &ValueKind) -> Option<Value> {
    Some(match (value, kind) {
        (ParameterValue::Number(number), ValueKind::Quantity(quantity)) => Value::Number {
            value: number.get(),
            quantity: *quantity,
        },
        (ParameterValue::Boolean(flag), _) => Value::Boolean(*flag),
        (ParameterValue::Text(text), _) => Value::Text(text.clone()),
        _ => return None,
    })
}

/// Write into `row` the part number and description that `chain[0]`, whose
/// chain of parents is the rest of `chain`, resolves (`resolved`) for the
/// scope otherwise than `projected`, as the row's `Part Number` and
/// `Description` property cells.
fn identities(
    row: &mut LegacyConfiguration,
    chain: &[&DesignConfiguration],
    resolved: &Overrides,
    projected: &Overrides,
    input: &Scoped<'_>,
    stored_active: bool,
    unstated: &mut Vec<String>,
) {
    let record = chain[0];
    let scope = input.scope;
    let path = |field: &str| format!("design.configurations[{}].{field}", record.id);
    for definition in resolved.definitions.keys() {
        if definition != scope {
            unstated.push(format!(
                "{}: a row's property cells set the definition of its own scope",
                path(&format!("overrides.definitions[{definition}]"))
            ));
        }
    }
    let base = projected
        .definitions
        .get(scope)
        .cloned()
        .unwrap_or_default();
    let stated = match resolved.definitions.get(scope) {
        Some(stated) => stated.clone(),
        None if stored_active => base.clone(),
        None => DefinitionOverride::default(),
    };
    let field = path(&format!("overrides.definitions[{scope}]"));
    if stated.source_name.is_some() || stated.label.is_some() || !stated.bom_properties.is_empty() {
        unstated.push(format!(
            "{field}: a row's property cells set only the part number and the description"
        ));
    }
    let part_number = match written_part_number(chain, input) {
        Ok(part_number) => part_number,
        Err(why) => {
            unstated.push(format!("{field}.part_number: {why}"));
            base.part_number.clone()
        }
    };
    if part_number != base.part_number {
        let text = part_number.map(|part_number| match part_number {
            PartNumber::Text(text) => text,
            _ => String::new(),
        });
        set_property(row, "property:Part Number", text);
    }
    if stated.description != base.description {
        set_property(row, "property:Description", stated.description.clone());
    }
}

/// The part number that `chain[0]` resolves for the scope, as a row's cell
/// states it: a text, or `ConfigurationName` for the row's own name. A rule
/// is evaluated at the configuration of `chain` that states it, and the
/// string of a rule the cell does not hold is written as `Text`
/// (cadir-v11.md §5.11, Identity). The error names why the rule has no
/// string.
fn written_part_number(
    chain: &[&DesignConfiguration],
    input: &Scoped<'_>,
) -> Result<Option<PartNumber>, &'static str> {
    let stated = chain.iter().enumerate().find_map(|(at, link)| {
        Some((
            at,
            link.overrides
                .definitions
                .get(input.scope)?
                .part_number
                .as_ref()?,
        ))
    });
    let Some((at, part_number)) = stated else {
        return Ok(None);
    };
    let holder = chain[at];
    Ok(Some(match part_number {
        PartNumber::Text(text) => PartNumber::Text(text.clone()),
        PartNumber::ConfigurationName if at == 0 => PartNumber::ConfigurationName,
        PartNumber::ConfigurationName => PartNumber::Text(holder.name.clone()),
        PartNumber::ParentConfigurationName => {
            let parent = holder
                .parent
                .as_ref()
                .and_then(|parent| input.records.get(parent))
                .ok_or("the configuration that states the rule has no parent")?;
            PartNumber::Text(parent.name.clone())
        }
        PartNumber::DocumentName => {
            let own = resolved(&chain[at..]);
            let own = own.definitions.get(input.scope);
            let definition = input
                .definitions
                .iter()
                .find(|definition| &definition.id == input.scope);
            let title = |properties: &BTreeMap<NonBlankString, String>| {
                properties
                    .iter()
                    .find(|(key, _)| key.as_str() == "Title")
                    .map(|(_, title)| title.clone())
            };
            let name = own
                .and_then(|own| title(&own.bom_properties))
                .or_else(|| definition.and_then(|definition| title(&definition.bom_properties)))
                .or_else(|| own.and_then(|own| own.source_name.clone()))
                .or_else(|| definition.and_then(|definition| definition.source_name.clone()))
                .ok_or("the definition states no title and no source name")?;
            PartNumber::Text(name)
        }
    }))
}

/// Set the property `key` of `row` to `text`, or remove it.
fn set_property(row: &mut LegacyConfiguration, key: &str, text: Option<String>) {
    match (text, NonBlankString::new(key.to_owned())) {
        (Some(text), Some(key)) => {
            row.properties.insert(key, text);
        }
        _ => {
            row.properties.remove(key);
        }
    }
}

/// The Fusion text of `expression`, with each reference spelled by its
/// parameter's name in `names`; `None` where it has none.
fn spelled(expression: &Expression, names: &HashMap<&ParameterId, &str>) -> Option<String> {
    let name = |reference: &ParameterRef| match (&reference.target, &reference.through) {
        (RefTarget::Parameter(id), None) => names.get(id).map(|name| (*name).to_owned()),
        _ => None,
    };
    match expression {
        Expression::Neutral(Neutral(node)) => match node {
            Node::Number { value, unit } => number(value.get(), unit),
            Node::Unary {
                op: UnaryOp::Negate,
                operand,
            } => match operand.as_ref() {
                Node::Number { value, unit } => number(-value.get(), unit),
                _ => None,
            },
            Node::Reference(reference) => name(reference),
            _ => None,
        },
        Expression::Source {
            dialect: ExpressionDialect::Fusion,
            tokens,
        } => tokens
            .iter()
            .map(|token| match token {
                ExpressionToken::Text(text) => Some(text.clone()),
                ExpressionToken::Reference(reference) => name(reference),
            })
            .collect(),
        Expression::Source { .. } => None,
    }
}

/// The Fusion literal of `value` in `unit`.
fn number(value: f64, unit: &Unit) -> Option<String> {
    match unit.0.as_slice() {
        [] => Some(format!("{value}")),
        [factor] if factor.power.get() == 1 => Some(format!("{value} {}", factor.symbol.name())),
        _ => None,
    }
}

/// The v10 value of `value`, a value of a parameter of `kind`: the inverse
/// of [`cache`].
fn legacy_value(value: &ParameterValue, kind: &ValueKind) -> Option<LegacyValue> {
    Some(match value {
        ParameterValue::Number(number) if *kind == ValueKind::Quantity(Quantity::LENGTH) => {
            LegacyValue::Length(Length::new(number.get())?)
        }
        ParameterValue::Number(number) if *kind == ValueKind::Quantity(Quantity::ANGLE) => {
            LegacyValue::Angle(Angle::new(number.get())?)
        }
        ParameterValue::Number(number) if *kind == ValueKind::Quantity(Quantity::REAL) => {
            LegacyValue::Real(*number)
        }
        ParameterValue::Integer(integer) if *kind == ValueKind::Integer => {
            LegacyValue::Integer(*integer)
        }
        ParameterValue::Boolean(flag) if *kind == ValueKind::Boolean => LegacyValue::Boolean(*flag),
        ParameterValue::Text(text) if *kind == ValueKind::Text => LegacyValue::String(text.clone()),
        _ => return None,
    })
}
