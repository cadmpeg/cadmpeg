use crate::features::NonEmptyMembers;
use crate::ids::{NativeId, SubdId};
use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = include_str!("form-api-item.md")]
pub struct FormInputs {
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "cadmpeg_core::distinct_keys::btree_map"
    )]
    #[doc = include_str!("form-api-item.md")]
    pub parameters: BTreeMap<NonBlankString, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[doc = include_str!("form-api-item.md")]
    pub references: Vec<NativeId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[doc = include_str!("form-api-item.md")]
    pub lists: Vec<FormNativeList>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[doc = include_str!("form-api-item.md")]
    pub reference_evidence: Vec<FormNativeReference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[doc = include_str!("form-api-item.md")]
    pub position_evidence: Vec<FormPositionEvidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[doc = include_str!("form-api-item.md")]
    pub cardinality_evidence: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[doc = include_str!("form-api-item.md")]
    pub resource_evidence: Vec<FormResource>,
}

impl FormInputs {
    fn is_empty(&self) -> bool {
        self.parameters.is_empty()
            && self.references.is_empty()
            && self.lists.is_empty()
            && self.reference_evidence.is_empty()
            && self.position_evidence.is_empty()
            && self.cardinality_evidence.is_empty()
            && self.resource_evidence.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = include_str!("form-api-item.md")]
pub struct FormPositionEvidence {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_position_source"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "FormInputPath"))]
    #[doc = include_str!("form-api-item.md")]
    pub source: Option<FormInputPath>,
    #[doc = include_str!("form-api-item.md")]
    pub observation: FormObservationIndex,
    #[doc = include_str!("form-api-item.md")]
    pub ordinal: u32,
}
cadmpeg_core::named_optional_field!(deserialize_position_source, FormInputPath, "source");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = include_str!("form-api-item.md")]
pub struct FormNativeReference {
    #[doc = include_str!("form-api-item.md")]
    pub source: NativeId,
    #[doc = include_str!("form-api-item.md")]
    pub role: FormReferenceRole,
    #[doc = include_str!("form-api-item.md")]
    pub ordinal: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_reference_run"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "usize"))]
    #[doc = include_str!("form-api-item.md")]
    pub run: Option<usize>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_inline_type_guid"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    #[doc = include_str!("form-api-item.md")]
    pub inline_type_guid: Option<String>,
    #[doc = include_str!("form-api-item.md")]
    pub target: FormNativeReferenceTarget,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_resource"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "FormResource"))]
    #[doc = include_str!("form-api-item.md")]
    pub resource: Option<FormResource>,
}
cadmpeg_core::named_optional_field!(deserialize_inline_type_guid, String, "inline_type_guid");
cadmpeg_core::named_optional_field!(deserialize_reference_run, usize, "run");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[doc = include_str!("form-api-item.md")]
pub enum FormReferenceRole {
    #[doc = include_str!("form-api-item.md")]
    Body,
    #[doc = include_str!("form-api-item.md")]
    TrackingOwner,
    #[doc = include_str!("form-api-item.md")]
    TrackingMember,
    #[doc = include_str!("form-api-item.md")]
    SerializerSurface,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "namespace", rename_all = "snake_case", deny_unknown_fields)]
