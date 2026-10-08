// SPDX-License-Identifier: Apache-2.0
//! Feature-state recipes, operation names, and model reference names.

#[cfg(test)]
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::schema::SchemaClass;
use crate::psb;

/// Exact procedural recipe stored in a feature-state record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FeatureRecipe {
    /// Additive linear section sweep named `protextrude`.
    ProtrudeExtrude,
    /// Subtractive linear section sweep named `cutextrude`.
    CutExtrude,
    /// Additive rotational section sweep named `protrevolve`.
    ProtrudeRevolve,
    /// Subtractive rotational section sweep named `cutrevolve`.
    CutRevolve,
}

/// Geometry family selected by a procedural feature recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FeatureRecipeKind {
    /// Linear section sweep.
    Extrude,
    /// Rotational section sweep.
    Revolve,
}

/// Boolean effect selected by a procedural feature recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FeatureRecipeEffect {
    /// Material-adding operation.
    Protrude,
    /// Material-removing operation.
    Cut,
}

impl FeatureRecipe {
    /// Exact stored recipe name without its NUL terminator.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::ProtrudeExtrude => "protextrude",
            Self::CutExtrude => "cutextrude",
            Self::ProtrudeRevolve => "protrevolve",
            Self::CutRevolve => "cutrevolve",
        }
    }

    /// Section-sweep geometry family.
    pub(crate) const fn kind(self) -> FeatureRecipeKind {
        match self {
            Self::ProtrudeExtrude | Self::CutExtrude => FeatureRecipeKind::Extrude,
            Self::ProtrudeRevolve | Self::CutRevolve => FeatureRecipeKind::Revolve,
        }
    }

    /// Boolean effect of the section sweep.
    pub(crate) const fn effect(self) -> FeatureRecipeEffect {
        match self {
            Self::ProtrudeExtrude | Self::ProtrudeRevolve => FeatureRecipeEffect::Protrude,
            Self::CutExtrude | Self::CutRevolve => FeatureRecipeEffect::Cut,
        }
    }
}

const FEATURE_RECIPES: &[(&[u8], FeatureRecipe)] = &[
    (b"protextrude\0", FeatureRecipe::ProtrudeExtrude),
    (b"cutextrude\0", FeatureRecipe::CutExtrude),
    (b"protrevolve\0", FeatureRecipe::ProtrudeRevolve),
    (b"cutrevolve\0", FeatureRecipe::CutRevolve),
];

/// Stored identifier keyword, preserving `id` versus `ID`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdKeyword {
    /// Lowercase `id`.
    Id,
    /// Uppercase `ID`.
    ID,
}

impl IdKeyword {
    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        match bytes {
            b"id" => Some(Self::Id),
            b"ID" => Some(Self::ID),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::ID => "ID",
        }
    }
}

/// Source of a feature-operation display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OperationName {
    /// Stored `<Kind> id <N>` display name.
    Stored {
        /// Exact stored operation-name bytes excluding the NUL terminator.
        bytes: Vec<u8>,
        /// Stored identifier keyword.
        keyword: IdKeyword,
        /// Optional stored-name byte immediately preceding the family name.
        prefix: Option<u8>,
    },
    /// Derived operation name with no stored display name.
    Derived,
}

impl OperationName {
    pub(crate) fn display_name_stored(&self) -> bool {
        matches!(self, Self::Stored { .. })
    }

    #[cfg(test)]
    pub(crate) fn stored_name(&self) -> Option<String> {
        self.stored_name_bytes()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
    }

    pub(crate) fn stored_name_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Stored { bytes, .. } => Some(bytes),
            Self::Derived => None,
        }
    }

    pub(crate) fn identifier_keyword(&self) -> Option<&str> {
        match self {
            Self::Stored { keyword, .. } => Some(keyword.as_str()),
            Self::Derived => None,
        }
    }

    pub(crate) fn stored_name_prefix(&self) -> Option<u8> {
        match self {
            Self::Stored { prefix, .. } => *prefix,
            Self::Derived => None,
        }
    }
}

