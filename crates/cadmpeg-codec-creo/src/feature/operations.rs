// SPDX-License-Identifier: Apache-2.0
//! Feature-state recipes, operation names, and model reference names.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

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

impl FeatureOperationState {
    /// Project one selected source state onto the current-state operation.
    fn project(self) -> FeatureOperation {
        FeatureOperation {
            feature_id: self.feature_id,
            kind: self.kind,
            name: self.name,
            recipe: self.recipe.into(),
            display_state_conflict: self.display_state_conflict,
            depdb: self.depdb,
            offset: self.offset,
            state_offset: self.state_offset,
        }
    }
}

impl<R: RecipeForm> FeatureOperation<R> {
    pub(crate) fn display_name_stored(&self) -> bool {
        self.name.display_name_stored()
    }

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
    for offset in 0..payload.len().saturating_sub(2) {
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
            .get(name_start..name_start.saturating_add(256).min(payload.len()))
            .and_then(|tail| tail.iter().position(|byte| *byte == 0))
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
        ctx.try_reserve_items(&mut names, 1, "creo reference name entries")?;
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
    for marker in 0..payload.len() {
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
            .get(display_start..display_start.saturating_add(96).min(payload.len()))
            .and_then(|bytes| bytes.iter().position(|byte| *byte == 0))
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
            ctx.try_reserve_items(&mut bindings, 1, "creo recipe bindings")?;
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

fn agreeing_recipe_binding(bindings: &[FeatureRecipeBinding]) -> Option<FeatureRecipeBinding> {
    let first = *bindings.first()?;
    bindings
        .iter()
        .all(|binding| {
            binding.recipe == first.recipe
                && binding.root_schema_class == first.root_schema_class
                && binding.parent_feature_id == first.parent_feature_id
        })
        .then_some(first)
}

fn inline_recipe_resolution(record: &[u8]) -> RecipeState {
    let mut found = None;
    for (name, recipe) in FEATURE_RECIPES {
        for _ in record.windows(name.len()).filter(|window| *window == *name) {
            if found.is_some() {
                return RecipeState::Conflicting { candidate: None };
            }
            found = Some(*recipe);
        }
    }
    found.into()
}

fn conflicting_recipe_features(
    ctx: &DecodeContext<'_>,
    bindings: &[(u32, FeatureRecipeBinding)],
) -> Result<BTreeSet<u32>, CodecError> {
    let mut by_feature = BTreeMap::<u32, Vec<FeatureRecipeBinding>>::new();
    for (feature_id, binding) in bindings {
        match by_feature.entry(*feature_id) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "creo recipe feature nodes")?;
                let mut values = Vec::new();
                ctx.try_reserve_items(&mut values, 1, "creo recipe feature bindings")?;
                values.push(*binding);
                entry.insert(values);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let values = entry.get_mut();
                ctx.try_reserve_items(values, 1, "creo recipe feature bindings")?;
                values.push(*binding);
            }
        }
    }
    let mut conflicting = BTreeSet::new();
    for (feature_id, bindings) in by_feature {
        if agreeing_recipe_binding(&bindings).is_none() {
            ctx.charge_collection_items(1, "creo conflicting recipe features")?;
            conflicting.insert(feature_id);
        }
    }
    Ok(conflicting)
}

fn agreeing_value<T: Clone + Eq>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let first = values.next()?;
    values.all(|value| value == first).then_some(first)
}

