use crate::features::selections::NonEmptyMembers;
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
#[doc = "Native inputs and evidence for a Form construction."]
pub struct FormInputs {
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    #[doc = "Native parameter expressions indexed by parameter name."]
    pub parameters: BTreeMap<NonBlankString, String>,
    #[doc = "Ordered direct native references, including repetitions."]
    pub references: Vec<NativeId>,
    #[cfg_attr(
        feature = "schema",
        schemars(schema_with = "form_reference_runs_schema")
    )]
    #[doc = "Alternative direct-reference runs whose order is unresolved."]
    pub reference_runs: Vec<Vec<NativeId>>,
    #[cfg_attr(
        feature = "schema",
        schemars(schema_with = "unique_form_vec::<FormNativeList>")
    )]
    #[doc = "Native lists with their source paths, roles, declared counts, and readable members."]
    pub lists: Vec<FormNativeList>,
    #[cfg_attr(
        feature = "schema",
        schemars(schema_with = "unique_form_vec::<FormNativeReference>")
    )]
    #[doc = "Native reference slots with their roles and target evidence."]
    pub reference_evidence: Vec<FormNativeReference>,
    #[cfg_attr(
        feature = "schema",
        schemars(schema_with = "unique_form_vec::<FormPositionEvidence>")
    )]
    #[doc = "Evidence that places an observation at a source ordinal."]
    pub position_evidence: Vec<FormPositionEvidence>,
    #[cfg_attr(
        feature = "schema",
        schemars(schema_with = "unique_form_vec::<FormOrderEvidence>")
    )]
    #[doc = "Evidence for relative order among observations."]
    pub order_evidence: Vec<FormOrderEvidence>,
    #[cfg_attr(feature = "schema", schemars(schema_with = "unique_form_vec::<u32>"))]
    #[doc = "Independent claims about the number of members."]
    pub cardinality_evidence: Vec<u32>,
    #[cfg_attr(
        feature = "schema",
        schemars(schema_with = "unique_form_vec::<FormClosureEvidence>")
    )]
    #[doc = "Earlier complete membership claims retained as evidence."]
    pub closure_evidence: Vec<FormClosureEvidence>,
    #[cfg_attr(
        feature = "schema",
        schemars(schema_with = "unique_form_vec::<FormResource>")
    )]
    #[doc = "Native serializer resource identities."]
    pub resource_evidence: Vec<FormResource>,
    #[cfg_attr(
        feature = "schema",
        schemars(schema_with = "unique_form_vec::<FormResourceAssociation>")
    )]
    #[doc = "Additional associations between decoded cages and resources."]
    pub resource_associations: Vec<FormResourceAssociation>,
    #[cfg_attr(
        feature = "schema",
        schemars(schema_with = "unique_form_vec::<FormResourceStateEvidence>")
    )]
    #[doc = "Claims about the availability of serializer resources."]
    pub resource_state_evidence: Vec<FormResourceStateEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "A source claim that places an observation at an ordinal."]
pub struct FormPositionEvidence {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_position_source"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "FormInputPath"))]
    #[doc = "Source path for the position claim, when available."]
    pub source: Option<FormInputPath>,
    #[doc = "Index of the observation in the canonical observation vector."]
    pub observation: FormObservationIndex,
    #[doc = "Zero-based position claimed for the observation."]
    pub ordinal: u32,
}
cadmpeg_core::named_optional_field!(deserialize_position_source, FormInputPath, "source");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "A source or assumed relative order of observations."]
pub struct FormOrderEvidence {
    #[doc = "Origin of the order claim."]
    pub order: FormKnownOrder,
    #[doc = "Observation indices in the claimed relative order."]
    #[cfg_attr(
        feature = "schema",
        schemars(schema_with = "form_order_members_schema")
    )]
    pub members: Vec<FormObservationIndex>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "An earlier complete membership claim retained as evidence."]