/// Operation-family kind named by a feature-state record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OperationKind {
    /// Family name taken from a stored display name.
    Stored(String),
    /// Linear section-sweep family.
    Extrude,
    /// Rotational section-sweep family.
    Revolve,
    /// Consensus or conflict fallback.
    Native,
}

impl cadmpeg_core::decode::cost::DecodeCost for OperationKind {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        use cadmpeg_core::decode::cost::DecodeCost;

        match self {
            Self::Stored(value) => DecodeCost::decode_cost(&(0_u8, value.as_str()), ctx, operation),
            Self::Extrude => DecodeCost::decode_cost(&(1_u8,), ctx, operation),
            Self::Revolve => DecodeCost::decode_cost(&(2_u8,), ctx, operation),
            Self::Native => DecodeCost::decode_cost(&(3_u8,), ctx, operation),
        }
    }
}

impl OperationKind {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Stored(value) => value,
            Self::Extrude => "Extrude",
            Self::Revolve => "Revolve",
            Self::Native => "Native Feature",
        }
    }

    fn from_recipe(recipe: FeatureRecipe) -> Self {
        match recipe.kind() {
            FeatureRecipeKind::Extrude => Self::Extrude,
            FeatureRecipeKind::Revolve => Self::Revolve,
        }
    }
}

/// DEPDB recipe prefix pairing a schema class with a parent feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DepdbPrefix {
    /// Root feature-definition schema class.
    pub(crate) schema: SchemaClass,
    /// Previous or parent feature identifier.
    pub(crate) parent: u32,
}

/// Resolution of a feature's procedural recipe in one stored source state.
///
/// A source state that competes with another may still name the recipe it
/// stored; that candidate is source evidence, not a resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecipeState {
    /// No recipe is stored.
    None,
    /// One recipe is resolved.
    Resolved(FeatureRecipe),
    /// Competing recipes prevent resolution.
    Conflicting {
        /// Candidate retained by this source state.
        candidate: Option<FeatureRecipe>,
    },
}

impl RecipeState {
    /// Resolved recipe available to geometry consumers.
    pub(super) fn resolved(self) -> Option<FeatureRecipe> {
        match self {
            Self::Resolved(recipe) => Some(recipe),
            Self::None | Self::Conflicting { .. } => None,
        }
    }

    /// Stored recipe candidate retained for source records.
    pub(crate) fn candidate(self) -> Option<FeatureRecipe> {
        match self {
            Self::Resolved(recipe) => Some(recipe),
            Self::Conflicting { candidate } => candidate,
            Self::None => None,
        }
    }

    /// Whether competing recipes prevent resolution.
    pub(crate) fn is_conflicting(self) -> bool {
        matches!(self, Self::Conflicting { .. })
    }
}

impl From<Option<FeatureRecipe>> for RecipeState {
    fn from(recipe: Option<FeatureRecipe>) -> Self {
        recipe.map_or(Self::None, Self::Resolved)
    }
}

/// Resolution of a feature's procedural recipe across its stored states.
///
/// The projection selects one state per feature, so competing recipes leave
/// no candidate to carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecipeResolution {
    /// No recipe is stored.
    None,
    /// One recipe is resolved.
    Resolved(FeatureRecipe),
    /// Competing recipes prevent resolution.
    Conflicting,
}

impl RecipeResolution {
    /// Resolved recipe available to geometry consumers.
    pub(crate) fn resolved(self) -> Option<FeatureRecipe> {
        match self {
            Self::Resolved(recipe) => Some(recipe),
            Self::None | Self::Conflicting => None,
        }
    }

    /// Whether competing recipes prevent resolution.
    pub(crate) fn is_conflicting(self) -> bool {
        matches!(self, Self::Conflicting)
    }
}