/// Decode every NUL-terminated `<Kind> id <N>` operation state and bounded
/// procedural-recipe record from one feature-state namespace, in byte order.
pub(crate) fn operation_states(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<FeatureOperationState>, CodecError> {
    const SEPARATORS: &[&[u8]] = &[b" id ", b" ID "];
    let family_byte = |byte: u8| {
        byte.is_ascii_alphanumeric()
            || byte >= 0x80
            || matches!(byte, b' ' | b'_' | b'-' | b'/' | b'(' | b')')
    };
    let bound_recipes = recipe_bindings(ctx, payload)?;
    let conflicting_features = conflicting_recipe_features(ctx, &bound_recipes)?;
    let mut recipe_binding_counts = BTreeMap::<u32, usize>::new();
    for (feature_id, _) in &bound_recipes {
        match recipe_binding_counts.entry(*feature_id) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "creo recipe binding counts")?;
                entry.insert(1);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => *entry.get_mut() += 1,
        }
    }
    let mut result = Vec::new();
    for separator in 0..payload.len().saturating_sub(4) {
        let Some(separator_bytes) = SEPARATORS.iter().find(|candidate| {
            payload.get(separator..separator + candidate.len()) == Some(**candidate)
        }) else {
            continue;
        };
        let mut offset = separator;
        while offset > 0 && family_byte(payload[offset - 1]) {
            offset -= 1;
        }
        while offset < separator && std::str::from_utf8(&payload[offset..separator]).is_err() {
            offset += 1;
        }
        let state_offset = offset;
        let stored_family = &payload[offset..separator];
        let family = stored_family;
        if family.is_empty() || family.first() == Some(&b' ') || family.last() == Some(&b' ') {
            continue;
        }
        let (stored_name_prefix, family) = match family {
            [prefix @ (b'o' | b'x' | b'y' | b'z'), first, ..] if first.is_ascii_uppercase() => {
                offset += 1;
                (Some(*prefix), &family[1..])
            }
            _ => (None, family),
        };
        let digits = &payload[separator + separator_bytes.len()..];
        let Some(end) = digits.iter().position(|byte| *byte == 0) else {
            continue;
        };
        if end == 0 || !digits[..end].iter().all(u8::is_ascii_digit) {
            continue;
        }
        let Some(feature_id) = std::str::from_utf8(&digits[..end])
            .ok()
            .and_then(|digits| digits.parse::<u32>().ok())
        else {
            continue;
        };
        let record_start = payload[..offset]
            .iter()
            .rposition(|byte| *byte == 0xe3)
            .map_or(0, |position| position + 1);
        let record = &payload[record_start..offset];
        let mut matching_recipes = bound_recipes
            .iter()
            .filter(|(candidate, _)| *candidate == feature_id)
            .map(|(_, binding)| binding);
        let first_binding = matching_recipes.next().copied();
        let bound_recipe = first_binding.filter(|first| {
            matching_recipes.all(|binding| {
                binding.recipe == first.recipe
                    && binding.root_schema_class == first.root_schema_class
                    && binding.parent_feature_id == first.parent_feature_id
            })
        });
        let recipe = if first_binding.is_none() {
            inline_recipe_resolution(record)
        } else {
            bound_recipe.map_or(RecipeState::Conflicting { candidate: None }, |binding| {
                RecipeState::Resolved(binding.recipe)
            })
        };
        let kind = crate::text::copy_lossy_text(ctx, family, "creo operation family name")?;
        let name_bytes = ctx.copy_retained(
            &payload[state_offset..separator + separator_bytes.len() + end],
            "creo operation stored name bytes",
        )?;
        ctx.try_reserve_items(&mut result, 1, "creo feature operation states")?;
        result.push(FeatureOperation {
            feature_id,
            kind: OperationKind::Stored(kind),
            name: OperationName::Stored {
                bytes: name_bytes,
                keyword: IdKeyword::from_bytes(&separator_bytes[1..separator_bytes.len() - 1])
                    .unwrap_or(IdKeyword::Id),
                prefix: stored_name_prefix,
            },
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
    for (feature_id, binding) in bound_recipes {
        if recipe_binding_counts.get(&feature_id) == Some(&1)
            && result.iter().any(|operation| {
                operation.feature_id == feature_id
                    && operation.recipe == RecipeState::Resolved(binding.recipe)
                    && operation.root_schema_class() == Some(binding.root_schema_class)
                    && operation.parent_feature_id() == Some(binding.parent_feature_id)
            })
        {
            continue;
        }
        ctx.try_reserve_items(&mut result, 1, "creo feature operation states")?;
        result.push(FeatureOperation {
            feature_id,
            kind: OperationKind::from_recipe(binding.recipe),
            name: OperationName::Derived,
            recipe: if conflicting_features.contains(&feature_id) {
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
    result.sort_by_key(|operation| operation.offset);
    let mut display_counts = BTreeMap::<u32, usize>::new();
    for operation in result
        .iter()
        .filter(|operation| operation.display_name_stored())
    {
        match display_counts.entry(operation.feature_id) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "creo operation display counts")?;
                entry.insert(1);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => *entry.get_mut() += 1,
        }
    }
    let mut conflicting_display_features = BTreeSet::new();
    for (feature_id, count) in display_counts {
        if count > 1 {
            ctx.charge_collection_items(1, "creo conflicting operation displays")?;
            conflicting_display_features.insert(feature_id);
        }
    }
    for operation in &mut result {
        operation.display_state_conflict =
            conflicting_display_features.contains(&operation.feature_id);
    }
    Ok(result)
}

/// Decode one unambiguous or consensus operation projection per feature identifier.
pub(crate) fn operations(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<FeatureOperation>, CodecError> {
    let bindings = recipe_bindings(ctx, payload)?;
    let conflicting_features = conflicting_recipe_features(ctx, &bindings)?;
    let mut by_feature = BTreeMap::<u32, Vec<FeatureOperationState>>::new();
    for operation in operation_states(ctx, payload)? {
        match by_feature.entry(operation.feature_id) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "creo operation feature nodes")?;
                let mut states = Vec::new();
                ctx.try_reserve_items(&mut states, 1, "creo operation feature states")?;
                states.push(operation);
                entry.insert(states);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let states = entry.get_mut();
                ctx.try_reserve_items(states, 1, "creo operation feature states")?;
                states.push(operation);
            }
        }
    }
    let mut current = Vec::new();
    for states in by_feature.into_values() {
        let display_count = states
            .iter()
            .filter(|state| state.display_name_stored())
            .count();
        let projection = match display_count {
            0 => states
                .into_iter()
                .next()
                .map(FeatureOperationState::project),
            1 => states
                .into_iter()
                .find(FeatureOperationState::display_name_stored)
                .map(FeatureOperationState::project),
            _ => {
                let first = states.iter().find(|state| state.display_name_stored());
                let last_index = states
                    .iter()
                    .rposition(FeatureOperationState::display_name_stored);
                if let (Some(first), Some(last_index)) = (first, last_index) {
                    let first_offset = first.offset;
                    let first_state_offset = first.state_offset;
                    let same_kind = states
                        .iter()
                        .filter(|state| state.display_name_stored())
                        .all(|state| state.kind == first.kind);
                    let agreed_recipe = agreeing_value(
                        states
                            .iter()
                            .filter(|state| state.display_name_stored())
                            .map(|state| state.recipe.resolved()),
                    )
                    .flatten();
                    let recipe = agreeing_value(
                        states
                            .iter()
                            .filter(|state| state.display_name_stored())
                            .map(|state| RecipeResolution::from(state.recipe)),
                    )
                    .unwrap_or(RecipeResolution::Conflicting);
                    let schema = agreeing_value(
                        states
                            .iter()
                            .filter(|state| state.display_name_stored())
                            .map(FeatureOperationState::root_schema_class),
                    )
                    .flatten();
                    let parent = agreeing_value(
                        states
                            .iter()
                            .filter(|state| state.display_name_stored())
                            .map(FeatureOperationState::parent_feature_id),
                    )
                    .flatten();
                    states.into_iter().nth(last_index).map(|state| {
                        let mut projection = state.project();
                        projection.offset = first_offset;
                        projection.state_offset = first_state_offset;
                        projection.display_state_conflict = true;
                        if !same_kind {
                            projection.kind = agreed_recipe
                                .map(OperationKind::from_recipe)
                                .unwrap_or(OperationKind::Native);
                        }
                        projection.name = OperationName::Derived;
                        projection.recipe = recipe;
                        projection.depdb = match (schema, parent) {
                            (Some(schema), Some(parent)) => Some(DepdbPrefix { schema, parent }),
                            _ => None,
                        };
                        projection
                    })
                } else {
                    None
                }
            }
        };
        if let Some(projection) = projection {
            ctx.try_reserve_items(&mut current, 1, "creo current operation projections")?;
            current.push(projection);
        }
    }
    for operation in &mut current {
        if !conflicting_features.contains(&operation.feature_id) {
            continue;
        }
        operation.recipe = RecipeResolution::Conflicting;
        operation.depdb = None;
        if !operation.display_name_stored() {
            operation.kind = OperationKind::Native;
        }
    }
    current.sort_by_key(|operation| operation.offset);
    Ok(current)
}

#[cfg(test)]
mod tests {
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
        assert_eq!(run(1).expect("one entry admitted").len(), 1);
        let error = run(0).expect_err("entry needs one Vec item");
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
        assert_eq!(run(4).expect("four name bytes admitted").len(), 1);
        let error = run(3).expect_err("fourth retained byte exceeds limit");
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