pub struct FormClosureEvidence {
    #[doc = "Observation indices in the claimed membership."]
    pub members: Vec<FormObservationIndex>,
    #[doc = "Origin of the sequence order."]
    pub order: FormKnownOrder,
    #[doc = "Origin of the membership claim."]
    pub certainty: FormKnownOrder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", schemars(extend("allOf" = [
    {"if": {"properties": {"target": {"properties": {"kind": {"const": "document"}}}}}, "then": {"anyOf": [{"required": ["inline_type_guid"]}, {"properties": {"target": {"required": ["target_type_guid"]}}}]}},
    {"if": {"properties": {"target": {"properties": {"kind": {"enum": ["null", "unreadable", "unvisited"]}}}}}, "then": {"not": {"required": ["inline_type_guid"]}}}
])))]
#[doc = "A native reference slot and its retained target evidence."]
pub struct FormNativeReference {
    #[doc = "Native record that contains the reference."]
    pub source: NativeId,
    #[doc = "Role of the reference in its source record."]
    pub role: FormReferenceRole,
    #[doc = "Zero-based source slot for the reference."]
    pub ordinal: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_reference_group"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "FormReferenceGroup"))]
    #[doc = "Label that groups alternative references with the same source and role."]
    pub group: Option<FormReferenceGroup>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_inline_type_guid"
    )]
    #[cfg_attr(
        feature = "schema",
        schemars(
            with = "String",
            length(min = 36, max = 36),
            regex(
                pattern = "^[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}$"
            )
        )
    )]
    #[doc = "Type GUID encoded inline with the reference, when present."]
    pub inline_type_guid: Option<String>,
    #[doc = "Target identity or the state of an unreadable slot."]
    pub target: FormNativeReferenceTarget,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_resource"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "FormResource"))]
    #[doc = "Serializer resource associated with the reference, when present."]
    pub resource: Option<FormResource>,
}
impl FormNativeReference {
    #[doc = "Create a reference with its source, role, ordinal, and target."]
    pub fn new(
        source: NativeId,
        role: FormReferenceRole,
        ordinal: u32,
        target: FormNativeReferenceTarget,
    ) -> Self {
        Self {
            source,
            role,
            ordinal,
            group: None,
            inline_type_guid: None,
            target,
            resource: None,
        }
    }

    #[doc = "Attach an alternative group label to the reference."]
    #[must_use]
    pub fn with_group(mut self, group: FormReferenceGroup) -> Self {
        self.group = Some(group);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
#[doc = "Label that distinguishes reference alternatives within a source and role."]
pub struct FormReferenceGroup(u64);

impl FormReferenceGroup {
    #[doc = "Create a group label from its stored value."]
    pub const fn from_label(label: u64) -> Self {
        Self(label)
    }
}

cadmpeg_core::named_optional_field!(deserialize_inline_type_guid, String, "inline_type_guid");
cadmpeg_core::named_optional_field!(deserialize_reference_group, FormReferenceGroup, "group");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[doc = "Role of a native Form reference slot."]
pub enum FormReferenceRole {
    #[doc = "A body reference whose ordinal identifies its position in the body chain."]
    Body,
    #[doc = "The owner of a tracking selection."]
    TrackingOwner,
    #[doc = "A member of a tracking selection."]
    TrackingMember,
    #[doc = "A tracking member under the assumed fixed-slot reading."]
    TrackingMemberFixedAssumed,
    #[doc = "A serializer surface reference at ordinal zero."]
    SerializerSurface,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[doc = "Target or read state of a native reference slot."]
pub enum FormNativeReferenceTarget {
    #[doc = "An explicit absent reference."]
    Null {},
    #[doc = "A slot that could not be read."]
    Unreadable {},
    #[doc = "A slot whose boundary could not be established."]
    Unvisited {},
    #[doc = "A reference to a typed local native record."]
    Local {
        #[doc = "Typed native identity of the target."]
        target: NativeId,
    },
    #[doc = "A reference to an entity in a segment of the document."]
    Segment {
        #[doc = "Native entity number in the segment."]
        target: u64,
        #[doc = "Segment number containing the target."]
        segment: u32,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_asset_guid"
        )]
        #[cfg_attr(feature = "schema", schemars(with = "String"))]
        #[doc = "Asset GUID from a qualified same-document reference, when present."]
        asset_guid: Option<String>,
    },
    #[doc = "A reference to an entity in another document."]
    Document {
        #[doc = "Native entity number in the target segment."]
        target: u64,
        #[doc = "Segment number containing the target."]
        segment: u32,
        #[doc = "Literal asset GUID of the target document."]
        asset_guid: String,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_target_type_guid"
        )]
        #[doc = "Separately encoded target type GUID, when present."]
        #[cfg_attr(
            feature = "schema",
            schemars(
                with = "String",
                length(min = 36, max = 36),
                regex(
                    pattern = "^[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}$"
                )
            )
        )]
        target_type_guid: Option<String>,
        #[doc = "Literal link name for the target document."]
        link_name: String,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_form_external_version"
        )]
        #[cfg_attr(feature = "schema", schemars(with = "FormExternalVersion"))]
        #[doc = "External version identifiers, when present."]
        version: Option<FormExternalVersion>,
    },
}
cadmpeg_core::named_optional_field!(deserialize_asset_guid, String, "asset_guid");
cadmpeg_core::named_optional_field!(deserialize_target_type_guid, String, "target_type_guid");
cadmpeg_core::named_optional_field!(
    deserialize_form_external_version,
    FormExternalVersion,
    "version"
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "Literal version identifiers for an external document reference."]
pub struct FormExternalVersion {
    #[doc = "Version key from the reference."]
    pub key: String,
    #[doc = "Version URN from the reference."]
    pub urn: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "A native list with its declared count and readable members."]
