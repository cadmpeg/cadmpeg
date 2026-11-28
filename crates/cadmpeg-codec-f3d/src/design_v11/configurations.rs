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
//! writes the tables. A parameter cell caches the value of its expression in
//! the configuration of its row (f3d.md §1.2).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::codec::write::{edit_dropped, value_unwritable};
use cadmpeg_ir::design::configuration::{
    DefinitionOverride, DesignConfiguration, FeatureStateOverride, Overrides, ParameterOverride,
    PartNumber,
};
use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
use cadmpeg_ir::design::edit::{cache_value, evaluate, kind_value};
use cadmpeg_ir::design::expr::{
    Expression, ExpressionDialect, ExpressionToken, Neutral, Node, UnaryOp,
};
use cadmpeg_ir::design::parameter::{DesignParameter, ParameterDefinition, ParameterValue};
use cadmpeg_ir::design::units::{Quantity, Unit, ValueKind};
use cadmpeg_ir::features::parameters::{
    ConfigurationEvaluation, ConfigurationFeatureState, DesignConfiguration as LegacyConfiguration,
    DesignParameter as LegacyParameter, ParameterValue as LegacyValue,
};
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{ConfigurationId, Feature as LegacyFeature, FeatureId, ParameterId};
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::products::ProductDefinition;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::schema::EntityKind;
use cadmpeg_ir::units::Tolerances;
use cadmpeg_ir::CadIr;

use super::parameters::{cache, expression};
use super::timeline::document_prefix;
use crate::loss::F3dLossCode;

pub(crate) mod rebase;

/// The configurations of the scopes keyed by document prefix in `scopes`,
/// and each scope's active configuration.
pub(crate) struct Configurations {
    pub(crate) records: Vec<DesignConfiguration>,
    pub(crate) active: BTreeMap<ProductDefinitionId, ConfigurationId>,
}

/// A document in `hosted`, or one `scopes` does not key, has no scope of its
/// own. `features` holds the ids of the scopes' features and `parameters`
/// their parameters. Each class of row or cell the design omits charges one
/// loss to `losses`.
pub(crate) fn project(
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
    rows: RestatedRows,
    charges: Charges,
}

/// The rows of [`restate`].
pub(crate) struct RestatedRows {
    pub(crate) rows: Vec<LegacyConfiguration>,
    /// The configurations whose rows are active.
    pub(crate) active: BTreeSet<ConfigurationId>,
    /// The cells of active rows that the design configurations state.
    pub(crate) stated: StatedCells,
    /// One `<kept> active in place of <document>` per scope whose stored
    /// active row the rows keep in place of the active configuration of the
    /// document ([`ActiveRows::Stored`]).
    pub(crate) kept: Vec<String>,
}

/// The cells written into active rows from what their configurations
/// resolve.
#[derive(Default)]
pub(crate) struct StatedCells {
    pub(crate) parameters: BTreeSet<(ConfigurationId, ParameterId)>,
    pub(crate) features: BTreeSet<(ConfigurationId, FeatureId)>,
}

/// Which rows [`restate`] makes active.
#[derive(Clone, Copy)]
pub(crate) enum ActiveRows {
    /// The row of each scope's active configuration.
    Design,
    /// The row the stored tables name active.
    Stored,
}

/// What restated rows cannot state.
#[derive(Default)]
struct Charges {
    /// One `<path>: <why>` per field that no row states.
    unstated: Vec<String>,
    /// One `<path>: <cache>` per parameter cell whose expression has no
    /// value here, with the value the cell caches in its place.
    uncached: Vec<String>,
    /// One `<path>` per parameter cell that caches its expression's value in
    /// place of a value the configuration states.
    stale: Vec<String>,
}