impl From<Option<FeatureRecipe>> for RecipeResolution {
    fn from(recipe: Option<FeatureRecipe>) -> Self {
        recipe.map_or(Self::None, Self::Resolved)
    }
}

impl From<RecipeState> for RecipeResolution {
    fn from(state: RecipeState) -> Self {
        match state {
            RecipeState::None => Self::None,
            RecipeState::Resolved(recipe) => Self::Resolved(recipe),
            RecipeState::Conflicting { .. } => Self::Conflicting,
        }
    }
}

mod sealed {
    /// Closed set of procedural recipe forms.
    pub(crate) trait Sealed {}

    impl Sealed for super::RecipeState {}
    impl Sealed for super::RecipeResolution {}
}

/// Stored or resolved procedural recipe form.
pub(crate) trait RecipeForm: sealed::Sealed {}

impl RecipeForm for RecipeState {}
impl RecipeForm for RecipeResolution {}

/// One stored feature-state record, before current-state selection.
pub(crate) type FeatureOperationState = FeatureOperation<RecipeState>;

/// Feature-operation family named by a feature-state record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureOperation<R: RecipeForm = RecipeResolution> {
    /// Numeric feature identifier following `id` in the stored name.
    pub(crate) feature_id: u32,
    /// Operation-family kind.
    pub(crate) kind: OperationKind,
    /// Display-name source.
    pub(crate) name: OperationName,
    /// Procedural recipe resolution for this state.
    pub(crate) recipe: R,
    /// Multiple stored display states prevent a unique current-state selection.
    pub(crate) display_state_conflict: bool,
    /// DEPDB recipe prefix, when present.
    pub(crate) depdb: Option<DepdbPrefix>,
    /// Byte offset of the operation name in the original stream.
    pub(crate) offset: usize,
    /// Byte offset including the optional stored-name prefix.
    pub(crate) state_offset: usize,
}

impl<R: RecipeForm> FeatureOperation<R> {
    pub(crate) fn display_name_stored(&self) -> bool {
        self.name.display_name_stored()
    }

    #[cfg(test)]
    pub(crate) fn stored_name(&self) -> Option<String> {
        self.name.stored_name()
    }

    pub(crate) fn stored_name_prefix(&self) -> Option<u8> {
        self.name.stored_name_prefix()
    }

    pub(crate) fn root_schema_class(&self) -> Option<SchemaClass> {
        self.depdb.map(|prefix| prefix.schema)
    }

    pub(crate) fn parent_feature_id(&self) -> Option<u32> {
        self.depdb.map(|prefix| prefix.parent)
    }
}

/// Feature name joined to its model feature identifier by `mdl_feat_ref_info_new`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureReferenceName {
    /// Numeric model feature identifier.
    pub(crate) feature_id: u32,
    /// Exact stored feature-name bytes excluding the NUL terminator.
    pub(crate) name_bytes: Vec<u8>,
    /// Reference-database object identifier.
    pub(crate) own_reference_id: u32,
    /// Stored reference type.
    pub(crate) reference_type: u32,
    /// Byte offset of the `f7 0x71` entry header.
    pub(crate) offset: usize,
}

#[cfg(test)]
impl FeatureReferenceName {
    /// Stored name decoded with replacement for invalid UTF-8 sequences.
    pub(crate) fn name(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.name_bytes)
    }
}