pub struct FormNativeList {
    #[doc = "Native reference path to the list."]
    pub source: FormInputPath,
    #[doc = "Role of the list in the Form input."]
    pub role: FormListRole,
    #[doc = "Number of slots declared by the source list."]
    pub declared_count: u32,
    #[doc = "Readable members with their source positions."]
    #[cfg_attr(
        feature = "schema",
        schemars(schema_with = "unique_form_vec::<FormNativeMember>")
    )]
    pub members: Vec<FormNativeMember>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[doc = "Role of a native Form input list."]
pub enum FormListRole {
    #[doc = "The list of cage objects."]
    CageObjects,
    #[doc = "The tracking selection list."]
    TrackingSelection,
    #[doc = "The assumed fixed-slot reading of a sparse tracking selection."]
    TrackingSelectionFixedAssumed,
    #[doc = "The cage catalog lane."]
    CageCatalog,
    #[doc = "The companion lane of the cage catalog."]
    CageCatalogCompanions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "A readable native list member and its source slot."]
pub struct FormNativeMember {
    #[doc = "Zero-based source slot of the member."]
    pub ordinal: u32,
    #[doc = "Native record identity referenced by the member."]
    pub reference: NativeId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "A nonempty native reference path."]
pub struct FormInputPath {
    #[doc = "Native identities in path order."]
    pub references: NonEmptyMembers<NativeId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "Paths and binding evidence for one canonical cage observation."]
pub struct FormCageObservation {
    #[doc = "Independently established native paths to the observation."]
    #[cfg_attr(
        feature = "schema",
        schemars(schema_with = "unique_form_vec::<FormInputPath>")
    )]
    pub paths: Vec<FormInputPath>,
    #[doc = "Binding of the observation to a cage or resource."]
    pub binding: FormCageBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[doc = "Binding state of a Form cage observation."]
pub enum FormCageBinding {
    #[doc = "A decoded neutral cage and its optional source resource."]
    Decoded {
        #[doc = "Identity of the decoded subdivision cage."]
        cage: SubdId,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_resource"
        )]
        #[cfg_attr(feature = "schema", schemars(with = "FormResource"))]
        #[doc = "Source resource of the decoded cage, when known."]
        resource: Option<FormResource>,
    },
    #[doc = "A resource retained without a decoded cage."]
    Undecoded {
        #[doc = "Identity of the retained resource."]
        resource: FormResource,
        #[doc = "Availability of the retained resource."]
        availability: FormResourceAvailability,
    },
    #[doc = "An observation whose path has no resolved cage or resource."]
    Unresolved {},
}
cadmpeg_core::named_optional_field!(deserialize_resource, FormResource, "resource");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "A serializer resource identified by its source and archive entry."]
pub struct FormResource {
    #[doc = "Native source identity associated with the resource."]
    pub source: NativeId,
    #[doc = "Literal archive entry name."]
    pub entry: NonBlankString,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "An additional association between a decoded cage and a resource."]
