// SPDX-License-Identifier: Apache-2.0
//! What the caller changed against the decoded source document, and the
//! edits the native records cannot carry.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::parameters::DesignParameter;
use cadmpeg_ir::features::{Feature, FeatureId, ParameterId};
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::SldprtLossCode;
use crate::native::SldprtNative;

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
    /// Features written with their stored definition record: their
    /// definition edits are among the unwritten losses.
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
    unwritten: Vec<LossNote>,
    /// Features with an edit the output does not carry.
    unwritten_features: HashSet<FeatureId>,
    written: UnwrittenEdits,
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
            unwritten: Vec::new(),
            unwritten_features: HashSet::new(),
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
            unwritten: Vec::new(),
            unwritten_features: HashSet::new(),
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
        self.unwritten
            .push(SldprtLossCode::WriterParameterEditUnwritten.note(message));
    }

    /// Reports a detail of a parameter edit the output does not carry; the
    /// value and the expression are written.
    pub(crate) fn parameter_detail_unwritten(&mut self, message: String) {
        self.unwritten
            .push(SldprtLossCode::WriterParameterEditUnwritten.note(message));
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
        self.unwritten
            .push(SldprtLossCode::WriterParameterEditUnwritten.note(message));
    }

    /// Reports a feature edit the output does not carry. Without a decoded
    /// source the writer cannot keep a stored record, so the edit is refused.
    pub(crate) fn feature_unwritten(
        &mut self,
        id: &FeatureId,
        reason: String,
    ) -> Result<(), CodecError> {
        if !self.has_source() {
            return Err(CodecError::NotImplemented(reason));
        }
        self.unwritten_features.insert(id.clone());
        self.unwritten
            .push(SldprtLossCode::WriterFeatureEditUnwritten.note(format!(
                "{reason}; the written record keeps the stored state."
            )));
        Ok(())
    }

    /// Records that `feature` is written with its stored definition record,
    /// whatever its neutral definition states. The stored record keeps its
    /// stored operands, so an edit of its dependencies is not written either.
    pub(crate) fn keep_stored_definition(&mut self, feature: &Feature) -> Result<(), CodecError> {
        if !self.unwritten_features.contains(&feature.id)
            && self
                .source_feature(&feature.id)
                .is_some_and(|original| original.dependencies != feature.dependencies)
        {
            self.feature_unwritten(
                &feature.id,
                format!(
                    "SLDPRT feature {} changes its dependencies, and its stored definition record states its stored operands",
                    feature.id
                ),
            )?;
        }
        self.written.kept_definitions.insert(feature.id.clone());
        Ok(())
    }

    /// Whether an edit of feature `id` is not written.
    pub(crate) fn feature_edit_unwritten(&self, id: &FeatureId) -> bool {
        self.unwritten_features.contains(id)
    }

    /// Whether feature `id` keeps stored operands: its definition record is
    /// kept, or an edit of it is not written.
    pub(crate) fn operands_kept(&self, id: &FeatureId) -> bool {
        self.written.kept_definitions.contains(id) || self.unwritten_features.contains(id)
    }

    /// Whether the caller's feature states the definition and dependencies of
    /// the source feature of the same id.
    pub(crate) fn feature_construction_unedited(&self, feature: &Feature) -> bool {
        self.source_feature(&feature.id).is_some_and(|original| {
            original.dependencies == feature.dependencies
                && original.evaluation.definition() == feature.evaluation.definition()
        })
    }

    /// The unwritten-edit losses, the edits the output does not carry, and
    /// the edited parameters.
    pub(crate) fn into_parts(self) -> (Vec<LossNote>, UnwrittenEdits, HashSet<ParameterId>) {
        (self.unwritten, self.written, self.edited_parameters)
    }
}
