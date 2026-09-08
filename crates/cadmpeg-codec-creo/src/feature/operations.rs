// SPDX-License-Identifier: Apache-2.0
//! Feature-state recipes, operation names, and model reference names.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use super::schema::SchemaClass;
use crate::psb;

/// Exact procedural recipe stored in a feature-state record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureRecipe {
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
pub enum FeatureRecipeKind {
    /// Linear section sweep.
    Extrude,
    /// Rotational section sweep.
    Revolve,
}

/// Boolean effect selected by a procedural feature recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureRecipeEffect {
    /// Material-adding operation.
    Protrude,
    /// Material-removing operation.
    Cut,
}

impl FeatureRecipe {
    /// Exact stored recipe name without its NUL terminator.
    pub const fn name(self) -> &'static str {
        match self {
            Self::ProtrudeExtrude => "protextrude",
            Self::CutExtrude => "cutextrude",
            Self::ProtrudeRevolve => "protrevolve",
            Self::CutRevolve => "cutrevolve",
        }
    }

    /// Section-sweep geometry family.
    pub const fn kind(self) -> FeatureRecipeKind {
        match self {
            Self::ProtrudeExtrude | Self::CutExtrude => FeatureRecipeKind::Extrude,
            Self::ProtrudeRevolve | Self::CutRevolve => FeatureRecipeKind::Revolve,
        }
    }

    /// Boolean effect of the section sweep.
    pub const fn effect(self) -> FeatureRecipeEffect {
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
pub enum IdKeyword {
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

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::ID => "ID",
        }
    }
}

/// Source of a feature-operation display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationName {
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
    pub fn display_name_stored(&self) -> bool {
        matches!(self, Self::Stored { .. })
    }

    pub fn stored_name(&self) -> Option<String> {
        self.stored_name_bytes()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
    }

    pub fn stored_name_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Stored { bytes, .. } => Some(bytes),
            Self::Derived => None,
        }
    }

    pub fn identifier_keyword(&self) -> Option<&str> {
        match self {
            Self::Stored { keyword, .. } => Some(keyword.as_str()),
            Self::Derived => None,
        }
    }

    pub fn stored_name_prefix(&self) -> Option<u8> {
        match self {
            Self::Stored { prefix, .. } => *prefix,
            Self::Derived => None,
        }
    }
}