/// Decode structurally closed feature-name entries from model reference data.
pub(crate) fn reference_names(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<FeatureReferenceName>, CodecError> {
    let mut names = Vec::new();
    let Some(last) = payload.len().checked_sub(2) else {
        return Ok(names);
    };
    for offset in ctx.admit_iter(0..last, "creo reference name scan")? {
        if payload.get(offset..offset + 2) != Some(&[psb::token::ENTITY_REF, 0x71]) {
            continue;
        }
        let (own_reference_id, after_reference) = psb::compact_int(payload, offset + 2);
        let (reference_type, after_type) = psb::compact_int(payload, after_reference);
        let (feature_id, name_start) = psb::compact_int(payload, after_type);
        if after_reference == offset + 2
            || after_type == after_reference
            || name_start == after_type
            || feature_id == 0
        {
            continue;
        }
        let Some(name_end) = payload
            .get(name_start..)
            .and_then(|tail| tail.iter().take(256).position(|byte| *byte == 0))
            .map(|relative| name_start + relative)
        else {
            continue;
        };
        let name_bytes = &payload[name_start..name_end];
        if name_bytes.is_empty() || name_bytes.iter().any(u8::is_ascii_control) {
            continue;
        }
        let (first_close, after_first_close) = psb::compact_int(payload, name_end + 1);
        let (second_close, after_second_close) = psb::compact_int(payload, after_first_close);
        if after_first_close == name_end + 1
            || after_second_close == after_first_close
            || first_close != own_reference_id
            || second_close != own_reference_id
        {
            continue;
        }
        ctx.reserve_vec(&mut names, 1, "creo reference name entries")?;
        names.push(FeatureReferenceName {
            feature_id,
            name_bytes: ctx.copy_retained(name_bytes, "creo reference name bytes")?,
            own_reference_id,
            reference_type,
            offset,
        });
    }
    Ok(names)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FeatureRecipeBinding {
    recipe: FeatureRecipe,
    root_schema_class: SchemaClass,
    parent_feature_id: u32,
    offset: usize,
}

fn recipe_bindings(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<(u32, FeatureRecipeBinding)>, CodecError> {
    let mut bindings = Vec::new();
    for marker in ctx.admit_iter(0..payload.len(), "creo recipe binding scan")? {
        if payload.get(marker) != Some(&psb::token::ENTITY_REF) {
            continue;
        }
        let Ok((_, after_marker)) = psb::reference_id(payload, marker + 1) else {
            continue;
        };
        let (feature_id, after_feature) = psb::compact_int(payload, after_marker);
        let (schema_class, after_schema) = psb::compact_int(payload, after_feature);
        if after_feature == after_marker
            || after_schema == after_feature
            || !matches!(schema_class, 916 | 917)
            || payload.get(after_schema) != Some(&0xf6)
        {
            continue;
        }
        let (parent_feature_id, display_start) = psb::compact_int(payload, after_schema + 1);
        let Some(display_end) = payload
            .get(display_start..)
            .and_then(|bytes| bytes.iter().take(96).position(|byte| *byte == 0))
            .map(|relative| display_start + relative)
        else {
            continue;
        };
        let recipe_start = display_end + 3;
        if display_end == display_start
            || payload.get(display_end + 1..recipe_start) != Some(&[0xf6, 0x00])
        {
            continue;
        }
        if let Some((_, recipe)) = FEATURE_RECIPES
            .iter()
            .find(|(name, _)| payload.get(recipe_start..recipe_start + name.len()) == Some(*name))
        {
            ctx.reserve_vec(&mut bindings, 1, "creo recipe bindings")?;
            bindings.push((
                feature_id,
                FeatureRecipeBinding {
                    recipe: *recipe,
                    root_schema_class: SchemaClass::from(schema_class),
                    parent_feature_id,
                    offset: marker,
                },
            ));
        }
    }
    Ok(bindings)
}

fn inline_recipe_resolution(ctx: &DecodeContext<'_>, record: &[u8]) -> Result<RecipeState, CodecError> {
    let mut found = None;
    for (name, recipe) in FEATURE_RECIPES {
        let mut windows = record.windows(name.len());
        while let Some(window) = ctx.next_charged(&mut windows, "creo inline recipe scan")? {
            if window != *name { continue; }
            if found.is_some() { return Ok(RecipeState::Conflicting { candidate: None }); }
            found = Some(*recipe);
        }
    }
    Ok(found.into())
}

struct RecipeSummary {
    first: FeatureRecipeBinding,
    agreed: bool,
    count: usize,
}

#[derive(Clone, Copy)]
enum ParsedKind<'a> {
    Stored(&'a str),
    Recipe(FeatureRecipe),
    Native,
}

#[derive(Clone, Copy)]
struct ParsedName<'a> {
    bytes: &'a [u8],
    keyword: IdKeyword,
    prefix: Option<u8>,
}

struct ParsedOperation<'a, R: RecipeForm = RecipeState> {
    feature_id: u32,
    kind: ParsedKind<'a>,
    name: Option<ParsedName<'a>>,
    recipe: R,
    display_state_conflict: bool,
    depdb: Option<DepdbPrefix>,
    offset: usize,
    state_offset: usize,
}

impl<'a> ParsedOperation<'a> {
    fn project(self) -> ParsedOperation<'a, RecipeResolution> {
        ParsedOperation {
            feature_id: self.feature_id, kind: self.kind, name: self.name,
            recipe: self.recipe.into(), display_state_conflict: self.display_state_conflict,
            depdb: self.depdb, offset: self.offset, state_offset: self.state_offset,
        }
    }
}

