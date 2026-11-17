use crate::features::NonEmptyMembers;
use crate::ids::{NativeId, SubdId};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FormInputs {
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub parameters: BTreeMap<NonBlankString, String>,
    pub references: Vec<NativeId>,
    pub lists: Vec<FormNativeList>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FormNativeList {
    pub source: FormInputPath,
    pub role: FormListRole,
    pub declared_count: u32,
    pub members: Vec<FormNativeMember>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FormListRole {
    CageObjects,
    TrackingSelection,
    CageCatalog,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FormNativeMember {
    pub ordinal: u32,
    pub reference: NativeId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FormInputPath {
    pub references: NonEmptyMembers<NativeId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FormCageObservation {
    pub paths: Vec<FormInputPath>,
    pub binding: FormCageBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FormCageBinding {
    Decoded {
        cage: SubdId,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_resource"
        )]
        #[cfg_attr(feature = "schema", schemars(with = "FormResource"))]
        resource: Option<FormResource>,
    },
    Undecoded {
        resource: FormResource,
        availability: FormResourceAvailability,
    },
    Unresolved {},
}
cadmpeg_core::named_optional_field!(deserialize_resource, FormResource, "resource");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FormResource {
    pub source: NativeId,
    pub entry: NonBlankString,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FormResourceAvailability {
    ActiveUndecoded,
    Inactive,
    Unknown,
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct FormObservationIndex(pub usize);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FormMember {
    pub observation: FormObservationIndex,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_ordinal"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "u32"))]
    pub ordinal: Option<u32>,
}
cadmpeg_core::named_optional_field!(deserialize_ordinal, u32, "ordinal");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FormKnownOrder {
    Source,
    Assumed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FormOrder {
    Source,
    Assumed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FormMembership {
    Complete {
        members: Vec<FormObservationIndex>,
        order: FormKnownOrder,
        certainty: FormKnownOrder,
    },
    Partial {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cardinality"
        )]
        #[cfg_attr(feature = "schema", schemars(with = "u32"))]
        cardinality: Option<u32>,
        order: FormOrder,
        members: Vec<FormMember>,
    },
}
cadmpeg_core::named_optional_field!(deserialize_cardinality, u32, "cardinality");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormConstruction {
    inputs: FormInputs,
    observations: Vec<FormCageObservation>,
    membership: FormMembership,
}

impl Serialize for FormConstruction {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        FormWire::from(self.clone()).serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for FormConstruction {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(FormWire::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
#[cfg(feature = "schema")]
impl JsonSchema for FormConstruction {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "FormConstruction".into()
    }
    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut schema = FormWire::json_schema(generator);
        schema.insert("allOf".into(), serde_json::json!([{
            "if": {"required":["cages"], "properties":{"cages":{"properties":{"kind":{"const":"complete"}}}}},
            "then": {"properties":{"cage_inputs":{"properties":{"membership":{"properties":{"kind":{"const":"complete"}}}}}}},
            "else": {"not":{"required":["cage_inputs"]}, "properties":{"cages":{"properties":{"value":{"properties":{"membership":{"properties":{"kind":{"const":"partial"}}}}}}}}}
        }]));
        schema
    }
}

impl Default for FormConstruction {
    fn default() -> Self {
        Self {
            inputs: FormInputs::default(),
            observations: Vec::new(),
            membership: FormMembership::Partial {
                cardinality: None,
                order: FormOrder::Unknown,
                members: Vec::new(),
            },
        }
    }
}

impl FormConstruction {
    pub fn try_new(
        inputs: FormInputs,
        observations: Vec<FormCageObservation>,
        membership: FormMembership,
    ) -> Result<Self, &'static str> {
        for list in &inputs.lists {
            let mut ordinals = HashSet::new();
            if list.members.iter().any(|member| {
                member.ordinal >= list.declared_count || !ordinals.insert(member.ordinal)
            }) {
                return Err("invalid native Form list position");
            }
        }
        let mut cages = HashSet::new();
        if observations
            .iter()
            .any(|observation| match &observation.binding {
                FormCageBinding::Decoded { cage, .. } => !cages.insert(cage),
                _ => false,
            })
        {
            return Err("duplicate Form cage observation");
        }
        let mut selected = HashSet::new();
        let mut positions = HashSet::new();
        let mut admit = |index: FormObservationIndex,
                         ordinal: Option<u32>,
                         cardinality: Option<u32>,
                         complete: bool| {
            let Some(observation) = observations.get(index.0) else {
                return false;
            };
            selected.insert(index)
                && ordinal.is_none_or(|ordinal| {
                    cardinality.is_none_or(|count| ordinal < count) && positions.insert(ordinal)
                })
                && (!complete || matches!(observation.binding, FormCageBinding::Decoded { .. }))
                && !matches!(
                    observation.binding,
                    FormCageBinding::Undecoded {
                        availability: FormResourceAvailability::Inactive,
                        ..
                    }
                )
        };
        let valid = match &membership {
            FormMembership::Complete { members, .. } => {
                members.iter().all(|&index| admit(index, None, None, true))
            }
            FormMembership::Partial {
                cardinality,
                members,
                ..
            } => {
                cardinality.is_none_or(|count| {
                    members.len() <= cadmpeg_core::decode::index_from_u32(count)
                }) && members
                    .iter()
                    .all(|member| admit(member.observation, member.ordinal, *cardinality, false))
            }
        };
        if !valid {
            return Err("invalid Form membership");
        }
        Ok(Self {
            inputs,
            observations,
            membership,
        })
    }
    pub fn complete(cages: Vec<SubdId>) -> Result<Self, &'static str> {
        Self::from_cages(FormInputs::default(), cages)
    }
    fn from_cages(inputs: FormInputs, cages: Vec<SubdId>) -> Result<Self, &'static str> {
        let members = (0..cages.len()).map(FormObservationIndex).collect();
        Self::try_new(
            inputs,
            cages
                .into_iter()
                .map(|cage| FormCageObservation {
                    paths: Vec::new(),
                    binding: FormCageBinding::Decoded {
                        cage,
                        resource: None,
                    },
                })
                .collect(),
            FormMembership::Complete {
                members,
                order: FormKnownOrder::Source,
                certainty: FormKnownOrder::Source,
            },
        )
    }
    pub fn inputs(&self) -> &FormInputs {
        &self.inputs
    }
    pub fn observations(&self) -> &[FormCageObservation] {
        &self.observations
    }
    pub fn membership(&self) -> &FormMembership {
        &self.membership
    }
    pub fn membership_is_complete(&self) -> bool {
        matches!(self.membership, FormMembership::Complete { .. })
    }
    pub fn referenced_cages(&self) -> impl Iterator<Item = &SubdId> {
        self.observations
            .iter()
            .filter_map(|observation| match &observation.binding {
                FormCageBinding::Decoded { cage, .. } => Some(cage),
                _ => None,
            })
    }
    pub fn committed_cages(&self) -> impl Iterator<Item = &SubdId> {
        let count = match &self.membership {
            FormMembership::Complete { members, .. } => members.len(),
            FormMembership::Partial { members, .. } => members.len(),
        };
        (0..count).filter_map(|ordinal| {
            let index = match &self.membership {
                FormMembership::Complete { members, .. } => members[ordinal].0,
                FormMembership::Partial { members, .. } => members[ordinal].observation.0,
            };
            match &self.observations[index].binding {
                FormCageBinding::Decoded { cage, .. } => Some(cage),
                _ => None,
            }
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct FormEvidence {
    observations: Vec<FormCageObservation>,
    membership: FormMembership,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum FormCagesWire {
    Complete(Vec<SubdId>),
    Retained(FormEvidence),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct FormWire {
    cages: FormCagesWire,
    inputs: FormInputs,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cage_inputs"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "FormEvidence"))]
    cage_inputs: Option<FormEvidence>,
}
cadmpeg_core::named_optional_field!(deserialize_cage_inputs, FormEvidence, "cage_inputs");

impl TryFrom<FormWire> for FormConstruction {
    type Error = &'static str;
    fn try_from(wire: FormWire) -> Result<Self, Self::Error> {
        match wire.cages {
            FormCagesWire::Complete(cages) => {
                let Some(evidence) = wire.cage_inputs else {
                    return Self::from_cages(wire.inputs, cages);
                };
                let construction =
                    Self::try_new(wire.inputs, evidence.observations, evidence.membership)?;
                if !construction.membership_is_complete()
                    || !construction.committed_cages().eq(cages.iter())
                {
                    return Err("Form cages contradict cage_inputs");
                }
                Ok(construction)
            }
            FormCagesWire::Retained(evidence) => {
                if wire.cage_inputs.is_some()
                    || matches!(evidence.membership, FormMembership::Complete { .. })
                {
                    return Err("partial Form requires partial membership");
                }
                Self::try_new(wire.inputs, evidence.observations, evidence.membership)
            }
        }
    }
}

impl From<FormConstruction> for FormWire {
    fn from(value: FormConstruction) -> Self {
        let complete = value.membership_is_complete();
        let cages = if complete {
            value.committed_cages().cloned().collect()
        } else {
            Vec::new()
        };
        let minimal = matches!(
            value.membership,
            FormMembership::Complete {
                order: FormKnownOrder::Source,
                certainty: FormKnownOrder::Source,
                ..
            }
        ) && value.observations.iter().all(|observation| {
            observation.paths.is_empty()
                && matches!(
                    observation.binding,
                    FormCageBinding::Decoded { resource: None, .. }
                )
        }) && value.observations.len() == value.committed_cages().count()
            && matches!(&value.membership, FormMembership::Complete { members, .. } if members.iter().enumerate().all(|(index, member)| index == member.0));
        let evidence = FormEvidence {
            observations: value.observations,
            membership: value.membership,
        };
        let (cages, cage_inputs) = if complete {
            (
                FormCagesWire::Complete(cages),
                (!minimal).then_some(evidence),
            )
        } else {
            (FormCagesWire::Retained(evidence), None)
        };
        Self {
            inputs: value.inputs,
            cages,
            cage_inputs,
        }
    }
}

impl crate::schema::rewrite::typed::RewriteIdentities for FormConstruction {
    fn visit_identity_references(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let _depth = ctx.enter_nested("visit Form construction")?;
        self.inputs.visit_identity_references(ctx, visitor)?;
        self.observations.visit_identity_references(ctx, visitor)
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        map: &mut crate::schema::rewrite::typed::IdentityMap<'_, F>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let _depth = ctx.enter_nested("rewrite Form construction")?;
        Ok(Self {
            inputs: self.inputs.rewrite_identities(ctx, map)?,
            observations: self.observations.rewrite_identities(ctx, map)?,
            membership: self.membership,
        })
    }
}

rewrite_record!(FormNativeList, []; { source, role, declared_count, members });
rewrite_record!(FormNativeMember, []; { ordinal, reference });
rewrite_record!(FormInputPath, []; { references });
rewrite_record!(FormCageObservation, []; { paths, binding });

rewrite_record!(FormMember, []; { observation, ordinal });
rewrite_scalar!(FormListRole);
rewrite_scalar!(FormResourceAvailability);
rewrite_scalar!(FormObservationIndex);
rewrite_scalar!(FormOrder);
rewrite_scalar!(FormKnownOrder);
rewrite_enum!(FormCageBinding, []; { Decoded { cage, resource }, Undecoded { resource, availability }, Unresolved {} });
rewrite_enum!(FormMembership, []; { Complete { members, order, certainty }, Partial { cardinality, order, members } });

#[cfg(test)]
mod tests;

impl crate::schema::rewrite::typed::RewriteIdentities for FormInputs {
    fn visit_identity_references(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        self.references.visit_identity_references(ctx, visitor)?;
        self.lists.visit_identity_references(ctx, visitor)
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        map: &mut crate::schema::rewrite::typed::IdentityMap<'_, F>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self {
            parameters: self.parameters,
            references: self.references.rewrite_identities(ctx, map)?,
            lists: self.lists.rewrite_identities(ctx, map)?,
        })
    }
}
impl crate::schema::rewrite::typed::RewriteIdentities for FormResource {
    fn visit_identity_references(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        self.source.visit_identity_references(ctx, visitor)
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        map: &mut crate::schema::rewrite::typed::IdentityMap<'_, F>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self {
            source: self.source.rewrite_identities(ctx, map)?,
            entry: self.entry,
        })
    }
}