/// Operation-family kind named by a feature-state record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationKind {
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
    pub fn as_str(&self) -> &str {
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
pub struct DepdbPrefix {
    /// Root feature-definition schema class.
    pub schema: SchemaClass,
    /// Previous or parent feature identifier.
    pub parent: u32,
}

/// Resolution of a feature's procedural recipe in one stored source state.
///
/// A source state that competes with another may still name the recipe it
/// stored; that candidate is source evidence, not a resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipeState {
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
    pub fn resolved(self) -> Option<FeatureRecipe> {
        match self {
            Self::Resolved(recipe) => Some(recipe),
            Self::None | Self::Conflicting { .. } => None,
        }
    }

    /// Stored recipe candidate retained for source records.
    pub fn candidate(self) -> Option<FeatureRecipe> {
        match self {
            Self::Resolved(recipe) => Some(recipe),
            Self::Conflicting { candidate } => candidate,
            Self::None => None,
        }
    }

    /// Whether competing recipes prevent resolution.
    pub fn is_conflicting(self) -> bool {
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
pub enum RecipeResolution {
    /// No recipe is stored.
    None,
    /// One recipe is resolved.
    Resolved(FeatureRecipe),
    /// Competing recipes prevent resolution.
    Conflicting,
}

impl RecipeResolution {
    /// Resolved recipe available to geometry consumers.
    pub fn resolved(self) -> Option<FeatureRecipe> {
        match self {
            Self::Resolved(recipe) => Some(recipe),
            Self::None | Self::Conflicting => None,
        }
    }

    /// Whether competing recipes prevent resolution.
    pub fn is_conflicting(self) -> bool {
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
    pub trait Sealed {}

    impl Sealed for super::RecipeState {}
    impl Sealed for super::RecipeResolution {}
}

/// Stored or resolved procedural recipe form.
pub trait RecipeForm: sealed::Sealed {}

impl RecipeForm for RecipeState {}
impl RecipeForm for RecipeResolution {}

/// One stored feature-state record, before current-state selection.
pub type FeatureOperationState = FeatureOperation<RecipeState>;

/// Feature-operation family named by a feature-state record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureOperation<R: RecipeForm = RecipeResolution> {
    /// Numeric feature identifier following `id` in the stored name.
    pub feature_id: u32,
    /// Operation-family kind.
    pub kind: OperationKind,
    /// Display-name source.
    pub name: OperationName,
    /// Procedural recipe resolution for this state.
    pub recipe: R,
    /// Multiple stored display states prevent a unique current-state selection.
    pub display_state_conflict: bool,
    /// DEPDB recipe prefix, when present.
    pub depdb: Option<DepdbPrefix>,
    /// Byte offset of the operation name in the original stream.
    pub offset: usize,
    /// Byte offset including the optional stored-name prefix.
    pub state_offset: usize,
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
    pub fn display_name_stored(&self) -> bool {
        self.name.display_name_stored()
    }

    pub fn stored_name(&self) -> Option<String> {
        self.name.stored_name()
    }

    pub fn stored_name_prefix(&self) -> Option<u8> {
        self.name.stored_name_prefix()
    }

    pub fn root_schema_class(&self) -> Option<SchemaClass> {
        self.depdb.map(|prefix| prefix.schema)
    }

    pub fn parent_feature_id(&self) -> Option<u32> {
        self.depdb.map(|prefix| prefix.parent)
    }
}

/// Feature name joined to its model feature identifier by `mdl_feat_ref_info_new`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureReferenceName {
    /// Numeric model feature identifier.
    pub feature_id: u32,
    /// Exact stored feature-name bytes excluding the NUL terminator.
    pub name_bytes: Vec<u8>,
    /// Reference-database object identifier.
    pub own_reference_id: u32,
    /// Stored reference type.
    pub reference_type: u32,
    /// Byte offset of the `f7 0x71` entry header.
    pub offset: usize,
}

impl FeatureReferenceName {
    /// Stored name decoded with replacement for invalid UTF-8 sequences.
    pub fn name(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.name_bytes)
    }
}

/// Decode structurally closed feature-name entries from model reference data.
pub fn reference_names(payload: &[u8]) -> Vec<FeatureReferenceName> {
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
        names.push(FeatureReferenceName {
            feature_id,
            name_bytes: name_bytes.to_vec(),
            own_reference_id,
            reference_type,
            offset,
        });
    }
    names
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FeatureRecipeBinding {
    recipe: FeatureRecipe,
    root_schema_class: SchemaClass,
    parent_feature_id: u32,
    offset: usize,
}

fn recipe_bindings(payload: &[u8]) -> Vec<(u32, FeatureRecipeBinding)> {
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
    bindings
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

fn conflicting_recipe_features(bindings: &[(u32, FeatureRecipeBinding)]) -> BTreeSet<u32> {
    let mut by_feature = BTreeMap::<u32, Vec<FeatureRecipeBinding>>::new();
    for (feature_id, binding) in bindings {
        by_feature.entry(*feature_id).or_default().push(*binding);
    }
    by_feature
        .into_iter()
        .filter_map(|(feature_id, bindings)| {
            agreeing_recipe_binding(&bindings)
                .is_none()
                .then_some(feature_id)
        })
        .collect()
}

fn agreeing_value<T: Clone + Eq>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let first = values.next()?;
    values.all(|value| value == first).then_some(first)
}

