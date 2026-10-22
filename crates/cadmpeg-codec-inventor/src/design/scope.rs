// SPDX-License-Identifier: Apache-2.0
//! `PmDc` model roots, parameter tables, and the parameter scopes they define.
//!
//! A parameter's context reference selects the parameter table that owns it.
//! A parameter table's context reference selects the model root that owns the
//! table. A model root names the B-rep segment that carries its body: the
//! document model names the `PmBrepSegmentType` segment and the flat-pattern
//! model names the `MbBrepSegmentType` segment.

use std::collections::HashMap;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
    FeatureOperation, NativeFeatureKind,
};
use serde::{Deserialize, Serialize};

use crate::pmdc::{inventor_id, reference_list, Cursor, PmDcReference, PmDcReferenceList};
use crate::record_identity::{Located, RecordPayload};

/// Type identifier of a `PmDc` model root.
pub(crate) const MODEL_ROOT_TYPE: [u8; 16] = inventor_id(0x9087_4d16);
/// Type identifier of a `PmDc` parameter table.
pub(crate) const PARAMETER_TABLE_TYPE: [u8; 16] = inventor_id(0x9087_4d63);

/// Number of u32 values between a model root's member list and its segment name.
const MODEL_ROOT_VALUE_COUNT: usize = 15;
/// Index of the parameter-table reference among the model root values.
const MODEL_ROOT_TABLE_VALUE: usize = 3;
/// Index of the null or repeated parameter-table reference among the model root values.
const MODEL_ROOT_TABLE_REPEAT_VALUE: usize = 9;
/// Native family tag of the flat-pattern model feature.
pub(crate) const FLAT_PATTERN_KIND: &str = "FlatPattern";
/// Source tag of the flat-pattern model feature.
const FLAT_PATTERN_TAG: &str = "flat_pattern";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcModelRootPayload {
    pub(crate) save_version_major: u8,
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) next: PmDcReference,
    pub(crate) flags: u32,
    pub(crate) identifier: String,
    pub(crate) name: String,
    pub(crate) members: PmDcReferenceList,
    pub(crate) values: [u32; MODEL_ROOT_VALUE_COUNT],
    pub(crate) parameter_table: PmDcReference,
    pub(crate) brep_segment: String,
    pub(crate) scale: cadmpeg_ir::scalar::FiniteReal,
    pub(crate) terminal_value: u32,
}

pub(crate) type PmDcModelRoot = Located<PmDcModelRootPayload>;

impl RecordPayload for PmDcModelRootPayload {
    const KIND: &'static str = "model-root";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcNamedReference {
    pub(crate) name: String,
    pub(crate) reference: PmDcReference,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcParameterTablePayload {
    pub(crate) save_version_major: u8,
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) next: PmDcReference,
    pub(crate) flags: u32,
    pub(crate) context: PmDcReference,
    pub(crate) members: PmDcReferenceList,
    pub(crate) names_metadata: Option<[u32; 2]>,
    pub(crate) names: Vec<PmDcNamedReference>,
    pub(crate) suffix_len: u32,
}

pub(crate) type PmDcParameterTable = Located<PmDcParameterTablePayload>;

impl RecordPayload for PmDcParameterTablePayload {
    const KIND: &'static str = "parameter-table";
}

/// The role of a B-rep segment a model root can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrepSegmentRole {
    /// The `PmBrepSegmentType` segment: the document model.
    Document,
    /// The `MbBrepSegmentType` segment: the flat-pattern model.
    FlatPattern,
}

/// One B-rep segment display name and the model role it selects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrepSegmentName {
    pub(crate) display_name: String,
    pub(crate) role: BrepSegmentRole,
}

/// The owner of the parameters in one parameter table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParameterScope {
    /// Document parameters.
    Document,
    /// Parameters owned by the flat-pattern model feature.
    FlatPattern(FeatureId),
}

/// The scope of one parameter table and the parameters its name map lists.
#[derive(Debug)]
struct TableScope {
    scope: ParameterScope,
    /// Parameter names keyed by the record ordinal the name map references.
    names: HashMap<u32, String>,
}

/// Parameter scopes keyed by segment token and parameter-table record ordinal.
#[derive(Debug, Default)]
pub(crate) struct ParameterScopes {
    tables: HashMap<String, HashMap<u32, TableScope>>,
}