pub struct FormResourceAssociation {
    #[doc = "Identity of the decoded cage."]
    pub cage: SubdId,
    #[doc = "Resource associated with the cage."]
    pub resource: FormResource,
}

impl FormInputs {
    fn cage_resources<'a>(
        &'a self,
        observations: &'a [FormCageObservation],
        cage: &'a SubdId,
    ) -> impl Iterator<Item = &'a FormResource> {
        observations
            .iter()
            .filter_map(move |observation| match &observation.binding {
                FormCageBinding::Decoded {
                    cage: bound,
                    resource: Some(resource),
                } if bound == cage => Some(resource),
                _ => None,
            })
            .chain(
                self.resource_associations
                    .iter()
                    .filter(move |association| &association.cage == cage)
                    .map(|association| &association.resource),
            )
    }

    #[doc = "Report whether the cage has incompatible resource states or associations."]
    pub fn cage_resource_conflicts(
        &self,
        observations: &[FormCageObservation],
        cage: &SubdId,
    ) -> bool {
        self.cage_resources(observations, cage).any(|resource| {
            self.resource_state_evidence.iter().any(|state| {
                state.resource.entry == resource.entry
                    && matches!(
                        state.availability,
                        FormResourceAvailability::Inactive | FormResourceAvailability::Ambiguous
                    )
            }) || observations.iter().any(|observation| match &observation.binding {
                FormCageBinding::Undecoded {
                    resource: candidate,
                    availability: FormResourceAvailability::Inactive | FormResourceAvailability::Ambiguous,
                } => candidate.entry == resource.entry,
                FormCageBinding::Decoded { cage: other, .. } if other != cage => self
                    .cage_resources(observations, other)
                    .any(|candidate| candidate.entry == resource.entry),
                _ => false,
            })
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[doc = "Availability state of a serializer resource."]
pub enum FormResourceAvailability {
    #[doc = "The active entry is present without a decoded cage."]
    ActiveUndecoded,
    #[doc = "The active folder is known and does not contain the entry."]
    Inactive,
    #[doc = "The active folder is unavailable."]
    Unknown,
    #[doc = "The evidence does not establish one compatible resource state or association."]
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "A resource and an availability claim retained as evidence."]
pub struct FormResourceStateEvidence {
    #[doc = "Resource to which the availability claim applies."]
    pub resource: FormResource,
    #[doc = "Claimed availability of the resource."]
    pub availability: FormResourceAvailability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
#[doc = "Index into the canonical cage observation vector."]
pub struct FormObservationIndex(#[doc = "Zero-based observation index."] pub usize);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "A selected observation with an optional source position."]
pub struct FormMember {
    #[doc = "Index of the selected observation."]
    pub observation: FormObservationIndex,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_ordinal"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "u32"))]
    #[doc = "Zero-based position of the member, when known."]
    pub ordinal: Option<u32>,
}
cadmpeg_core::named_optional_field!(deserialize_ordinal, u32, "ordinal");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[doc = "Origin of an established order or membership claim."]
pub enum FormKnownOrder {
    #[doc = "The claim comes from source data."]
    Source,
    #[doc = "The claim rests on an assumption."]
    Assumed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[doc = "Origin of an order claim, including unknown order."]
pub enum FormOrder {
    #[doc = "The order comes from source data."]
    Source,
    #[doc = "The order rests on an assumption."]
    Assumed,
    #[doc = "The order is unknown."]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[doc = "Current complete or partial selection of cage observations."]
pub enum FormMembership {
    #[doc = "An ordered selection of distinct decoded observations."]
    Complete {
        #[doc = "Observation indices in the selected order."]
        #[cfg_attr(
            feature = "schema",
            schemars(schema_with = "unique_form_vec::<FormObservationIndex>")
        )]
        members: Vec<FormObservationIndex>,
        #[doc = "Origin of the selected order."]
        order: FormKnownOrder,
        #[doc = "Origin of the complete membership claim."]
        certainty: FormKnownOrder,
    },
    #[doc = "A partial selection with optional cardinality and positions."]
    Partial {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cardinality"
        )]
        #[cfg_attr(feature = "schema", schemars(with = "u32"))]
        #[doc = "Claimed member count, when known."]
        cardinality: Option<u32>,
        #[doc = "Origin of the order claim, or unknown order."]
        order: FormOrder,
        #[doc = "Confirmed members and their known positions."]
        #[cfg_attr(
            feature = "schema",
            schemars(schema_with = "unique_form_vec::<FormMember>")
        )]
        members: Vec<FormMember>,
    },
}
cadmpeg_core::named_optional_field!(deserialize_cardinality, u32, "cardinality");

#[derive(Debug, Clone, PartialEq, Eq)]
#[doc = "Form inputs, canonical cage observations, and current membership."]
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
        schema.insert("allOf".into(), cadmpeg_core::json!([{
            "if": {"required":["cages"], "properties":{"cages":{"properties":{"kind":{"const":"complete"}}}}},
            "then": {"properties":{"cages":{"properties":{"value":{"uniqueItems":true}}},"cage_inputs":{"properties":{"membership":{"properties":{"kind":{"const":"complete"}}}}}}},
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
    #[doc = "Validate and construct a Form from its inputs, observations, and membership."]
    pub fn try_new(
        inputs: FormInputs,
        observations: Vec<FormCageObservation>,
        membership: FormMembership,
    ) -> Result<Self, &'static str> {
        if has_duplicates(&inputs.lists)
            || has_duplicates(&inputs.reference_evidence)
            || has_duplicates(&inputs.closure_evidence)
        {
            return Err("duplicate Form source evidence");
        }
        for reference in &inputs.reference_evidence {
            if (reference.inline_type_guid.is_some()
                && matches!(
                    &reference.target,
                    FormNativeReferenceTarget::Null {}
                        | FormNativeReferenceTarget::Unreadable {}
                        | FormNativeReferenceTarget::Unvisited {}
                ))
                || reference
                    .inline_type_guid
                    .as_deref()
                    .is_some_and(|guid| !form_guid(guid))
                || matches!(&reference.target, FormNativeReferenceTarget::Document { target_type_guid, .. } if target_type_guid.as_deref().is_some_and(|guid| !form_guid(guid)) || (target_type_guid.is_none() && reference.inline_type_guid.is_none()))
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
        for (index, evidence) in inputs.order_evidence.iter().enumerate() {
            if evidence.members.len() < 2
                || evidence
                    .members
                    .iter()
                    .any(|member| member.0 >= observations.len())
                || inputs.order_evidence[..index].contains(evidence)
            {
                return Err("invalid Form order evidence");
            }
        }
        if inputs.closure_evidence.iter().any(|claim| {
            claim
                .members
                .iter()
                .any(|member| member.0 >= observations.len())
        }) {
            return Err("invalid Form closure evidence");
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
        for (index, state) in inputs.resource_state_evidence.iter().enumerate() {
            if inputs.resource_state_evidence[..index].contains(state) {
                return Err("duplicate Form resource state evidence");
            }
        }
        for (index, run) in inputs.reference_runs.iter().enumerate() {
            if !inputs.references.is_empty()
                || inputs.reference_runs.len() < 2
                || run.is_empty()
                || inputs.reference_runs[..index]
                    .iter()
                    .any(|other| other.starts_with(run) || run.starts_with(other))
            {
                return Err("invalid Form reference runs");
            }
        }
        if observations
            .iter()
            .any(|observation| has_duplicates(&observation.paths))
        {
            return Err("duplicate Form observation path");
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
        for (index, association) in inputs.resource_associations.iter().enumerate() {
            if inputs.resource_associations[..index].contains(association)
                || !cages.contains(&association.cage)
                || observations.iter().any(|observation| {
                    matches!(&observation.binding,
                        FormCageBinding::Decoded { cage, resource: Some(resource) }
                        if cage == &association.cage && resource == &association.resource)
                })
            {
                return Err("invalid Form resource association");
            }
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
        let mut undecoded_resources = HashSet::new();
        for observation in &observations {
            if let FormCageBinding::Undecoded {
                resource,
                availability,
            } = &observation.binding
            {
                if !undecoded_resources.insert(resource.entry.as_str()) {
                    return Err("duplicate Form resource observation");
                }
                if inputs.resource_state_evidence.iter().any(|claim| {
                    claim.resource.entry == resource.entry
                        && match availability {
                            FormResourceAvailability::ActiveUndecoded => matches!(
                                claim.availability,
                                FormResourceAvailability::Inactive
                                    | FormResourceAvailability::Ambiguous
                            ),
                            FormResourceAvailability::Inactive => matches!(
                                claim.availability,
                                FormResourceAvailability::ActiveUndecoded
                                    | FormResourceAvailability::Ambiguous
                            ),
                            FormResourceAvailability::Unknown
                            | FormResourceAvailability::Ambiguous => false,
                        }
                }) {
                    return Err("contradictory Form resource state evidence");
                }
            }
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
                && !matches!(&observation.binding, FormCageBinding::Undecoded { resource, availability: FormResourceAvailability::Unknown } if inputs.resource_state_evidence.iter().any(|claim| claim.resource.entry == resource.entry && matches!(claim.availability, FormResourceAvailability::Inactive | FormResourceAvailability::Ambiguous)))
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
        let selected_positions_match = match &membership {
            FormMembership::Complete { .. } => true,
            FormMembership::Partial { members, .. } => members.iter().all(|member| {
                member.ordinal.is_none_or(|ordinal| {
                    inputs.position_evidence.iter().all(|claim| {
                        (claim.observation != member.observation || claim.ordinal == ordinal)
                            && (claim.ordinal != ordinal || claim.observation == member.observation)
                    })
                })
            }),
        };
        let selected_count_matches = match &membership {
            FormMembership::Complete { .. } => true,
            FormMembership::Partial {
                cardinality,
                members,
                ..
            } => cardinality.is_none_or(|count| {
                inputs
                    .cardinality_evidence
                    .iter()
                    .all(|claim| *claim == count)
                    && inputs.closure_evidence.iter().all(|claim| {
                        u32::try_from(claim.members.len()).ok() == Some(count)
                            && members
                                .iter()
                                .all(|member| claim.members.contains(&member.observation))
                    })
            }),
        };
        let claims_match = match &membership {
            FormMembership::Complete {
                members,
                order,
                certainty,
            } => {
                inputs
                    .cardinality_evidence
                    .iter()
                    .all(|&count| u32::try_from(members.len()).ok() == Some(count))
                    && inputs.closure_evidence.iter().all(|claim| {
                        claim.members == *members
                            && (*order == FormKnownOrder::Source
                                || claim.order == FormKnownOrder::Assumed)
                            && (*certainty == FormKnownOrder::Source
                                || claim.certainty == FormKnownOrder::Assumed)
                    })
                    && inputs.position_evidence.iter().all(|evidence| {
                        usize::try_from(evidence.ordinal)
                            .ok()
                            .and_then(|ordinal| members.get(ordinal))
                            == Some(&evidence.observation)
                    })
                    && inputs
                        .order_evidence
                        .iter()
                        .all(|evidence| is_subsequence(&evidence.members, members))
                    && members.iter().all(|index| {
                        match observations
                            .get(index.0)
                            .map(|observation| &observation.binding)
                        {
                            Some(FormCageBinding::Decoded { cage, .. }) => {
                                !inputs.cage_resource_conflicts(&observations, cage)
                            }
                            _ => true,
                        }
                    })
            }
            FormMembership::Partial {
                order: FormOrder::Unknown,
                ..
            } => true,
            FormMembership::Partial { members, .. } => {
                let selected = members
                    .iter()
                    .map(|member| member.observation)
                    .collect::<Vec<_>>();
                let mut last = None;
                let positions_in_order =
                    members
                        .iter()
                        .filter_map(|member| member.ordinal)
                        .all(|ordinal| {
                            let valid = last.is_none_or(|previous| previous < ordinal);
                            last = Some(ordinal);
                            valid
                        });
                let selected_positions = members
                    .iter()
                    .enumerate()
                    .flat_map(|(index, member)| {
                        member
                            .ordinal
                            .into_iter()
                            .chain(inputs.position_evidence.iter().filter_map(move |claim| {
                                (claim.observation == member.observation).then_some(claim.ordinal)
                            }))
                            .map(move |ordinal| (index, ordinal))
                    })
                    .collect::<Vec<_>>();
                let evidence_positions_in_order = selected_positions.iter().all(|&(left, a)| {
                    selected_positions
                        .iter()
                        .all(|&(right, b)| match left.cmp(&right) {
                            std::cmp::Ordering::Less => a < b,
                            std::cmp::Ordering::Equal => a == b,
                            std::cmp::Ordering::Greater => a > b,
                        })
                });
                positions_in_order
                    && evidence_positions_in_order
                    && inputs
                        .order_evidence
                        .iter()
                        .map(|evidence| evidence.members.as_slice())
                        .chain(
                            inputs
                                .closure_evidence
                                .iter()
                                .map(|claim| claim.members.as_slice()),
                        )
                        .all(|sequence| {
                            let relevant = sequence
                                .iter()
                                .copied()
                                .filter(|member| selected.contains(member))
                                .collect::<Vec<_>>();
                            is_subsequence(&relevant, &selected)
                        })
            }
        };
        if !valid || !selected_positions_match || !selected_count_matches || !claims_match {
            return Err("invalid Form membership");
        }
        Ok(Self {
            inputs,
            observations,
            membership,
        })
    }
    #[doc = "Construct complete membership from an ordered list of distinct cages."]
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
    #[doc = "Return the retained native inputs and evidence."]
    pub fn inputs(&self) -> &FormInputs {
        &self.inputs
    }
    #[doc = "Return the canonical cage observations."]
    pub fn observations(&self) -> &[FormCageObservation] {
        &self.observations
    }
    #[doc = "Return the current membership."]
    pub fn membership(&self) -> &FormMembership {
        &self.membership
    }
    #[doc = "Report whether the current membership is complete."]
    pub fn membership_is_complete(&self) -> bool {
        matches!(self.membership, FormMembership::Complete { .. })
    }
    #[doc = "Iterate over all decoded cages, including unselected candidates."]
    pub fn referenced_cages(&self) -> impl Iterator<Item = &SubdId> {
        self.observations
            .iter()
            .filter_map(|observation| match &observation.binding {
                FormCageBinding::Decoded { cage, .. } => Some(cage),
                _ => None,
            })
    }
    #[doc = "Iterate over decoded cages selected by the current membership."]
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

fn is_subsequence<T: PartialEq>(sequence: &[T], values: &[T]) -> bool {
    let mut remaining = values.iter();
    sequence
        .iter()
        .all(|item| remaining.any(|value| value == item))
}

fn has_duplicates<T: PartialEq>(values: &[T]) -> bool {
    values
        .iter()
        .enumerate()
        .any(|(index, value)| values[..index].contains(value))
}

#[cfg(feature = "schema")]
fn unique_form_vec<T: JsonSchema>(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let mut schema = generator.subschema_for::<Vec<T>>();
    schema.insert("uniqueItems".into(), true.into());
    schema
}

#[cfg(feature = "schema")]
fn form_reference_runs_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let mut schema = unique_form_vec::<Vec<NativeId>>(generator);
    let mut run = generator.subschema_for::<Vec<NativeId>>();
    run.insert("minItems".into(), 1.into());
    schema.insert("items".into(), run.into());
    schema.insert(
        "anyOf".into(),
        cadmpeg_core::json!([{"maxItems": 0}, {"minItems": 2}]),
    );
    schema
}

#[cfg(feature = "schema")]
fn form_order_members_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let mut schema = generator.subschema_for::<Vec<FormObservationIndex>>();
    schema.insert("minItems".into(), 2.into());
    schema
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct FormEvidence {
    observations: Vec<FormCageObservation>,
    membership: FormMembership,
}

#[derive(Debug, Clone, Serialize, cadmpeg_derive::DeserializeTagged)]
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
    fn rewrite_identities(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        map: &mut crate::schema::rewrite::typed::IdentityMap<'_>,
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
rewrite_record!(FormPositionEvidence, []; { source, observation, ordinal });
rewrite_record!(FormNativeMember, []; { ordinal, reference });
rewrite_record!(FormInputPath, []; { references });
rewrite_record!(FormCageObservation, []; { paths, binding });
rewrite_record!(FormResourceAssociation, []; { cage, resource });
rewrite_record!(FormResourceStateEvidence, []; { resource, availability });

rewrite_record!(FormMember, []; { observation, ordinal });
rewrite_scalar!(FormListRole);
rewrite_scalar!(FormResourceAvailability);
rewrite_scalar!(FormReferenceRole);
rewrite_scalar!(FormObservationIndex);
rewrite_scalar!(FormOrder);
rewrite_scalar!(FormKnownOrder);
rewrite_enum!(FormCageBinding, []; { Decoded { cage, resource }, Undecoded { resource, availability }, Unresolved {} });
rewrite_enum!(FormMembership, []; { Complete { members, order, certainty }, Partial { cardinality, order, members } });

impl crate::schema::rewrite::typed::RewriteIdentities for FormNativeReference {
    fn visit_identity_references(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        self.source.visit_identity_references(ctx, visitor)?;
        self.target.visit_identity_references(ctx, visitor)?;
        self.resource.visit_identity_references(ctx, visitor)
    }

    fn rewrite_identities(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        map: &mut crate::schema::rewrite::typed::IdentityMap<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self {
            source: self.source.rewrite_identities(ctx, map)?,
            role: self.role,
            ordinal: self.ordinal,
            group: self.group,
            inline_type_guid: self.inline_type_guid,
            target: self.target.rewrite_identities(ctx, map)?,
            resource: self.resource.rewrite_identities(ctx, map)?,
        })
    }
}

impl crate::schema::rewrite::typed::RewriteIdentities for FormNativeReferenceTarget {
    fn visit_identity_references(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        if let Self::Local { target } = self {
            target.visit_identity_references(ctx, visitor)?;
        }
        Ok(())
    }

    fn rewrite_identities(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        map: &mut crate::schema::rewrite::typed::IdentityMap<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(match self {
            Self::Local { target } => Self::Local {
                target: target.rewrite_identities(ctx, map)?,
            },
            literal => literal,
        })
    }
}

#[cfg(test)]
mod tests;

impl crate::schema::rewrite::typed::RewriteIdentities for FormInputs {
    fn visit_identity_references(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        self.references.visit_identity_references(ctx, visitor)?;
        self.reference_runs
            .visit_identity_references(ctx, visitor)?;
        self.lists.visit_identity_references(ctx, visitor)?;
        self.reference_evidence
            .visit_identity_references(ctx, visitor)?;
        self.position_evidence
            .visit_identity_references(ctx, visitor)?;
        self.resource_evidence
            .visit_identity_references(ctx, visitor)?;
        self.resource_associations
            .visit_identity_references(ctx, visitor)?;
        self.resource_state_evidence
            .visit_identity_references(ctx, visitor)
    }
    fn rewrite_identities(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        map: &mut crate::schema::rewrite::typed::IdentityMap<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self {
            parameters: self.parameters,
            references: self.references.rewrite_identities(ctx, map)?,
            reference_runs: self.reference_runs.rewrite_identities(ctx, map)?,
            lists: self.lists.rewrite_identities(ctx, map)?,
            reference_evidence: self.reference_evidence.rewrite_identities(ctx, map)?,
            position_evidence: self.position_evidence.rewrite_identities(ctx, map)?,
            order_evidence: self.order_evidence,
            cardinality_evidence: self.cardinality_evidence,
            closure_evidence: self.closure_evidence,
            resource_evidence: self.resource_evidence.rewrite_identities(ctx, map)?,
            resource_associations: self.resource_associations.rewrite_identities(ctx, map)?,
            resource_state_evidence: self.resource_state_evidence.rewrite_identities(ctx, map)?,
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
    fn rewrite_identities(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        map: &mut crate::schema::rewrite::typed::IdentityMap<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self {
            source: self.source.rewrite_identities(ctx, map)?,
            entry: self.entry,
        })
    }
}