impl<R: RecipeForm> ParsedOperation<'_, R> {
    fn display_name_stored(&self) -> bool { self.name.is_some() }
    fn root_schema_class(&self) -> Option<SchemaClass> { self.depdb.map(|prefix| prefix.schema) }
    fn parent_feature_id(&self) -> Option<u32> { self.depdb.map(|prefix| prefix.parent) }

    fn materialize(self, ctx: &DecodeContext<'_>) -> Result<FeatureOperation<R>, CodecError> {
        let kind = match self.kind {
            ParsedKind::Stored(text) => OperationKind::Stored(ctx.copy_retained_text(text, "creo operation family name")?),
            ParsedKind::Recipe(recipe) => OperationKind::from_recipe(recipe),
            ParsedKind::Native => OperationKind::Native,
        };
        let name = match self.name {
            Some(name) => OperationName::Stored {
                bytes: ctx.copy_retained(name.bytes, "creo operation stored name bytes")?,
                keyword: name.keyword, prefix: name.prefix,
            },
            None => OperationName::Derived,
        };
        Ok(FeatureOperation {
            feature_id: self.feature_id, kind, name, recipe: self.recipe,
            display_state_conflict: self.display_state_conflict, depdb: self.depdb,
            offset: self.offset, state_offset: self.state_offset,
        })
    }
}

/// Decode source states and materialize each stored name in source order.
pub(crate) fn operation_states(ctx: &DecodeContext<'_>, payload: &[u8]) -> Result<Vec<FeatureOperationState>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "creo parsed operation state storage")?;
    let states = storage.with_storage(|| parse_operation_states(ctx, payload))?;
    let mut result = Vec::new();
    for state in ctx.admit_iter(states, "creo operation state materialization")? {
        let state = state.materialize(ctx)?;
        ctx.push_vec(&mut result, state, "creo materialized operation states")?;
    }
    Ok(result)
}

