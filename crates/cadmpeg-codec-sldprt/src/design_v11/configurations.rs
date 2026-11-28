// SPDX-License-Identifier: Apache-2.0
//! The part's configurations (§5.11).
//!
//! `SolidWorks` configurations are peers, so the base is the configuration
//! active at decode, which states no overrides. Another configuration states
//! the material it assigns where that differs from the active one's. Its
//! parameter values are compared with the inherited expressions evaluated
//! in that configuration. A different value replaces the expression with a
//! literal, or is held as a cache with a loss if the expression is unread.
//!
//! Interim: the decoder reads a feature's suppression from the history,
//! which every configuration shares, so no configuration states a
//! suppression of its own, and a loss names each configuration whose own
//! suppression is unread (DI-110). The configuration's own definition of a feature
//! (§5.11 `definition`) is deferred: the decoder's per-configuration
//! definitions differ from the active one's also where only a dimension
//! differs, and no reading yet separates a structural difference from that,
//! so a loss names each feature whose definition differs.

use std::collections::{BTreeSet, HashMap, HashSet};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::configuration::{
    DesignConfiguration, MaterialOverride, MaterialRef, Overrides,
};
use cadmpeg_ir::design::evaluation::Evaluation;
use cadmpeg_ir::design::expr::{Expression, ExpressionToken};
use cadmpeg_ir::design::feature::Feature;
use cadmpeg_ir::design::projection::restated::Restater;
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::features::parameters::{
    ConfigurationEvaluation, DesignConfiguration as ModelConfiguration,
};
use cadmpeg_ir::features::ConfigurationId;
use cadmpeg_ir::ids::{BodyId, ProductDefinitionId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::Body;

use crate::history::project::neutral_configuration_id;
use crate::loss::{NamedCount, SldprtLossCode};
use crate::records::{Configuration, FeatureHistory};

pub(super) mod values;

/// The configurations of a scope and the active one.
#[derive(Default)]
pub(super) struct Configurations {
    pub(super) records: Vec<DesignConfiguration>,
    pub(super) active: Option<ConfigurationId>,
}

/// The configurations of the histories, or none where the document's active
/// configuration does not resolve to one of them: a scope with
/// configurations names its active one (part 10), and a loss in `losses`
/// names them.
pub(super) fn configurations(
    histories: &[FeatureHistory],
    source: Option<&SourceMeta>,
    scope: &ProductDefinitionId,
    losses: &mut Vec<LossNote>,
) -> Result<Configurations, CodecError> {
    let native = histories
        .iter()
        .flat_map(|history| &history.configurations)
        .collect::<Vec<_>>();
    let selectors = native
        .iter()
        .map(|configuration| {
            (
                Some(configuration.name.as_str()),
                configuration.partition_index(),
            )
        })
        .collect::<Vec<_>>();
    let Some(active) = crate::decode::active_configuration_position(source, &selectors) else {
        let mut named = NamedCount::default();
        for configuration in &native {
            named.push(neutral_configuration_id(&configuration.id)?.as_str());
        }
        if !named.is_empty() {
            losses.push(
                SldprtLossCode::DesignConfigurationActiveUnread.note(format!(
                    "{} configurations state no design configuration: the document's active \
                 configuration resolves to none of them: {named}",
                    named.count()
                )),
            );
        }
        return Ok(Configurations::default());
    };
    let listed = histories
        .iter()
        .filter(|history| !history.configurations.is_empty())
        .count()
        == 1;
    let base_material = native[active].material.as_deref();
    let mut records = Vec::with_capacity(native.len());
    for configuration in &native {
        let materials = configuration
            .material
            .as_deref()
            .filter(|material| Some(*material) != base_material)
            .and_then(|material| NonBlankString::new(material.to_owned()))
            .map(|material| MaterialOverride {
                target: None,
                material: MaterialRef::Named(material),
            })
            .into_iter()
            .collect();
        records.push(DesignConfiguration {
            id: neutral_configuration_id(&configuration.id)?,
            scope: scope.clone(),
            name: configuration.name.clone(),
            position: listed.then_some(configuration.ordinal),
            parent: None,
            comment: None,
            overrides: Overrides::default(),
            active_arrangements: BTreeSet::new(),
            materials,
            native_ref: Some(configuration.id.clone()),
        });
    }
    let active = records.get(active).map(|record| record.id.clone());
    Ok(Configurations { records, active })
}

/// The note naming the bodies of `geometry` outside `current`, the active
/// configuration's bodies, in a part whose feature tree holds no feature.
/// The scope lists them with its unproduced bodies (§5.5): a scope with no
/// root container has no position for the stored state of a configuration
/// that is not active.
pub(super) fn unplaced(geometry: &[Body], current: &[BodyId]) -> Option<LossNote> {
    let current = current.iter().collect::<HashSet<_>>();
    let mut named = NamedCount::default();
    for body in geometry.iter().filter(|body| !current.contains(&body.id)) {
        named.push(body.id.as_str());
    }
    (!named.is_empty()).then(|| {
        SldprtLossCode::DesignConfigurationStateUnplaced.note(format!(
            "{} bodies the geometry holds outside the active configuration's bodies are \
             unproduced bodies of the scope: the feature tree holds no feature, and a scope \
             with no root container has no position for the stored state of a configuration \
             that is not active: {named}",
            named.count()
        ))
    })
}

/// The notes naming what the configurations that are not active do not
/// state of `features`: their own suppression, which the decoder does not
/// read, and each feature whose definition in their decoded state, the
/// model configuration of the same id in `decoded`, differs from the active
/// configuration's.
pub(super) fn unread_states(
    configurations: &Configurations,
    decoded: &[cadmpeg_ir::features::parameters::DesignConfiguration],
    features: &[Feature],
) -> Vec<LossNote> {
    let Configurations { records, active } = configurations;
    if features.is_empty() {
        return Vec::new();
    }
    let state = |id: &ConfigurationId| decoded.iter().find(|state| state.id == *id);
    let base = active.as_ref().and_then(state);
    let mut suppression = NamedCount::default();
    let mut definitions = NamedCount::default();
    for record in records {
        if active.as_ref() == Some(&record.id) {
            continue;
        }
        suppression.push(record.id.as_str());
        let Some(state) = state(&record.id) else {
            continue;
        };
        for feature in features {
            let differs = state
                .feature_states
                .get(&feature.id)
                .zip(base.and_then(|base| base.feature_states.get(&feature.id)))
                .is_some_and(|(own, active)| own.definition != active.definition);
            if differs {
                definitions.push(&format!(
                    "{} in {}",
                    feature.id.as_str(),
                    record.id.as_str()
                ));
            }
        }
    }
    let mut notes = Vec::new();
    if !suppression.is_empty() {
        notes.push(
            SldprtLossCode::DesignConfigurationSuppressionUnread.note(format!(
                "{} configurations state no suppression of their own: the decoder reads one \
             suppression state per feature, which every configuration shares: {suppression}",
                suppression.count()
            )),
        );
    }
    if !definitions.is_empty() {
        notes.push(
            SldprtLossCode::DesignConfigurationDefinitionUnstated.note(format!(
                "{} features of a configuration that is not active state no definition of their \
             own where the decoded definition differs from the active configuration's: no \
             reading separates a structural difference from a dimension's: {definitions}",
                definitions.count()
            )),
        );
    }
    notes
}

pub(super) fn native_with_active<'a>(
    histories: &'a [FeatureHistory],
    source: Option<&SourceMeta>,
) -> Option<(Vec<&'a Configuration>, usize)> {
    let native = histories
        .iter()
        .flat_map(|history| &history.configurations)
        .collect::<Vec<_>>();
    let selectors = native
        .iter()
        .map(|configuration| {
            (
                Some(configuration.name.as_str()),
                configuration.partition_index(),
            )
        })
        .collect::<Vec<_>>();
    let active = crate::decode::active_configuration_position(source, &selectors)?;
    Some((native, active))
}

