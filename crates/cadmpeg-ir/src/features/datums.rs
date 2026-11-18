use super::{FeatureDirection3, FeatureOperation, FinitePoint3, UnresolvedFamily};
use crate::design::construction::ConstructionInput;
use crate::design::options::NativeOptions;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};

#[doc = "Readable geometry, construction inputs and source options of a datum axis."]
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "DatumAxisDraft")]
pub struct DatumAxis {
    origin: Option<FinitePoint3>,
    direction: Option<FeatureDirection3>,
    inputs: Vec<ConstructionInput>,
    native_options: NativeOptions,
    source_settings: NativeOptions,
}

impl Serialize for DatumAxis {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let fields = 3 + usize::from(self.origin.is_some()) + usize::from(self.direction.is_some());
        let mut wire = serializer.serialize_struct("DatumAxisDraft", fields)?;
        if let Some(origin) = &self.origin {
            wire.serialize_field("origin", origin)?;
        }
        if let Some(direction) = &self.direction {
            wire.serialize_field("direction", direction)?;
        }
        wire.serialize_field("inputs", &self.inputs)?;
        wire.serialize_field("native_options", &self.native_options)?;
        wire.serialize_field("source_settings", &self.source_settings)?;
        wire.end()
    }
}

#[doc = "Input fields for checked datum-axis construction."]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DatumAxisDraft {
    #[doc = "A known point on the axis."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_origin"
    )]
    pub origin: Option<FinitePoint3>,
    #[doc = "The known direction."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_direction"
    )]
    pub direction: Option<FeatureDirection3>,
    #[doc = "Established construction input occurrences."]
    pub inputs: Vec<ConstructionInput>,
    #[doc = "Options whose meaning remains unresolved."]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    #[doc = "Source settings with established equivalent design meaning."]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

impl DatumAxisDraft {
    #[doc = "Whether the draft carries a readable axis fact."]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.origin.is_none()
            && self.direction.is_none()
            && self.inputs.is_empty()
            && self.native_options.is_empty()
            && self.source_settings.is_empty()
    }

    #[doc = "Canonicalize a readable draft or the unresolved axis family."]
    pub fn into_operation(self) -> Result<FeatureOperation, &'static str> {
        if self.is_empty() {
            Ok(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumAxis,
            })
        } else {
            DatumAxis::try_from(self).map(|axis| FeatureOperation::DatumAxis { axis })
        }
    }
}

impl TryFrom<DatumAxisDraft> for DatumAxis {
    type Error = &'static str;
    fn try_from(draft: DatumAxisDraft) -> Result<Self, Self::Error> {
        if draft.is_empty() {
            return Err("an empty datum axis uses the unresolved datum-axis family");
        }
        if draft
            .native_options
            .keys()
            .any(|key| draft.source_settings.contains_key(key))
        {
            return Err("datum-axis option keys must occur in one classification");
        }
        let DatumAxisDraft {
            origin,
            direction,
            inputs,
            native_options,
            source_settings,
        } = draft;
        Ok(Self {
            origin,
            direction,
            inputs,
            native_options,
            source_settings,
        })
    }
}

impl From<DatumAxis> for DatumAxisDraft {
    fn from(axis: DatumAxis) -> Self {
        let DatumAxis {
            origin,
            direction,
            inputs,
            native_options,
            source_settings,
        } = axis;
        Self {
            origin,
            direction,
            inputs,
            native_options,
            source_settings,
        }
    }
}

impl DatumAxis {
    #[doc = "Construct a complete fixed axis."]
    #[must_use]
    pub fn complete(origin: FinitePoint3, direction: FeatureDirection3) -> Self {
        Self {
            origin: Some(origin),
            direction: Some(direction),
            inputs: Vec::new(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        }
    }

    #[doc = "The known origin."]
    #[must_use]
    pub fn origin(&self) -> Option<&FinitePoint3> {
        self.origin.as_ref()
    }

    #[doc = "The known direction."]
    #[must_use]
    pub fn direction(&self) -> Option<&FeatureDirection3> {
        self.direction.as_ref()
    }

    #[doc = "The complete geometric pair, when both components are known."]
    #[must_use]
    pub fn geometry(&self) -> Option<(&FinitePoint3, &FeatureDirection3)> {
        self.origin().zip(self.direction())
    }

    #[doc = "Construction input occurrences."]
    #[must_use]
    pub fn inputs(&self) -> &[ConstructionInput] {
        &self.inputs
    }

    #[doc = "Options with unresolved meaning."]
    #[must_use]
    pub fn native_options(&self) -> &NativeOptions {
        &self.native_options
    }

    #[doc = "Source settings with equivalent design meaning."]
    #[must_use]
    pub fn source_settings(&self) -> &NativeOptions {
        &self.source_settings
    }

    #[doc = "Whether the axis has attached construction or source controls."]
    #[must_use]
    pub fn has_controls(&self) -> bool {
        !self.inputs.is_empty()
            || !self.native_options.is_empty()
            || !self.source_settings.is_empty()
    }
}

impl FeatureOperation {
    #[doc = "Construct a datum axis with complete geometry and empty controls."]
    #[must_use]
    pub fn datum_axis(origin: FinitePoint3, direction: FeatureDirection3) -> Self {
        Self::DatumAxis {
            axis: DatumAxis::complete(origin, direction),
        }
    }
}

rewrite_record!(DatumAxis, []; {origin, direction, inputs, native_options, source_settings});

#[cfg(feature = "schema")]
impl JsonSchema for DatumAxis {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "DatumAxis".into()
    }
    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut schema = DatumAxisDraft::json_schema(generator);
        schema.insert(
            "allOf".into(),
            serde_json::json!([{"anyOf": [
                {"required":["origin"]}, {"required":["direction"]},
                {"properties":{"inputs":{"minItems":1}}},
                {"properties":{"native_options":{"minProperties":1}}},
                {"properties":{"source_settings":{"minProperties":1}}}
            ]}]),
        );
        if let Some(serde_json::Value::Object(properties)) = schema.get_mut("properties") {
            properties.insert(
                "origin".into(),
                generator.subschema_for::<FinitePoint3>().into(),
            );
            properties.insert(
                "direction".into(),
                generator.subschema_for::<FeatureDirection3>().into(),
            );
        }
        schema
    }
}

cadmpeg_core::named_optional_field!(deserialize_origin, FinitePoint3, "origin");
cadmpeg_core::named_optional_field!(deserialize_direction, FeatureDirection3, "direction");

#[cfg(test)]
mod tests;
