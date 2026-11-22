// SPDX-License-Identifier: Apache-2.0
//! What the caller changed against the decoded source document, and the
//! edits the native records cannot carry.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{Feature, FeatureId, ParameterId};
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::SldprtLossCode;
use crate::native::SldprtNative;

/// The decoded source document a write compares against, and the losses of
/// the edits the write keeps the stored record for.
pub(crate) struct SourceEdits<'source> {
    /// Source features by id; `None` without a decoded source.
    features: Option<HashMap<&'source FeatureId, &'source Feature>>,
    /// Caller parameters that differ from the source parameter of the same
    /// id, or that the source does not state. Every parameter without a
    /// decoded source.
    edited_parameters: HashSet<ParameterId>,
    /// The caller's native namespace equals the source's, so every change of
    /// the native records during the write is the writer's own.
    native_unedited: bool,
    unwritten: Vec<LossNote>,
    /// Features written with their stored definition record.
    kept_definitions: HashSet<FeatureId>,
    /// Features with an edit the output does not carry.
    unwritten_features: HashSet<FeatureId>,
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
            features: Some(
                source
                    .model
                    .features
                    .iter()
                    .map(|feature| (&feature.id, feature))
                    .collect(),
            ),
            edited_parameters,
            native_unedited: source_native.as_ref() == input_native,
            unwritten: Vec::new(),
            kept_definitions: HashSet::new(),
            unwritten_features: HashSet::new(),
        })
    }

    /// No decoded source: every parameter counts as edited, and the native
    /// records count as edited by the caller.
    pub(crate) fn without_source(input: &CadIr) -> Self {
        Self {
            features: None,
            edited_parameters: input
                .model
                .parameters
                .iter()
                .map(|parameter| parameter.id.clone())
                .collect(),
            native_unedited: false,
            unwritten: Vec::new(),
            kept_definitions: HashSet::new(),
            unwritten_features: HashSet::new(),
        }
    }

    pub(crate) fn has_source(&self) -> bool {
        self.features.is_some()
    }

    /// The source feature of the same id.
    pub(crate) fn source_feature(&self, id: &FeatureId) -> Option<&'source Feature> {
        self.features.as_ref()?.get(id).copied()
    }

    pub(crate) fn parameter_edited(&self, id: &ParameterId) -> bool {
        self.edited_parameters.contains(id)
    }

    pub(crate) fn native_unedited(&self) -> bool {
        self.native_unedited
    }

    /// Reports a parameter edit the output does not carry.
    pub(crate) fn parameter_unwritten(&mut self, message: String) {
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

    /// Records that feature `id` is written with its stored definition
    /// record, whatever its neutral definition states.
    pub(crate) fn keep_stored_definition(&mut self, id: &FeatureId) {
        self.kept_definitions.insert(id.clone());
    }

    /// Whether feature `id` keeps stored operands: its definition record is
    /// kept, or an edit of it is not written.
    pub(crate) fn operands_kept(&self, id: &FeatureId) -> bool {
        self.kept_definitions.contains(id) || self.unwritten_features.contains(id)
    }

    /// Whether the caller's feature states the definition and dependencies of
    /// the source feature of the same id.
    pub(crate) fn feature_construction_unedited(&self, feature: &Feature) -> bool {
        self.source_feature(&feature.id).is_some_and(|original| {
            original.dependencies == feature.dependencies
                && original.evaluation.definition() == feature.evaluation.definition()
        })
    }

    /// The unwritten-edit losses, the features written with their stored
    /// definition record, and the edited parameters.
    pub(crate) fn into_parts(self) -> (Vec<LossNote>, HashSet<FeatureId>, HashSet<ParameterId>) {
        (
            self.unwritten,
            self.kept_definitions,
            self.edited_parameters,
        )
    }
}