#[doc = include_str!("form-api-item.md")]
pub enum FormNativeReferenceTarget {
    #[doc = include_str!("form-api-item.md")]
    Local {
        #[doc = include_str!("form-api-item.md")]
        target: NativeId,
    },
    #[doc = include_str!("form-api-item.md")]
    Segment {
        #[doc = include_str!("form-api-item.md")]
        target: u64,
        #[doc = include_str!("form-api-item.md")]
        segment: u32,
    },
    #[doc = include_str!("form-api-item.md")]
    Document {
        #[doc = include_str!("form-api-item.md")]
        target: u64,
        #[doc = include_str!("form-api-item.md")]
        segment: u32,
        #[doc = include_str!("form-api-item.md")]
        asset_guid: String,
        #[doc = include_str!("form-api-item.md")]
        target_type_guid: String,
        #[doc = include_str!("form-api-item.md")]
        link_name: String,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_form_external_version"
        )]
        #[cfg_attr(feature = "schema", schemars(with = "FormExternalVersion"))]
        #[doc = include_str!("form-api-item.md")]
        version: Option<FormExternalVersion>,
    },
}
cadmpeg_core::named_optional_field!(
    deserialize_form_external_version,
    FormExternalVersion,
    "version"
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = include_str!("form-api-item.md")]
pub struct FormExternalVersion {
    #[doc = include_str!("form-api-item.md")]
    pub key: String,
    #[doc = include_str!("form-api-item.md")]
    pub urn: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = include_str!("form-api-item.md")]