impl ParameterScopes {
    /// The scope of the parameter record `parameter_ordinal` named `name`
    /// whose context reference is `context` in the segment `token`.
    ///
    /// The parameter has a scope only when its table has an established owner
    /// and the table's name map lists the parameter under its own name. `None`
    /// means the owner is not established.
    pub(crate) fn of(
        &self,
        token: &str,
        context: PmDcReference,
        parameter_ordinal: u32,
        name: &str,
    ) -> Option<&ParameterScope> {
        let table = self.tables.get(token)?.get(&context.record_ordinal()?)?;
        (table.names.get(&parameter_ordinal).map(String::as_str) == Some(name))
            .then_some(&table.scope)
    }

    fn insert(
        &mut self,
        token: &str,
        table_ordinal: u32,
        scope: ParameterScope,
        named: &[PmDcNamedReference],
    ) {
        let names = named
            .iter()
            .filter_map(|entry| {
                let ordinal = entry.reference.record_ordinal()?;
                Some((ordinal, entry.name.clone()))
            })
            .collect();
        self.tables
            .entry(token.to_owned())
            .or_default()
            .insert(table_ordinal, TableScope { scope, names });
    }

    /// Scopes in which the one table `table_ordinal` of segment `token` holds
    /// the named document parameters `named`.
    #[cfg(test)]
    pub(crate) fn document_table(
        token: &str,
        table_ordinal: u32,
        named: &[PmDcNamedReference],
    ) -> Self {
        let mut scopes = Self::default();
        scopes.insert(token, table_ordinal, ParameterScope::Document, named);
        scopes
    }
}

pub(crate) fn parse_model_root(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcModelRootPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header_value = cursor.u32("model root header value")?;
    let header_id = cursor.u16("model root header id")?;
    let next = cursor.reference("model root next reference")?;
    let flags = cursor.u32("model root flags")?;
    let identifier = cursor.take_array::<16>("model root identifier")?;
    let name = cursor.utf16(ctx, "model root name")?;
    let members = reference_list(&mut cursor, 2, "model root member list")?;
    let values = cursor.u32_array::<MODEL_ROOT_VALUE_COUNT>("model root values")?;
    let table = values[MODEL_ROOT_TABLE_VALUE];
    let repeat = values[MODEL_ROOT_TABLE_REPEAT_VALUE];
    if repeat != 0 && repeat != table {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc model root repeats parameter table {table:#010x} as {repeat:#010x}"
        )));
    }
    let brep_segment = cursor.utf16(ctx, "model root B-rep segment name")?;
    let scale = cursor.f64("model root scale")?;
    let terminal_value = cursor.u32("model root terminal value")?;
    cursor.finish("model root")?;
    Ok(PmDcModelRootPayload {
        save_version_major: version,
        header_value,
        header_id,
        next,
        flags,
        identifier: crate::pmdc::type_id_string(identifier),
        name,
        members,
        values,
        parameter_table: PmDcReference::from_packed(table),
        brep_segment,
        scale,
        terminal_value,
    })
}

pub(crate) fn parse_parameter_table(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcParameterTablePayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header_value = cursor.u32("parameter table header value")?;
    let header_id = cursor.u16("parameter table header id")?;
    let next = cursor.reference("parameter table next reference")?;
    let flags = cursor.u32("parameter table flags")?;
    let context = cursor.reference("parameter table context reference")?;
    let members = reference_list(&mut cursor, 2, "parameter table member list")?;
    let marker = [
        cursor.u16("parameter table name-map marker 0")?,
        cursor.u16("parameter table name-map marker 1")?,
    ];
    if marker != [6, 0x3000] {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc parameter table name-map marker is {marker:?}"
        )));
    }
    let count = usize::try_from(cursor.u32("parameter table name count")?)
        .map_err(|_| CodecError::Malformed("Inventor numeric value exceeds target range".into()))?;
    // Each entry holds at least a u32 code-unit count and a u32 reference.
    if count > cursor.remaining() / 8 {
        return Err(CodecError::malformed(
            "Inventor PmDc parameter table name count exceeds remaining payload",
        ));
    }
    let names_metadata = if count == 0 {
        None
    } else {
        Some(cursor.u32_array::<2>("parameter table name-map metadata")?)
    };
    let mut names = Vec::new();
    for _ in 0..count {
        let name = cursor.utf16(ctx, "parameter table name")?;
        let reference = cursor.reference("parameter table named reference")?;
        names.push(PmDcNamedReference { name, reference });
    }
    let suffix_len = u32::try_from(cursor.remaining())
        .map_err(|_| CodecError::Malformed("Inventor numeric value exceeds target range".into()))?;
    Ok(PmDcParameterTablePayload {
        save_version_major: version,
        header_value,
        header_id,
        next,
        flags,
        context,
        members,
        names_metadata,
        names,
        suffix_len,
    })
}