/// Decode every NUL-terminated `<Kind> id <N>` operation state and bounded
/// procedural-recipe record from one feature-state namespace, in byte order.
fn parse_operation_states<'a>(
    ctx: &DecodeContext<'_>,
    payload: &'a [u8],
) -> Result<Vec<ParsedOperation<'a>>, CodecError> {
    const SEPARATORS: &[&[u8]] = &[b" id ", b" ID "];
    let family_byte = |byte: u8| {
        byte.is_ascii_alphanumeric()
            || byte >= 0x80
            || matches!(byte, b' ' | b'_' | b'-' | b'/' | b'(' | b')')
    };
    let mut binding_storage = ctx.reserve_scoped(0, "creo recipe binding storage")?;
    let bound_recipes = binding_storage.with_storage(|| recipe_bindings(ctx, payload))?;
    let mut summaries = HashMap::<u32, RecipeSummary>::new();
    for &(feature_id, binding) in ctx.admit_iter(&bound_recipes, "creo recipe binding summaries")? {
        let summary = binding_storage.with_storage(|| ctx.entry_hash_map(
            &mut summaries, feature_id, "creo recipe binding counts"))?
            .or_insert(RecipeSummary { first: binding, agreed: true, count: 0 });
        summary.count += 1;
        summary.agreed &= binding.recipe == summary.first.recipe
            && binding.root_schema_class == summary.first.root_schema_class
            && binding.parent_feature_id == summary.first.parent_feature_id;
    }
    let mut display_counts = HashMap::<u32, usize>::new();
    let mut displayed_bindings = HashMap::<u32, ()>::new();
    let mut result = Vec::new();
    let Some(last) = payload.len().checked_sub(4) else {
        return Ok(result);
    };
    for separator in ctx.admit_iter(0..last, "creo operation display scan")? {
        let Some(separator_bytes) = SEPARATORS.iter().find(|candidate| {
            payload.get(separator..separator + candidate.len()) == Some(**candidate)
        }) else {
            continue;
        };
        let mut offset = ctx.rposition_by(&payload[..separator],
            |byte| Ok(!family_byte(*byte)), "creo operation family start")?
            .map_or(0, |position| position + 1);
        let stored_family = loop {
            match ctx.validate_utf8(&payload[offset..separator], "creo UTF-8 validation")? {
                Ok(text) => break text,
                Err(_) => offset += 1,
            }
        };
        let state_offset = offset;
        let family = stored_family;
        if family.is_empty() || family.starts_with(' ') || family.ends_with(' ') {
            continue;
        }
        let (stored_name_prefix, family) = match family.as_bytes() {
            [prefix @ (b'o' | b'x' | b'y' | b'z'), first, ..] if first.is_ascii_uppercase() => {
                offset += 1;
                (Some(*prefix), &family[1..])
            }
            _ => (None, family),
        };
        let digits = &payload[separator + separator_bytes.len()..];
        let Some(end) = ctx.position_by(digits, |byte| Ok(*byte == 0), "creo operation identity end")? else {
            continue;
        };
        if end == 0 || !ctx.all_by(&digits[..end], |byte| Ok(byte.is_ascii_digit()), "creo operation identity digits")? {
            continue;
        }
        let Ok(digits) = ctx.validate_utf8(&digits[..end], "creo UTF-8 validation")? else {
            continue;
        };
        let Ok(feature_id) = ctx.parse_text::<u32>(digits, "creo scalar text parsing")? else {
            continue;
        };
        let record_start = ctx.rposition_by(&payload[..offset], |byte| Ok(*byte == 0xe3),
            "creo operation record start")?.map_or(0, |position| position + 1);
        let record = &payload[record_start..offset];
        let summary = summaries.get(&feature_id);
        let bound_recipe = summary.filter(|summary| summary.agreed).map(|summary| summary.first);
        let recipe = if summary.is_none() {
            inline_recipe_resolution(ctx, record)?
        } else {
            bound_recipe.map_or(RecipeState::Conflicting { candidate: None }, |binding| {
                RecipeState::Resolved(binding.recipe)
            })
        };
        if bound_recipe.is_some() {
            binding_storage.with_storage(|| ctx.insert_hash_map(
                &mut displayed_bindings, feature_id, (), "creo displayed recipe bindings"))?;
        }
        *binding_storage.with_storage(|| ctx.entry_hash_map(
            &mut display_counts, feature_id, "creo operation display counts"))?.or_insert(0) += 1;
        ctx.reserve_vec(&mut result, 1, "creo feature operation states")?;
        result.push(ParsedOperation {
            feature_id,
            kind: ParsedKind::Stored(family),
            name: Some(ParsedName {
                bytes: &payload[state_offset..separator + separator_bytes.len() + end],
                keyword: IdKeyword::from_bytes(&separator_bytes[1..separator_bytes.len() - 1])
                    .unwrap_or(IdKeyword::Id),
                prefix: stored_name_prefix,
            }),
            recipe,
            display_state_conflict: false,
            depdb: bound_recipe.map(|binding| DepdbPrefix {
                schema: binding.root_schema_class,
                parent: binding.parent_feature_id,
            }),
            offset,
            state_offset,
        });
    }
    for &(feature_id, binding) in ctx.admit_iter(&bound_recipes, "creo bound recipe states")? {
        if summaries.get(&feature_id).is_some_and(|summary| summary.count == 1)
            && displayed_bindings.contains_key(&feature_id) { continue; }
        ctx.reserve_vec(&mut result, 1, "creo feature operation states")?;
        result.push(ParsedOperation {
            feature_id,
            kind: ParsedKind::Recipe(binding.recipe),
            name: None,
            recipe: if summaries.get(&feature_id).is_some_and(|summary| !summary.agreed) {
                RecipeState::Conflicting {
                    candidate: Some(binding.recipe),
                }
            } else {
                RecipeState::Resolved(binding.recipe)
            },
            display_state_conflict: false,
            depdb: Some(DepdbPrefix {
                schema: binding.root_schema_class,
                parent: binding.parent_feature_id,
            }),
            offset: binding.offset,
            state_offset: binding.offset,
        });
    }
    ctx.stable_sort_by(
        result.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo operation states result ordering",
    )?;
    for operation in ctx.admit_iter(&mut result, "creo operation display conflicts")? {
        operation.display_state_conflict = display_counts.get(&operation.feature_id).is_some_and(|count| *count > 1);
    }
    Ok(result)
}