pub struct FormNativeList {
    #[doc = include_str!("form-api-item.md")]
    pub source: FormInputPath,
    #[doc = include_str!("form-api-item.md")]
    pub role: FormListRole,
    #[doc = include_str!("form-api-item.md")]
    pub declared_count: u32,
    #[doc = include_str!("form-api-item.md")]
    pub members: Vec<FormNativeMember>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[doc = include_str!("form-api-item.md")]
pub enum FormListRole {
    #[doc = include_str!("form-api-item.md")]
    CageObjects,
    #[doc = include_str!("form-api-item.md")]
    TrackingSelection,
    #[doc = include_str!("form-api-item.md")]
    CageCatalog,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = include_str!("form-api-item.md")]
pub struct FormNativeMember {
    #[doc = include_str!("form-api-item.md")]
    pub ordinal: u32,
    #[doc = include_str!("form-api-item.md")]
    pub reference: NativeId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = include_str!("form-api-item.md")]
pub struct FormInputPath {
    #[doc = include_str!("form-api-item.md")]
    pub references: NonEmptyMembers<NativeId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = include_str!("form-api-item.md")]
pub struct FormCageObservation {
    #[doc = include_str!("form-api-item.md")]
    pub paths: Vec<FormInputPath>,
    #[doc = include_str!("form-api-item.md")]
    pub binding: FormCageBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
#[doc = include_str!("form-api-item.md")]
pub enum FormCageBinding {
    #[doc = include_str!("form-api-item.md")]
    Decoded {
        #[doc = include_str!("form-api-item.md")]
        cage: SubdId,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_resource"
        )]
        #[cfg_attr(feature = "schema", schemars(with = "FormResource"))]
        #[doc = include_str!("form-api-item.md")]
        resource: Option<FormResource>,
    },
    #[doc = include_str!("form-api-item.md")]
    Undecoded {
        #[doc = include_str!("form-api-item.md")]
        resource: FormResource,
        #[doc = include_str!("form-api-item.md")]
        availability: FormResourceAvailability,
    },
    #[doc = include_str!("form-api-item.md")]
    Unresolved {},
}
cadmpeg_core::named_optional_field!(deserialize_resource, FormResource, "resource");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = include_str!("form-api-item.md")]
pub struct FormResource {
    #[doc = include_str!("form-api-item.md")]
    pub source: NativeId,
    #[doc = include_str!("form-api-item.md")]
    pub entry: NonBlankString,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[doc = include_str!("form-api-item.md")]
pub enum FormResourceAvailability {
    #[doc = include_str!("form-api-item.md")]
    ActiveUndecoded,
    #[doc = include_str!("form-api-item.md")]
    Inactive,
    #[doc = include_str!("form-api-item.md")]
    Unknown,
    #[doc = include_str!("form-api-item.md")]
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
#[doc = include_str!("form-api-item.md")]
pub struct FormObservationIndex(#[doc = include_str!("form-api-item.md")] pub usize);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = include_str!("form-api-item.md")]
pub struct FormMember {
    #[doc = include_str!("form-api-item.md")]
    pub observation: FormObservationIndex,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_ordinal"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "u32"))]
    #[doc = include_str!("form-api-item.md")]
    pub ordinal: Option<u32>,
}
cadmpeg_core::named_optional_field!(deserialize_ordinal, u32, "ordinal");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[doc = include_str!("form-api-item.md")]
pub enum FormKnownOrder {
    #[doc = include_str!("form-api-item.md")]
    Source,
    #[doc = include_str!("form-api-item.md")]
    Assumed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[doc = include_str!("form-api-item.md")]
pub enum FormOrder {
    #[doc = include_str!("form-api-item.md")]
    Source,
    #[doc = include_str!("form-api-item.md")]
    Assumed,
    #[doc = include_str!("form-api-item.md")]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
#[doc = include_str!("form-api-item.md")]
pub enum FormMembership {
    #[doc = include_str!("form-api-item.md")]
    Complete {
        #[doc = include_str!("form-api-item.md")]
        members: Vec<FormObservationIndex>,
        #[doc = include_str!("form-api-item.md")]
        order: FormKnownOrder,
        #[doc = include_str!("form-api-item.md")]
        certainty: FormKnownOrder,
    },
    #[doc = include_str!("form-api-item.md")]
    Partial {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cardinality"
        )]
        #[cfg_attr(feature = "schema", schemars(with = "u32"))]
        #[doc = include_str!("form-api-item.md")]
        cardinality: Option<u32>,
        #[doc = include_str!("form-api-item.md")]
        order: FormOrder,
        #[doc = include_str!("form-api-item.md")]
        members: Vec<FormMember>,
    },
}
cadmpeg_core::named_optional_field!(deserialize_cardinality, u32, "cardinality");

#[derive(Debug, Clone, PartialEq, Eq)]
#[doc = include_str!("form-api-item.md")]
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
            "if": {"required":["cages"], "properties":{"cages":{"type":"array"}}},
            "then": {"properties":{"cages":{"uniqueItems":true},"cage_inputs":{"properties":{"membership":{"properties":{"state":{"const":"complete"}}}}}}},
            "else": {"not":{"required":["cage_inputs"]}, "properties":{"cages":{"properties":{"membership":{"properties":{"state":{"const":"partial"}}}}}}}
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
    #[doc = include_str!("form-api-item.md")]
    pub fn try_new(
        inputs: FormInputs,
        observations: Vec<FormCageObservation>,
        membership: FormMembership,
    ) -> Result<Self, &'static str> {
        for reference in &inputs.reference_evidence {
            if reference
                .inline_type_guid
                .as_deref()
                .is_some_and(|guid| !form_guid(guid))
                || matches!(&reference.target, FormNativeReferenceTarget::Document { target_type_guid, .. } if !form_guid(target_type_guid))
            {
                return Err("invalid Form reference type GUID");
            }
        }
        for list in &inputs.lists {
            let mut ordinals = HashSet::new();
            if list.members.iter().any(|member| {
                member.ordinal >= list.declared_count || !ordinals.insert(member.ordinal)
            }) {
                return Err("invalid native Form list position");
            }
        }
        for (index, evidence) in inputs.position_evidence.iter().enumerate() {
            if evidence.observation.0 >= observations.len()
                || inputs.position_evidence[..index].contains(evidence)
            {
                return Err("invalid Form position evidence");
            }
        }
        let mut counts = HashSet::new();
        if inputs
            .cardinality_evidence
            .iter()
            .any(|count| !counts.insert(count))
        {
            return Err("duplicate Form cardinality evidence");
        }
        for (index, resource) in inputs.resource_evidence.iter().enumerate() {
            if inputs.resource_evidence[..index].contains(resource) {
                return Err("duplicate Form resource evidence");
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
        let mut decoded_resources = HashSet::new();
        let mut ambiguous_resource = false;
        for observation in &observations {
            if let FormCageBinding::Decoded {
                resource: Some(resource),
                ..
            } = &observation.binding
            {
                ambiguous_resource |= !decoded_resources.insert(resource.entry.as_str());
            }
        }
        if observations
            .iter()
            .any(|observation| match &observation.binding {
                FormCageBinding::Undecoded { resource, .. } => {
                    decoded_resources.contains(resource.entry.as_str())
                }
                _ => false,
            })
        {
            return Err("contradictory Form resource availability");
        }
        if ambiguous_resource {
            return Err("ambiguous Form resource association");
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
        let claims_match = match &membership {
            FormMembership::Complete { members, .. } => {
                inputs
                    .cardinality_evidence
                    .iter()
                    .all(|&count| u32::try_from(members.len()).ok() == Some(count))
                    && inputs.position_evidence.iter().all(|evidence| {
                        usize::try_from(evidence.ordinal)
                            .ok()
                            .and_then(|ordinal| members.get(ordinal))
                            == Some(&evidence.observation)
                    })
            }
            FormMembership::Partial { .. } => true,
        };
        if !valid || !claims_match {
            return Err("invalid Form membership");
        }
        Ok(Self {
            inputs,
            observations,
            membership,
        })
    }
    #[doc = include_str!("form-api-item.md")]
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
    #[doc = include_str!("form-api-item.md")]
    pub fn inputs(&self) -> &FormInputs {
        &self.inputs
    }
    #[doc = include_str!("form-api-item.md")]
    pub fn observations(&self) -> &[FormCageObservation] {
        &self.observations
    }
    #[doc = include_str!("form-api-item.md")]
    pub fn membership(&self) -> &FormMembership {
        &self.membership
    }
    #[doc = include_str!("form-api-item.md")]
    pub fn membership_is_complete(&self) -> bool {
        matches!(self.membership, FormMembership::Complete { .. })
    }
    #[doc = include_str!("form-api-item.md")]
    pub fn referenced_cages(&self) -> impl Iterator<Item = &SubdId> {
        self.observations
            .iter()
            .filter_map(|observation| match &observation.binding {
                FormCageBinding::Decoded { cage, .. } => Some(cage),
                _ => None,
            })
    }
    #[doc = include_str!("form-api-item.md")]
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

fn form_guid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
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
#[serde(untagged)]
enum FormCagesWire {
    Complete(Vec<SubdId>),
    Retained(FormEvidence),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct FormWire {
    cages: FormCagesWire,
    #[serde(default, skip_serializing_if = "FormInputs::is_empty")]
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
rewrite_record!(FormNativeReference, []; { source, role, ordinal, run, inline_type_guid, target, resource });
rewrite_record!(FormPositionEvidence, []; { source, observation, ordinal });
rewrite_enum!(FormNativeReferenceTarget, []; { Local { target }, Segment { target, segment }, Document { target, segment, asset_guid, target_type_guid, link_name, version } });
rewrite_record!(FormExternalVersion, []; { key, urn });
rewrite_record!(FormNativeMember, []; { ordinal, reference });
rewrite_record!(FormInputPath, []; { references });
rewrite_record!(FormCageObservation, []; { paths, binding });

rewrite_record!(FormMember, []; { observation, ordinal });
rewrite_scalar!(FormListRole);
rewrite_scalar!(FormResourceAvailability);
rewrite_scalar!(FormReferenceRole);
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
        self.lists.visit_identity_references(ctx, visitor)?;
        self.reference_evidence
            .visit_identity_references(ctx, visitor)?;
        self.position_evidence
            .visit_identity_references(ctx, visitor)?;
        self.resource_evidence
            .visit_identity_references(ctx, visitor)
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
            reference_evidence: self.reference_evidence.rewrite_identities(ctx, map)?,
            position_evidence: self.position_evidence.rewrite_identities(ctx, map)?,
            cardinality_evidence: self.cardinality_evidence,
            resource_evidence: self.resource_evidence.rewrite_identities(ctx, map)?,
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