/// The role of a model root, from the B-rep segment it names.
///
/// A root resolves only when exactly one B-rep segment of the document has its
/// segment name.
pub(crate) fn root_role(
    root: &PmDcModelRootPayload,
    segments: &[BrepSegmentName],
) -> Option<BrepSegmentRole> {
    let mut matches = segments
        .iter()
        .filter(|segment| segment.display_name == root.brep_segment);
    let first = matches.next()?;
    matches.next().is_none().then_some(first.role)
}

/// The feature identity of the model a root defines.
pub(crate) fn model_feature_id(root: &PmDcModelRoot) -> Result<FeatureId, CodecError> {
    Ok(FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "design", "feature"),
        root.identity.key()?,
    ))
}

/// Resolves the owner of every parameter table.
///
/// A segment admits scopes only when exactly one of its model roots names the
/// document B-rep segment. A table whose context is that root holds document
/// parameters. A table whose context is a root naming the flat-pattern B-rep
/// segment, and which that root selects as its parameter table, holds the
/// flat-pattern parameters. Every other table has no established owner.
pub(crate) fn resolve_scopes(
    roots: &[PmDcModelRoot],
    tables: &[PmDcParameterTable],
    segments: &[BrepSegmentName],
) -> Result<ParameterScopes, CodecError> {
    let mut by_root = HashMap::new();
    let mut document_roots = HashMap::<&str, usize>::new();
    for root in roots {
        let Some(role) = root_role(root, segments) else {
            continue;
        };
        let token = root.identity.segment_token.as_str();
        if role == BrepSegmentRole::Document {
            *document_roots.entry(token).or_default() += 1;
        }
        by_root.insert((token, root.identity.record_ordinal), (role, root));
    }
    let mut scopes = ParameterScopes::default();
    for table in tables {
        let token = table.identity.segment_token.as_str();
        if document_roots.get(token) != Some(&1) {
            continue;
        }
        let Some(root_ordinal) = table.context.record_ordinal() else {
            continue;
        };
        let Some((role, root)) = by_root.get(&(token, root_ordinal)) else {
            continue;
        };
        let scope = match role {
            BrepSegmentRole::Document => ParameterScope::Document,
            BrepSegmentRole::FlatPattern => {
                if root.parameter_table.record_ordinal() != Some(table.identity.record_ordinal) {
                    continue;
                }
                ParameterScope::FlatPattern(model_feature_id(root)?)
            }
        };
        scopes.insert(token, table.identity.record_ordinal, scope, &table.names);
    }
    Ok(scopes)
}

/// Projects one native-family feature for each flat-pattern model root of a
/// segment that has exactly one document model root.
///
/// The flat-pattern model is derived from the folded document model, so its
/// features follow every projected document feature. `first_ordinal` is the
/// first ordinal after them; roots take consecutive ordinals in record order.
pub(crate) fn project_model_features(
    ctx: &DecodeContext<'_>,
    roots: &[PmDcModelRoot],
    segments: &[BrepSegmentName],
    first_ordinal: u64,
) -> Result<Vec<Feature>, CodecError> {
    let mut document_roots = HashMap::<&str, usize>::new();
    for root in roots {
        if root_role(root, segments) == Some(BrepSegmentRole::Document) {
            *document_roots
                .entry(root.identity.segment_token.as_str())
                .or_default() += 1;
        }
    }
    let mut features = Vec::new();
    let mut ordinal = first_ordinal;
    for root in roots {
        if root_role(root, segments) != Some(BrepSegmentRole::FlatPattern)
            || document_roots.get(root.identity.segment_token.as_str()) != Some(&1)
        {
            continue;
        }
        ctx.charge_entities(1, "project Inventor flat-pattern feature")?;
        let native_ref = root.id();
        features.push(Feature {
            id: model_feature_id(root)?,
            needs_recompute: false,
            ordinal,
            name: None,
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: Some(FLAT_PATTERN_TAG.into()),
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::Native {
                    kind: NativeFeatureKind::Other(FLAT_PATTERN_KIND.into()),
                    parameters: std::collections::BTreeMap::new(),
                },
            )),
            native_ref: Some(native_ref),
        });
        ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| CodecError::malformed("Inventor feature ordinal overflows"))?;
    }
    Ok(features)
}

#[cfg(test)]
mod tests;