/// Decode one unambiguous or consensus operation projection per feature identifier.
pub(crate) fn operations(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<FeatureOperation>, CodecError> {
    let mut grouping_storage = ctx.reserve_scoped(0, "creo operation grouping storage")?;
    let mut by_feature = BTreeMap::<u32, Vec<ParsedOperation<'_>>>::new();
    let parsed = grouping_storage.with_storage(|| parse_operation_states(ctx, payload))?;
    for operation in ctx.admit_iter(parsed, "creo operation grouping")? {
        match grouping_storage.with_storage(|| ctx.entry_btree_map(
            &mut by_feature, operation.feature_id, "creo operation feature nodes"))? {
            std::collections::btree_map::Entry::Vacant(entry) => {
                let mut states = Vec::new();
                grouping_storage.with_storage(|| ctx.reserve_vec(&mut states, 1, "creo operation feature states"))?;
                states.push(operation);
                entry.insert(states);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let states = entry.get_mut();
                grouping_storage.with_storage(|| ctx.reserve_vec(states, 1, "creo operation feature states"))?;
                states.push(operation);
            }
        }
    }
    let mut current = Vec::new();
    for (_, mut states) in ctx.admit_iter(by_feature, "creo operation projection groups")? {
        let mut first_display: Option<usize> = None;
        let mut last_display = None;
        let mut same_kind = true;
        let mut agreed_recipe = None;
        let mut recipe = None;
        let mut schema = None;
        let mut parent = None;
        let mut binding_conflict = false;
        for (index, state) in ctx.admit_iter(&states, "creo operation state consensus")?.enumerate() {
            binding_conflict |= !state.display_name_stored()
                && matches!(state.recipe, RecipeState::Conflicting { candidate: Some(_) });
            if !state.display_name_stored() { continue; }
            if let Some(first) = first_display {
                same_kind = same_kind && match (state.kind, states[first].kind) {
                    (ParsedKind::Stored(a), ParsedKind::Stored(b)) => ctx.equal(a, b, "creo feature operation kind comparison")?,
                    _ => false,
                };
                if agreed_recipe != Some(state.recipe.resolved()) { agreed_recipe = None; }
                if recipe != Some(RecipeResolution::from(state.recipe)) { recipe = None; }
                if schema != Some(state.root_schema_class()) { schema = None; }
                if parent != Some(state.parent_feature_id()) { parent = None; }
            } else {
                first_display = Some(index);
                agreed_recipe = Some(state.recipe.resolved());
                recipe = Some(RecipeResolution::from(state.recipe));
                schema = Some(state.root_schema_class());
                parent = Some(state.parent_feature_id());
            }
            last_display = Some(index);
        }
        let projection = match (first_display, last_display) {
            (None, None) => if states.is_empty() { None } else { Some(states.swap_remove(0).project()) },
            (Some(first), Some(last)) => {
                let first_offset = states[first].offset;
                let first_state_offset = states[first].state_offset;
                let mut projection = states.swap_remove(last).project();
                if first != last {
                    projection.offset = first_offset;
                    projection.state_offset = first_state_offset;
                    projection.display_state_conflict = true;
                    if !same_kind {
                        projection.kind = agreed_recipe.flatten()
                            .map_or(ParsedKind::Native, ParsedKind::Recipe);
                    }
                    projection.name = None;
                    projection.recipe = recipe.unwrap_or(RecipeResolution::Conflicting);
                    projection.depdb = match (schema.flatten(), parent.flatten()) {
                        (Some(schema), Some(parent)) => Some(DepdbPrefix { schema, parent }),
                        _ => None,
                    };
                }
                Some(projection)
            }
            _ => None,
        };
        if let Some(mut projection) = projection {
            if binding_conflict {
                projection.recipe = RecipeResolution::Conflicting;
                projection.depdb = None;
                if !projection.display_name_stored() { projection.kind = ParsedKind::Native; }
            }
            ctx.reserve_vec(&mut current, 1, "creo current operation projections")?;
            current.push(projection.materialize(ctx)?);
        }
    }
    ctx.stable_sort_by(
        current.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo operations current ordering",
    )?;
    Ok(current)
}

#[cfg(test)]
mod tests {
    mod decode_cost;
    mod resource_limits;

    use super::reference_names;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::borrow::Cow;

    #[test]
    fn reference_name_entry_refuses_before_vec_growth() {
        let data = b"\xf7\x71\x01\x05\x02Name\0\x01\x01";
        let run = |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(data, &arena, &policy)
                .expect("root input is admitted");
            reference_names(&ctx, data)
        };
        assert_eq!(run(u64::MAX).expect("one entry admitted").len(), 1);
        let error = run(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some("creo reference name entries"), run)).expect_err("entry needs one Vec item");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo reference name entries"));
    }

    #[test]
    fn reference_name_bytes_refuse_before_retained_copy() {
        let data = b"\xf7\x71\x01\x05\x02Name\0\x01\x01";
        let run = |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(data, &arena, &policy)
                .expect("root input is admitted");
            reference_names(&ctx, data)
        };
        assert_eq!(run(u64::MAX).expect("four name bytes admitted").len(), 1);
        let error = run(crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, Some("creo reference name bytes"), run)).expect_err("fourth retained byte exceeds limit");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "creo reference name bytes"));
    }

    #[test]
    fn reference_name_text_follows_stored_bytes() {
        let data = b"\xf7\x71\x01\x05\x02N\xff\0\x01\x01";
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
            .expect("root input is admitted");
        let mut names = reference_names(&ctx, data).expect("one reference name");
        let [record] = names.as_mut_slice() else {
            panic!("one closed reference name");
        };
        assert_eq!(record.name_bytes, b"N\xff");
        assert_eq!(record.name(), "N\u{fffd}");
        assert!(matches!(record.name(), Cow::Owned(_)));
        record.name_bytes = b"Renamed".to_vec();
        assert_eq!(record.name(), Cow::Borrowed("Renamed"));
    }
}

#[cfg(test)]
mod recipe_tests;
