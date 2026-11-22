// SPDX-License-Identifier: Apache-2.0
//! What the caller changed against the decoded source document, and the
//! edits the native records cannot carry.

use std::collections::{BTreeSet, HashMap, HashSet};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::parameters::DesignParameter;
use cadmpeg_ir::features::{Feature, FeatureId, ParameterId};
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::SldprtLossCode;
use crate::native::SldprtNative;
use crate::records::FeatureInputLane;

/// Which part of a parameter edit the output does not carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnwrittenPart {
    /// No part of the edit is written.
    Whole,
    /// The expression is written and the edited value is not.
    Value,
    /// The value is written and the edited expression is not.
    Expression,
}

/// The edits a write does not carry, as the stale-solid check reads them.
#[derive(Debug, Default)]
pub(crate) struct UnwrittenEdits {
    /// Features whose construction edit the output does not carry: they are
    /// written with their stored definition record, or their written records
    /// read back as the source. Their construction edits are among the
    /// unwritten losses.
    pub(crate) kept_definitions: HashSet<FeatureId>,
    /// Parameters with an edit the output does not carry, and which part.
    pub(crate) parameters: HashMap<ParameterId, UnwrittenPart>,
    /// Configuration parameter values the output does not carry, by
    /// configuration id.
    pub(crate) configuration_values: HashSet<(String, ParameterId)>,
}

/// The decoded source document a write compares against, and the losses of
/// the edits the write keeps the stored record for.
pub(crate) struct SourceEdits<'source> {
    /// The decoded source document; `None` without one.
    source: Option<&'source CadIr>,
    /// Source features by id; `None` without a decoded source.
    features: Option<HashMap<&'source FeatureId, &'source Feature>>,
    /// Source parameters by id; `None` without a decoded source.
    parameters: Option<HashMap<&'source ParameterId, &'source DesignParameter>>,
    /// Caller parameters that differ from the source parameter of the same
    /// id, or that the source does not state. Every parameter without a
    /// decoded source.
    edited_parameters: HashSet<ParameterId>,
    /// The caller's native namespace equals the source's, so every change of
    /// the native records during the write is the writer's own.
    native_unedited: bool,
    /// The native records of the decoded source.
    source_native: Option<SldprtNative>,
    /// The unwritten-edit losses in the order they are found; a feature's
    /// loss is completed from `unwritten_features` at the end.
    unwritten: Vec<PendingLoss>,
    /// Features with an edit the output does not carry, and what their loss
    /// names.
    unwritten_features: HashMap<FeatureId, FeatureLoss>,
    written: UnwrittenEdits,
}

/// An unwritten-edit loss as it is collected.
enum PendingLoss {
    Note(LossNote),
    /// The loss of a feature whose stored definition record is kept.
    Feature(FeatureId),
}

/// Why a feature keeps its stored definition record, and the edits of its
/// construction that the record does not carry.
#[derive(Default)]
struct FeatureLoss {
    reasons: Vec<String>,
    dropped: Vec<String>,
}

/// What a write's edit comparison leaves for the rest of the write.
pub(crate) struct EditOutcome {
    /// The unwritten-edit losses.
    pub(crate) unwritten: Vec<LossNote>,
    /// The edits the output does not carry.
    pub(crate) written: UnwrittenEdits,
    /// The caller's parameters that differ from the source's.
    pub(crate) edited_parameters: HashSet<ParameterId>,
    /// The feature-input lanes of the decoded source, by id.
    pub(crate) source_lanes: HashMap<String, FeatureInputLane>,
}

/// One edited leaf of a feature's construction against its source feature.
pub(crate) struct ConstructionEdit {
    /// What a loss calls it.
    pub(crate) label: String,
    leaf: ConstructionLeaf,
}

enum ConstructionLeaf {
    /// A JSON pointer into the wire form of the definition.
    Definition(String),
    Dependencies,
    SourceProperty(NonBlankString),
}