impl Restated {
    /// The rows; charges to `losses` each field that no row states, each
    /// cell whose cached value is not its expression's value, and each value
    /// a configuration states that a cell does not cache.
    pub(crate) fn into_rows(self, losses: &mut Vec<LossNote>) -> RestatedRows {
        losses.extend(edit_dropped(
            EntityKind::DesignConfiguration,
            "F3D",
            "drops these configuration edits, which a row of the configuration table cannot state",
            self.charges.unstated,
        ));
        losses.extend(value_unwritable(
            "F3D",
            "writes these parameter cells with the cached value each names: the writer cannot evaluate the cell's expression in the configuration of its row",
            self.charges.uncached,
        ));
        losses.extend(value_unwritable(
            "F3D",
            "writes these parameter cells with the value of their expression in the configuration of their row in place of the value the configuration states, which that expression does not give there: a cell caches the value of its expression (f3d.md §1.2)",
            self.charges.stale,
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
    /// The scopes whose active configuration differs from the stored
    /// tables' active row.
    moved: &'a BTreeSet<&'a ProductDefinitionId>,
    records: &'a BTreeMap<&'a ConfigurationId, &'a DesignConfiguration>,
    /// The rows the stored configuration tables project, by id.
    stored: &'a BTreeMap<&'a ConfigurationId, &'a LegacyConfiguration>,
    definitions: &'a [ProductDefinition],
    tolerances: &'a Tolerances,
}

/// `rows`, v10 configurations of the configuration tables, with what the
/// configurations of the design of `target` state otherwise than [`project`]
/// reads from `stored`, the rows the stored tables project, written in: the
/// neutral record wins (cadir-v11.md §3.4, W2), and a value of `rows` that
/// the design leaves as the stored tables state it is kept. A design scope
/// restates the rows of its documents where they name an active row or
/// there are none. A row of such a scope that the stored tables hold and no
/// configuration states is removed. A configuration with no row becomes a
/// new row after the others, in arena order, unless the stored tables hold
/// its row and it states what that row projects. The rows are numbered
/// again in their order. `history` holds the v10 features whose states the
/// rows set, and `own` the v10 parameters; a cache agrees with its
/// expression's value within the tolerances of `target`. `which` selects
/// the active rows.
pub(crate) fn restate(
    rows: &[LegacyConfiguration],
    stored: &[LegacyConfiguration],
    target: &CadIr,
    history: &[LegacyFeature],
    own: &[LegacyParameter],
    which: ActiveRows,
) -> Restated {
    let (design, definitions, tolerances) = (
        &target.design,
        target.model.product_definitions.as_slice(),
        &target.tolerances,
    );
    let scoped = design
        .scopes
        .iter()
        .map(|scope| &scope.definition)
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
    for row in rows.iter().chain(
        stored
            .iter()
            .filter(|_| matches!(which, ActiveRows::Stored)),
    ) {
        if let Some(scope) = scope_of(row) {
            *named.entry(scope).or_default() |= row.active;
        }
    }
    let restates = |scope: &ProductDefinitionId| {
        scoped.contains(scope) && named.get(scope).copied().unwrap_or(true)
    };
    let mut active = match which {
        ActiveRows::Stored => stored
            .iter()
            .filter(|row| row.active)
            .map(|row| &row.id)
            .collect(),
        ActiveRows::Design => BTreeSet::new(),
    };
    let (mut moved, mut kept) = (BTreeSet::new(), Vec::new());
    let shown = |id: Option<&ConfigurationId>| {
        id.map_or("no configuration".to_owned(), ToString::to_string)
    };
    for scope in &design.scopes {
        let held = stored
            .iter()
            .find(|row| row.active && scope_of(row).as_ref() == Some(&scope.definition))
            .map(|row| &row.id);
        let stated = scope.active_configuration.as_ref();
        match which {
            ActiveRows::Design => {
                active.extend(stated);
                if stated != held {
                    moved.insert(&scope.definition);
                }
            }
            ActiveRows::Stored => {
                if stated != held {
                    kept.push(format!(
                        "{} active in place of {}",
                        shown(held),
                        shown(stated)
                    ));
                }
            }
        }
    }
    let stored = stored
        .iter()
        .map(|row| (&row.id, row))
        .collect::<BTreeMap<_, _>>();
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
        moved: &moved,
        records: &records,
        stored: &stored,
        definitions,
        tolerances,
    };
    let mut out = Restated {
        rows: RestatedRows {
            rows: Vec::new(),
            active: active.iter().map(|id| (*id).clone()).collect(),
            stated: StatedCells::default(),
            kept,
        },
        charges: Charges::default(),
    };
    let present = rows.iter().map(|row| &row.id).collect::<BTreeSet<_>>();
    let retained = stored.values().copied().filter(|row| {
        matches!(which, ActiveRows::Stored)
            && active.contains(&row.id)
            && !present.contains(&row.id)
    });
    let mut held = BTreeSet::new();
    for row in rows.iter().chain(retained) {
        let Some(scope) = scope_of(row).filter(|scope| restates(scope)) else {
            out.rows.rows.push(row.clone());
            continue;
        };
        let Some(record) = records.get(&row.id) else {
            match stored.get(&row.id) {
                Some(stored) if matches!(which, ActiveRows::Stored) && active.contains(&row.id) => {
                    out.rows.rows.push((*stored).clone());
                    out.charges.unstated.push(format!(
                        "design.configurations[{}]: the retained DesignConfigurationRoot record still references this row",
                        row.id
                    ));
                }
                Some(_) => {}
                None => out.rows.rows.push(row.clone()),
            }
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
                &mut out.rows.stated,
            );
        } else {
            out.charges.unstated.push(format!(
                "design.configurations[{}].scope: the configuration table of its row belongs to {scope}",
                record.id
            ));
        }
        out.rows.rows.push(restated);
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
        if stored
            .get(&record.id)
            .is_some_and(|row| states_row(record, row, &input(&record.scope)))
        {
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
        overlay(
            &mut row,
            record,
            &input(&record.scope),
            &mut out.charges,
            &mut out.rows.stated,
        );
        added.push(row);
    }
    out.rows
        .rows
        .sort_by(|left, right| (left.ordinal, &left.id).cmp(&(right.ordinal, &right.id)));
    out.rows.rows.extend(added);
    for (at, row) in (0_u32..).zip(&mut out.rows.rows) {
        row.ordinal = at;
    }
    out.charges.unstated.sort();
    out.charges.uncached.sort();
    out.charges.stale.sort();
    out
}

/// Whether `record` states what the stored row `row` projects, and nothing
/// a row cannot hold.
fn states_row(record: &DesignConfiguration, row: &LegacyConfiguration, input: &Scoped<'_>) -> bool {
    record.parent.is_none()
        && record.comment.is_none()
        && record.materials.is_empty()
        && record.active_arrangements.is_empty()
        && row.name.as_deref().unwrap_or_default() == record.name
        && projected(
            row,
            input.scope,
            input.features,
            input.parameters,
            &mut Omitted::default(),
        ) == record.overrides
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
/// decoder projects from the stored row of its id (else from `row`), read
/// under the record's activity: the active row projects none, and the active
/// configuration states none, so a removal applies to another row only. A
/// value `record` resolves as the stored row states it keeps `row`'s. A row
/// has no parent, so it states the values `record` inherits. Each parameter
/// cell caches its expression's value in the configuration (§5.11,
/// Resolution). Charges to `charges` each field no row states, and each
/// cache the cell does not take from its expression. Adds to `stated` each
/// cell written into the row of an active configuration.
fn overlay(
    row: &mut LegacyConfiguration,
    record: &DesignConfiguration,
    input: &Scoped<'_>,
    charges: &mut Charges,
    stated_cells: &mut StatedCells,
) {
    let mut stored = input
        .stored
        .get(&row.id)
        .map_or_else(|| row.clone(), |stored| (*stored).clone());
    let active = input.active.contains(&record.id);
    stored.active = active;
    let projected = projected(
        &stored,
        input.scope,
        input.features,
        input.parameters,
        &mut Omitted::default(),
    );
    let chain = chain(record, input.records);
    let resolved = resolved(&chain);
    let path = |field: &str| format!("design.configurations[{}].{field}", record.id);
    if stored.name.as_deref().unwrap_or_default() != record.name {
        row.name = Some(record.name.clone());
    }
    if input.moved.contains(input.scope) {
        row.active = input.active.contains(&record.id);
    }
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
    let mut values = Values::new(&resolved.parameters, &scoped);
    let mut before = Values::new(&projected.parameters, &scoped);
    for (id, stated) in &resolved.parameters {
        let field = path(&format!("overrides.parameters[{id}]"));
        let Some(parameter) = scoped.get(id) else {
            charges
                .unstated
                .push(format!("{field}: the scope holds no such parameter"));
            continue;
        };
        let typed = |value: Option<ParameterValue>| {
            value.and_then(|value| legacy_value(&value, &parameter.kind))
        };
        let evaluation = typed(values.evaluation(id));
        if projected.parameters.get(id) == Some(stated) {
            let moved = match (&evaluation, typed(before.evaluation(id))) {
                (Some(now), Some(then)) => !agree(now, &then, input.tolerances),
                _ => false,
            };
            if !moved {
                continue;
            }
        }
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
        let stated_value = stated
            .value
            .as_ref()
            .and_then(|value| legacy_value(value, &parameter.kind));
        let charged = stated.value.is_some() && stated_value.is_none();
        if charged {
            charges.unstated.push(format!(
                "{field}.value: the value is not of the parameter's kind"
            ));
        }
        let cell = stored
            .parameter_overrides
            .get(id)
            .filter(|cell| **cell == text)
            .and_then(|_| stored.parameter_values.get(id));
        let value = match (evaluation, &stated.expression) {
            (Some(evaluated), _) => match stated_value {
                Some(stated) if agree(&stated, &evaluated, input.tolerances) => Some(stated),
                Some(_) => {
                    charges.stale.push(field);
                    Some(evaluated)
                }
                None => Some(evaluated),
            },
            (None, Some(_)) => match (stated_value, cell) {
                (Some(stated), _) => Some(stated),
                (None, Some(cell)) => Some(cell.clone()),
                (None, None) => {
                    if !charged {
                        charges
                            .uncached
                            .push(format!("{field}: the parameter's own value"));
                    }
                    own.and_then(|own| own.value.clone())
                }
            },
            (None, None) => match (cell, stated_value) {
                (Some(cell), Some(stated)) => {
                    if !agree(cell, &stated, input.tolerances) {
                        charges.uncached.push(format!(
                            "{field}: the stored cell's value in place of the stated value"
                        ));
                    }
                    Some(cell.clone())
                }
                (Some(cell), None) => Some(cell.clone()),
                (None, stated) => stated.or_else(|| own.and_then(|own| own.value.clone())),
            },
        };
        row.parameter_overrides.insert(id.clone(), text);
        if active {
            stated_cells
                .parameters
                .insert((record.id.clone(), id.clone()));
        }
        match value {
            Some(value) => {
                row.parameter_values.insert(id.clone(), value);
            }
            None => {
                row.parameter_values.remove(id);
            }
        }
    }
    if !active {
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
        if active {
            stated_cells
                .features
                .insert((record.id.clone(), id.clone()));
        }
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
    if !active {
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
        active,
        &mut charges.unstated,
    );
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
            "a row has no cell for an arrangement".to_owned(),
        ),
        (
            !record.materials.is_empty(),
            "materials",
            "a row has no cell for a material".to_owned(),
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

/// The deepest chain of references through which [`Values`] evaluates a
/// parameter's value.
const REFERENCE_DEPTH: usize = 64;

/// The relative tolerance within which a cache that is neither a length nor
/// an angle agrees with its expression's value, as validation compares
/// caches.
const CACHE_RELATIVE_TOLERANCE: f64 = 1e-9;

/// The values of a scope's parameters in a configuration that resolves
/// `overrides` (cadir-v11.md §5.11, Resolution).
struct Values<'a> {
    overrides: &'a BTreeMap<ParameterId, ParameterOverride>,
    scoped: &'a HashMap<&'a ParameterId, &'a DesignParameter>,
    evaluated: HashMap<ParameterId, Option<ParameterValue>>,
    open: Vec<ParameterId>,
}

impl<'a> Values<'a> {
    fn new(
        overrides: &'a BTreeMap<ParameterId, ParameterOverride>,
        scoped: &'a HashMap<&'a ParameterId, &'a DesignParameter>,
    ) -> Self {
        Self {
            overrides,
            scoped,
            evaluated: HashMap::new(),
            open: Vec::new(),
        }
    }

    /// The value of the expression of `id` in the configuration: its
    /// override's expression, else its own, with each reference taking the
    /// referenced parameter's [`Self::value`]. `None` where the expression is
    /// not a neutral tree, has no value of the parameter's kind, or reaches
    /// `id` again or deeper than [`REFERENCE_DEPTH`].
    fn evaluation(&mut self, id: &ParameterId) -> Option<ParameterValue> {
        if let Some(value) = self.evaluated.get(id) {
            return value.clone();
        }
        if self.open.contains(id) || self.open.len() >= REFERENCE_DEPTH {
            return None;
        }
        let (overrides, scoped) = (self.overrides, self.scoped);
        let parameter = *scoped.get(id)?;
        let expression = match overrides
            .get(id)
            .and_then(|stated| stated.expression.as_ref())
        {
            Some(expression) => Some(expression),
            None => match &parameter.definition {
                ParameterDefinition::Expression(expression) => Some(expression),
                _ => None,
            },
        };
        let value = match expression {
            Some(Expression::Neutral(Neutral(node))) => {
                self.open.push(id.clone());
                let value = evaluate(node, &mut |reference: &ParameterRef| {
                    let (RefTarget::Parameter(target), None) =
                        (&reference.target, &reference.through)
                    else {
                        return None;
                    };
                    let kind = &scoped.get(target)?.kind;
                    cache_value(kind, &self.value(target)?)
                });
                self.open.pop();
                value
                    .ok()
                    .and_then(|value| kind_value(&parameter.kind, value).ok())
            }
            _ => None,
        };
        self.evaluated.insert(id.clone(), value.clone());
        value
    }

    /// The value of `id` in the configuration: its [`Self::evaluation`],
    /// else the value its override states, else its own cache.
    fn value(&mut self, id: &ParameterId) -> Option<ParameterValue> {
        self.evaluation(id)
            .or_else(|| self.overrides.get(id)?.value.clone())
            .or_else(|| self.scoped.get(id)?.value.clone())
    }
}

/// Whether the cache `left` agrees with `right`: a length within the linear
/// tolerance, an angle within the angular tolerance, a real within
/// [`CACHE_RELATIVE_TOLERANCE`], another value exactly.
pub(crate) fn agree(left: &LegacyValue, right: &LegacyValue, tolerances: &Tolerances) -> bool {
    match (left, right) {
        (LegacyValue::Length(left), LegacyValue::Length(right)) => {
            (left.get() - right.get()).abs() <= tolerances.linear.get()
        }
        (LegacyValue::Angle(left), LegacyValue::Angle(right)) => {
            (left.get() - right.get()).abs() <= tolerances.angular.get()
        }
        (LegacyValue::Real(left), LegacyValue::Real(right)) => {
            (left.get() - right.get()).abs()
                <= CACHE_RELATIVE_TOLERANCE * left.get().abs().max(right.get().abs())
        }
        _ => left == right,
    }
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
    active: bool,
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
        None if active => base.clone(),
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