/// States the model configuration fields each configuration leaves out, by
/// comparing the two records (§5.11). A configuration has no field for its
/// custom properties. Another configuration than the active one states what
/// differs from the active one's record only through its overrides and
/// materials, each override field covering only the field it states: a
/// parameter value through `value`, a parameter expression through an
/// `expression` that is the stored text, a feature's suppression through
/// `suppressed`, and its definition and dependencies through `definition`.
/// Its bodies are those of its stated state. The active configuration's
/// bodies are the current bodies `current`. Each leaves out `ordinal` where
/// the model holds more than one configuration and their order by `ordinal`
/// is not the order by `position` of their design configurations (§2.2).
/// The native record its `native_ref` names holds the native slot. Each
/// compared configuration is recorded, also where it leaves out no field.
pub(super) fn restate(
    ir: &mut CadIr,
    configurations: &Configurations,
    evaluations: &[Evaluation],
    current: &[BodyId],
    parameter_cuts: &HashSet<ConfigurationId>,
) {
    let model = ir
        .model
        .configurations
        .iter()
        .map(|configuration| (configuration.id.as_str(), configuration))
        .collect::<HashMap<_, _>>();
    let base = configurations
        .active
        .as_ref()
        .and_then(|active| model.get(active.as_str()).copied());
    let stated = evaluations
        .iter()
        .filter(|evaluation| evaluation.content.is_some())
        .filter_map(|evaluation| evaluation.configuration.as_ref())
        .collect::<HashSet<_>>();
    let ordered = ir.model.configurations.len() < 2 || {
        let positions = configurations
            .records
            .iter()
            .filter_map(|design| Some((design.id.as_str(), design.position?)))
            .collect::<HashMap<_, _>>();
        let mut placed = ir
            .model
            .configurations
            .iter()
            .map(|model| Some((model.ordinal, *positions.get(model.id.as_str())?)))
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default();
        placed.sort_unstable();
        placed.len() == ir.model.configurations.len()
            && placed.windows(2).all(|pair| pair[0].1 < pair[1].1)
    };
    let mut restated = Vec::new();
    for design in &configurations.records {
        let Some(own) = model.get(design.id.as_str()).copied() else {
            continue;
        };
        let mut cut = Vec::new();
        if !ordered {
            cut.push("ordinal");
        }
        if own.active != (configurations.active.as_ref() == Some(&design.id)) {
            cut.push("active");
        }
        if own.source_index.is_some() && design.native_ref.is_none() {
            cut.push("source_index");
        }
        if own
            .native_ref
            .as_ref()
            .is_some_and(|native| design.native_ref.as_ref() != Some(native))
        {
            cut.push("native_ref");
        }
        if !own.properties.is_empty() {
            cut.push("properties");
        }
        if own.name.as_deref().is_some_and(|name| name != design.name) {
            cut.push("name");
        }
        let bodies_stated = match base {
            Some(base) if base.id == own.id => own
                .bodies
                .as_ref()
                .is_none_or(|bodies| bodies.iter().all(|body| current.contains(body))),
            _ => {
                own.bodies
                    .as_ref()
                    .is_none_or(cadmpeg_ir::features::selections::DistinctMembers::is_empty)
                    || stated.contains(&design.id)
            }
        };
        if !bodies_stated {
            cut.push("bodies");
        }
        if let Some(base) = base.filter(|base| base.id != own.id) {
            cut.extend(override_cut(design, own, base));
            if parameter_cuts.contains(&design.id) {
                cut.push("parameter_values");
            }
        }
        restated.push((design.id.as_str().to_owned(), cut));
    }
    for (id, cut) in restated {
        let by = Restater::Record(id.clone());
        ir.model.restated_mut().record(&id, by, cut);
    }
}