impl ConstructionEdit {
    /// Whether this is the edit of the dependencies.
    pub(crate) fn is_dependencies(&self) -> bool {
        matches!(self.leaf, ConstructionLeaf::Dependencies)
    }

    /// The source feature `original` with this edit of `current` alone;
    /// `None` when that does not form a definition.
    pub(crate) fn applied(&self, original: &Feature, current: &Feature) -> Option<Feature> {
        let mut applied = original.clone();
        match &self.leaf {
            ConstructionLeaf::Definition(pointer) => {
                let mut wire = serde_json::to_value(original.evaluation.definition()).ok()?;
                let edited = serde_json::to_value(current.evaluation.definition()).ok()?;
                *wire.pointer_mut(pointer)? = edited.pointer(pointer)?.clone();
                applied
                    .evaluation
                    .set_definition(serde_json::from_value(wire).ok()?);
            }
            ConstructionLeaf::Dependencies => {
                applied.dependencies.clone_from(&current.dependencies);
            }
            ConstructionLeaf::SourceProperty(key) => match current.source_properties.get(key) {
                Some(value) => {
                    applied.source_properties.insert(key.clone(), value.clone());
                }
                None => {
                    applied.source_properties.remove(key);
                }
            },
        }
        Some(applied)
    }

    /// Whether `written`, the written records read back, states this leaf as
    /// `source` does. The values are the features' definitions in wire form;
    /// `dependencies_as_source` states whether the written dependencies are
    /// the source's.
    pub(crate) fn reads_back_as_source(
        &self,
        source: (&Feature, &serde_json::Value),
        written: (&Feature, &serde_json::Value),
        dependencies_as_source: bool,
    ) -> bool {
        match &self.leaf {
            ConstructionLeaf::Definition(pointer) => {
                source.1.pointer(pointer) == written.1.pointer(pointer)
            }
            ConstructionLeaf::Dependencies => dependencies_as_source,
            ConstructionLeaf::SourceProperty(key) => {
                source.0.source_properties.get(key) == written.0.source_properties.get(key)
            }
        }
    }
}

/// The edited leaves of `current`'s construction against `original`: each
/// differing leaf of the definition's wire form, the dependencies, and each
/// differing source property.
pub(crate) fn construction_edits(original: &Feature, current: &Feature) -> Vec<ConstructionEdit> {
    let mut edits = Vec::new();
    if original.evaluation.definition() != current.evaluation.definition() {
        match (
            serde_json::to_value(original.evaluation.definition()),
            serde_json::to_value(current.evaluation.definition()),
        ) {
            (Ok(original), Ok(current)) => {
                differing_leaves(Some(&original), Some(&current), "", "", &mut edits);
            }
            _ => edits.push(ConstructionEdit {
                label: "definition".into(),
                leaf: ConstructionLeaf::Definition(String::new()),
            }),
        }
    }
    if original.dependencies != current.dependencies {
        edits.push(ConstructionEdit {
            label: "dependencies".into(),
            leaf: ConstructionLeaf::Dependencies,
        });
    }
    let keys = original
        .source_properties
        .keys()
        .chain(current.source_properties.keys())
        .collect::<BTreeSet<_>>();
    for key in keys {
        if original.source_properties.get(key) != current.source_properties.get(key) {
            edits.push(ConstructionEdit {
                label: format!("source property {key}"),
                leaf: ConstructionLeaf::SourceProperty(key.clone()),
            });
        }
    }
    edits
}