/// Decode every NUL-terminated `<Kind> id <N>` operation state and bounded
/// procedural-recipe record from one feature-state namespace, in byte order.
pub fn operation_states(payload: &[u8]) -> Vec<FeatureOperationState> {
    const SEPARATORS: &[&[u8]] = &[b" id ", b" ID "];
    let family_byte = |byte: u8| {
        byte.is_ascii_alphanumeric()
            || byte >= 0x80
            || matches!(byte, b' ' | b'_' | b'-' | b'/' | b'(' | b')')
    };
    let bound_recipes = recipe_bindings(payload);
    let conflicting_features = conflicting_recipe_features(&bound_recipes);
    let recipe_binding_counts = bound_recipes.iter().fold(
        BTreeMap::<u32, usize>::new(),
        |mut counts, (feature_id, _)| {
            *counts.entry(*feature_id).or_default() += 1;
            counts
        },
    );
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
        let Ok(feature_id) = String::from_utf8_lossy(&digits[..end]).parse::<u32>() else {
            continue;
        };
        let record_start = payload[..offset]
            .iter()
            .rposition(|byte| *byte == 0xe3)
            .map_or(0, |position| position + 1);
        let record = &payload[record_start..offset];
        let matching_recipes = bound_recipes
            .iter()
            .filter(|(candidate, _)| *candidate == feature_id)
            .map(|(_, binding)| *binding)
            .collect::<Vec<_>>();
        let bound_recipe = agreeing_recipe_binding(&matching_recipes);
        let recipe = if matching_recipes.is_empty() {
            inline_recipe_resolution(record)
        } else {
            bound_recipe.map_or(RecipeState::Conflicting { candidate: None }, |binding| {
                RecipeState::Resolved(binding.recipe)
            })
        };
        result.push(FeatureOperation {
            feature_id,
            kind: OperationKind::Stored(String::from_utf8_lossy(family).into_owned()),
            name: OperationName::Stored {
                bytes: payload[state_offset..separator + separator_bytes.len() + end].to_vec(),
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
    let conflicting_display_features = result
        .iter()
        .filter(|operation| operation.display_name_stored())
        .fold(BTreeMap::<u32, usize>::new(), |mut counts, operation| {
            *counts.entry(operation.feature_id).or_default() += 1;
            counts
        })
        .into_iter()
        .filter_map(|(feature_id, count)| (count > 1).then_some(feature_id))
        .collect::<BTreeSet<_>>();
    for operation in &mut result {
        operation.display_state_conflict =
            conflicting_display_features.contains(&operation.feature_id);
    }
    result
}

/// Decode one unambiguous or consensus operation projection per feature identifier.
pub fn operations(payload: &[u8]) -> Vec<FeatureOperation> {
    let bindings = recipe_bindings(payload);
    let conflicting_features = conflicting_recipe_features(&bindings);
    let mut by_feature = BTreeMap::<u32, Vec<FeatureOperationState>>::new();
    for operation in operation_states(payload) {
        by_feature
            .entry(operation.feature_id)
            .or_default()
            .push(operation);
    }
    let mut current = by_feature
        .into_values()
        .filter_map(|states| {
            let display_states = states
                .iter()
                .filter(|state| state.display_name_stored())
                .collect::<Vec<_>>();
            match display_states.as_slice() {
                [] => states.first().cloned().map(FeatureOperationState::project),
                [display] => Some((*display).clone().project()),
                displays => {
                    let mut projection = (*displays.last()?).clone().project();
                    projection.offset = displays.first()?.offset;
                    projection.state_offset = displays.first()?.state_offset;
                    projection.display_state_conflict = true;
                    projection.kind =
                        agreeing_value(displays.iter().map(|state| state.kind.clone()))
                            .or_else(|| {
                                agreeing_value(displays.iter().map(|state| state.recipe.resolved()))
                                    .flatten()
                                    .map(OperationKind::from_recipe)
                            })
                            .unwrap_or(OperationKind::Native);
                    projection.name = OperationName::Derived;
                    projection.recipe = agreeing_value(
                        displays
                            .iter()
                            .map(|state| RecipeResolution::from(state.recipe)),
                    )
                    .unwrap_or(RecipeResolution::Conflicting);
                    projection.depdb = match (
                        agreeing_value(displays.iter().map(|state| state.root_schema_class()))
                            .flatten(),
                        agreeing_value(displays.iter().map(|state| state.parent_feature_id()))
                            .flatten(),
                    ) {
                        (Some(schema), Some(parent)) => Some(DepdbPrefix { schema, parent }),
                        _ => None,
                    };
                    Some(projection)
                }
            }
        })
        .collect::<Vec<_>>();
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
    current
}

#[cfg(test)]
mod tests {
    use super::reference_names;
    use std::borrow::Cow;

    #[test]
    fn reference_name_text_follows_stored_bytes() {
        let mut names = reference_names(b"\xf7\x71\x01\x05\x02N\xff\0\x01\x01");
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