/// Whether `expression` is the stored expression text `text`: a source
/// expression of text tokens alone that spell it.
fn spells(expression: Option<&Expression>, text: &str) -> bool {
    let Some(Expression::Source { tokens, .. }) = expression else {
        return false;
    };
    let mut spelled = String::new();
    for token in tokens {
        let ExpressionToken::Text(part) = token else {
            return false;
        };
        spelled.push_str(part);
    }
    spelled == text
}

/// The fields of `own`, a configuration other than the active one `base`,
/// that differ from the active one's and that `design` does not state.
fn override_cut(
    design: &DesignConfiguration,
    own: &ModelConfiguration,
    base: &ModelConfiguration,
) -> Vec<&'static str> {
    let mut cut = BTreeSet::new();
    let material_stated = own.material == base.material
        || own.material.as_deref().is_some_and(|material| {
            design.materials.iter().any(|stated| {
                stated.target.is_none()
                    && matches!(&stated.material, MaterialRef::Named(name) if name.as_str() == material)
            })
        });
    if !material_stated {
        cut.insert("material");
    }
    let parameters = &design.overrides.parameters;
    let parameter = |id: &cadmpeg_ir::ParameterId| parameters.get(id);
    if own.parameter_overrides.iter().any(|(id, text)| {
        base.parameter_overrides.get(id) != Some(text)
            && !spells(
                parameter(id).and_then(|stated| stated.expression.as_ref()),
                text,
            )
    }) {
        cut.insert("parameter_overrides");
    }
    for (id, state) in &own.feature_states {
        let stated = design.overrides.features.get(id);
        let active = base.feature_states.get(id);
        let suppressed = state.evaluation.is_suppressed();
        let suppression_differs = active.map_or(suppressed, |active| {
            active.evaluation.is_suppressed() != suppressed
        });
        if suppression_differs && stated.and_then(|stated| stated.suppressed) != Some(suppressed) {
            cut.insert("feature_states");
        }
        let outputs = |evaluation: &ConfigurationEvaluation| match evaluation {
            ConfigurationEvaluation::Active { outputs } => Some(outputs.as_slice().to_vec()),
            ConfigurationEvaluation::Suppressed {} => None,
        };
        let outputs_differ = match (outputs(&state.evaluation), active) {
            (Some(own), Some(active)) => {
                outputs(&active.evaluation).is_some_and(|base| base != own)
            }
            (Some(own), None) => !own.is_empty(),
            (None, _) => false,
        };
        if outputs_differ {
            cut.insert("feature_states");
        }
        let definition_stated = stated.is_some_and(|stated| stated.definition.is_some());
        let definition_differs = active.is_none_or(|active| active.definition != state.definition);
        if definition_differs && !definition_stated {
            cut.insert("feature_states.definition");
            if !state.dependencies.is_empty() {
                cut.insert("feature_states.dependencies");
            }
        }
        if active.is_none_or(|active| active.dependencies != state.dependencies)
            && !definition_stated
        {
            cut.insert("feature_states.dependencies");
        }
    }
    cut.into_iter().collect()
}