/// Pushes a definition leaf for each place where `current` differs from
/// `original`. `pointer` is the JSON pointer of the place and `path` the
/// name a loss gives it. The `value` of a tagged value adds no name.
fn differing_leaves(
    original: Option<&serde_json::Value>,
    current: Option<&serde_json::Value>,
    pointer: &str,
    path: &str,
    edits: &mut Vec<ConstructionEdit>,
) {
    use serde_json::Value;

    if original == current {
        return;
    }
    match (original, current) {
        (Some(Value::Object(original)), Some(Value::Object(current))) => {
            let tagged = matches!(
                (original.get("kind"), current.get("kind")),
                (Some(Value::String(_)), Some(Value::String(_)))
            );
            if !(tagged && original.get("kind") != current.get("kind")) {
                let keys = original
                    .keys()
                    .chain(current.keys())
                    .collect::<BTreeSet<_>>();
                for key in keys {
                    let escaped = key.replace('~', "~0").replace('/', "~1");
                    let named = if tagged && key == "value" {
                        path.to_owned()
                    } else if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    differing_leaves(
                        original.get(key),
                        current.get(key),
                        &format!("{pointer}/{escaped}"),
                        &named,
                        edits,
                    );
                }
                return;
            }
        }
        (Some(Value::Array(original)), Some(Value::Array(current)))
            if original.len() == current.len() =>
        {
            for (index, (original, current)) in original.iter().zip(current).enumerate() {
                differing_leaves(
                    Some(original),
                    Some(current),
                    &format!("{pointer}/{index}"),
                    &format!("{path}[{index}]"),
                    edits,
                );
            }
            return;
        }
        _ => {}
    }
    edits.push(ConstructionEdit {
        label: if path.is_empty() {
            "definition".into()
        } else {
            format!("definition {path}")
        },
        leaf: ConstructionLeaf::Definition(pointer.to_owned()),
    });
}

/// The first labels of `labels`, and how many more there are.
fn listed(labels: &[String]) -> String {
    const SHOWN: usize = 8;
    let mut text = labels
        .iter()
        .take(SHOWN)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if labels.len() > SHOWN {
        text = format!("{text} and {} more", labels.len() - SHOWN);
    }
    text
}

impl<'source> SourceEdits<'source> {
    /// Compares the caller's document `input` and its native records with the
    /// retained source image decoded again.
    pub(crate) fn new(
        input: &CadIr,
        input_native: Option<&SldprtNative>,
        source: Option<&'source CadIr>,
    ) -> Result<Self, CodecError> {
        let Some(source) = source else {
            return Ok(Self::without_source(input));
        };
        let source_parameters = source
            .model
            .parameters
            .iter()
            .map(|parameter| (&parameter.id, parameter))
            .collect::<HashMap<_, _>>();
        let edited_parameters = input
            .model
            .parameters
            .iter()
            .filter(|parameter| source_parameters.get(&parameter.id).copied() != Some(*parameter))
            .map(|parameter| parameter.id.clone())
            .collect();
        let source_native = source
            .native
            .namespace("sldprt")
            .map(|namespace| SldprtNative::load(namespace).map_err(CodecError::from))
            .transpose()?;
        Ok(Self {
            source: Some(source),
            features: Some(
                source
                    .model
                    .features
                    .iter()
                    .map(|feature| (&feature.id, feature))
                    .collect(),
            ),
            parameters: Some(source_parameters),
            edited_parameters,
            native_unedited: source_native.as_ref() == input_native,
            source_native,
            unwritten: Vec::new(),
            unwritten_features: HashMap::new(),
            written: UnwrittenEdits::default(),
        })
    }

    /// No decoded source: every parameter counts as edited, and the native
    /// records count as edited by the caller.
    pub(crate) fn without_source(input: &CadIr) -> Self {
        Self {
            source: None,
            features: None,
            parameters: None,
            edited_parameters: input
                .model
                .parameters
                .iter()
                .map(|parameter| parameter.id.clone())
                .collect(),
            native_unedited: false,
            source_native: None,
            unwritten: Vec::new(),
            unwritten_features: HashMap::new(),
            written: UnwrittenEdits::default(),
        }
    }

    pub(crate) fn has_source(&self) -> bool {
        self.features.is_some()
    }

    /// The source feature of the same id.
    pub(crate) fn source_feature(&self, id: &FeatureId) -> Option<&'source Feature> {
        self.features.as_ref()?.get(id).copied()
    }

    /// The decoded source document.
    pub(crate) fn source(&self) -> Option<&'source CadIr> {
        self.source
    }

    /// The source parameters by id; `None` without a decoded source.
    pub(crate) fn source_parameters(
        &self,
    ) -> Option<&HashMap<&'source ParameterId, &'source DesignParameter>> {
        self.parameters.as_ref()
    }

    /// Whether `feature` states the source feature of the same id in every
    /// field but its name, under the same tree parent: the stored record
    /// states it, and only a rename is written.
    pub(crate) fn feature_unedited(&self, ir: &CadIr, feature: &Feature) -> bool {
        let (Some(source), Some(original)) = (self.source, self.source_feature(&feature.id)) else {
            return false;
        };
        if ir.model.feature_parent(&feature.id) != source.model.feature_parent(&feature.id) {
            return false;
        }
        let mut renamed = original.clone();
        renamed.name.clone_from(&feature.name);
        renamed == *feature
    }

    /// Whether feature `id` has another tree parent in `ir` than in the
    /// source document; `true` without a decoded source.
    pub(crate) fn feature_parent_moved(&self, ir: &CadIr, id: &FeatureId) -> bool {
        self.source
            .is_none_or(|source| source.model.feature_parent(id) != ir.model.feature_parent(id))
    }

    pub(crate) fn parameter_edited(&self, id: &ParameterId) -> bool {
        self.edited_parameters.contains(id)
    }

    pub(crate) fn native_unedited(&self) -> bool {
        self.native_unedited
    }

    /// Reports the part `part` of an edit of parameter `id` that the output
    /// does not carry.
    pub(crate) fn parameter_unwritten(
        &mut self,
        id: &ParameterId,
        part: UnwrittenPart,
        message: String,
    ) {
        let recorded = self.written.parameters.entry(id.clone()).or_insert(part);
        if *recorded != part {
            *recorded = UnwrittenPart::Whole;
        }
        self.unwritten.push(PendingLoss::Note(
            SldprtLossCode::WriterParameterEditUnwritten.note(message),
        ));
    }

    /// Reports a detail of a parameter edit the output does not carry; the
    /// value and the expression are written.
    pub(crate) fn parameter_detail_unwritten(&mut self, message: String) {
        self.unwritten.push(PendingLoss::Note(
            SldprtLossCode::WriterParameterEditUnwritten.note(message),
        ));
    }

    /// Reports an edit of the value of parameter `parameter` in configuration
    /// `configuration` that the output does not carry.
    pub(crate) fn configuration_value_unwritten(
        &mut self,
        configuration: &str,
        parameter: &ParameterId,
        message: String,
    ) {
        self.written
            .configuration_values
            .insert((configuration.to_owned(), parameter.clone()));
        self.unwritten.push(PendingLoss::Note(
            SldprtLossCode::WriterParameterEditUnwritten.note(message),
        ));
    }

    /// Reports a feature edit the output does not carry: the feature keeps
    /// its stored definition record, and its one loss names every reason.
    /// Without a decoded source the writer cannot keep a stored record, so
    /// the edit is refused.
    pub(crate) fn feature_unwritten(
        &mut self,
        id: &FeatureId,
        reason: String,
    ) -> Result<(), CodecError> {
        if !self.has_source() {
            return Err(CodecError::NotImplemented(reason));
        }
        if !self.unwritten_features.contains_key(id) {
            self.unwritten.push(PendingLoss::Feature(id.clone()));
        }
        self.unwritten_features
            .entry(id.clone())
            .or_default()
            .reasons
            .push(reason);
        Ok(())
    }

    /// Records that `feature` is written with its stored definition record,
    /// whatever its neutral definition states. The stored record keeps its
    /// stored operands and source properties, so the feature's loss names
    /// every edit of its construction.
    pub(crate) fn keep_stored_definition(&mut self, feature: &Feature) -> Result<(), CodecError> {
        let dropped = self
            .source_feature(&feature.id)
            .map(|original| construction_edits(original, feature))
            .unwrap_or_default()
            .into_iter()
            .map(|edit| edit.label)
            .collect::<Vec<_>>();
        if !dropped.is_empty() {
            if !self.unwritten_features.contains_key(&feature.id) {
                self.feature_unwritten(
                    &feature.id,
                    format!(
                        "SLDPRT feature {} keeps its stored definition record",
                        feature.id
                    ),
                )?;
            }
            if let Some(loss) = self.unwritten_features.get_mut(&feature.id) {
                loss.dropped = dropped;
            }
        }
        self.written.kept_definitions.insert(feature.id.clone());
        Ok(())
    }

    /// Reports the construction edits `labels` of feature `id` that its
    /// written records read back as the source states them. `whole` states
    /// that no other edit of its construction is written: the feature then
    /// counts as written with its stored definition.
    pub(crate) fn construction_reads_back_as_stored(
        &mut self,
        id: &FeatureId,
        labels: &[String],
        whole: bool,
    ) {
        let rest = if whole {
            "they read back as stored"
        } else {
            "they read back as stored, and the rest of its edit is written"
        };
        self.unwritten.push(PendingLoss::Note(
            SldprtLossCode::WriterFeatureEditUnwritten.note(format!(
                "SLDPRT feature {id} changes {}, which its written records do not state; {rest}.",
                listed(labels)
            )),
        ));
        if whole {
            self.written.kept_definitions.insert(id.clone());
        }
    }

    /// Whether an edit of feature `id` is not written.
    pub(crate) fn feature_edit_unwritten(&self, id: &FeatureId) -> bool {
        self.unwritten_features.contains_key(id)
    }

    /// Whether feature `id` keeps stored operands: its definition record is
    /// kept, its construction edit is not written, or an edit of it is not
    /// written.
    pub(crate) fn operands_kept(&self, id: &FeatureId) -> bool {
        self.written.kept_definitions.contains(id) || self.unwritten_features.contains_key(id)
    }

    /// Whether the caller's feature states the definition and dependencies of
    /// the source feature of the same id.
    pub(crate) fn feature_construction_unedited(&self, feature: &Feature) -> bool {
        self.source_feature(&feature.id).is_some_and(|original| {
            original.dependencies == feature.dependencies
                && original.evaluation.definition() == feature.evaluation.definition()
        })
    }

    /// The unwritten-edit losses, the edits the output does not carry, the
    /// edited parameters, and the source's feature-input lanes.
    pub(crate) fn into_parts(self) -> EditOutcome {
        let Self {
            source_native,
            unwritten,
            mut unwritten_features,
            written,
            edited_parameters,
            ..
        } = self;
        let unwritten = unwritten
            .into_iter()
            .filter_map(|pending| match pending {
                PendingLoss::Note(note) => Some(note),
                PendingLoss::Feature(id) => {
                    let loss = unwritten_features.remove(&id)?;
                    let mut message = format!(
                        "{}; the written record keeps the stored state",
                        loss.reasons.join("; ")
                    );
                    if !loss.dropped.is_empty() {
                        message.push_str(" and does not carry its edits of ");
                        message.push_str(&listed(&loss.dropped));
                    }
                    message.push('.');
                    Some(SldprtLossCode::WriterFeatureEditUnwritten.note(message))
                }
            })
            .collect();
        EditOutcome {
            unwritten,
            written,
            edited_parameters,
            source_lanes: source_native
                .map(|native| {
                    native
                        .feature_input_lanes
                        .into_iter()
                        .map(|lane| (lane.id.clone(), lane))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}
