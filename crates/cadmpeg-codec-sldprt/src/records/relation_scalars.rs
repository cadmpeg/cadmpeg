// SPDX-License-Identifier: Apache-2.0
//! Relation scalar membership and selected parameter/display roles.

use super::{FeatureInputScalar, FeatureInputScalarRole};
use serde::{ser::SerializeMap, Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RelationScalars {
    refs: Vec<String>,
    parameter: Option<usize>,
    display: Option<usize>,
}

impl RelationScalars {
    pub(crate) fn from_scalars<'a>(
        scalars: impl IntoIterator<Item = &'a FeatureInputScalar>,
    ) -> Self {
        let scalars = scalars.into_iter().collect::<Vec<_>>();
        let unique = |role| {
            let mut indices = scalars
                .iter()
                .enumerate()
                .filter_map(|(index, scalar)| (scalar.role == role).then_some(index));
            let first = indices.next()?;
            indices.next().is_none().then_some(first)
        };
        Self {
            parameter: unique(FeatureInputScalarRole::Driving),
            display: unique(FeatureInputScalarRole::Display),
            refs: scalars
                .into_iter()
                .map(|scalar| scalar.id.clone())
                .collect(),
        }
    }

    pub(crate) fn from_refs(
        refs: Vec<String>,
        parameter: Option<String>,
        display: Option<String>,
    ) -> Result<Self, String> {
        let index = |id: Option<String>, field| {
            id.map(|id| {
                refs.iter()
                    .position(|member| member == &id)
                    .ok_or_else(|| format!("{field} must be a member of scalar_refs"))
            })
            .transpose()
        };
        Ok(Self {
            parameter: index(parameter, "parameter_scalar_ref")?,
            display: index(display, "display_scalar_ref")?,
            refs,
        })
    }

    pub(crate) fn refs(&self) -> &[String] {
        &self.refs
    }

    pub(crate) fn parameter(&self) -> Option<&str> {
        self.parameter.map(|index| self.refs[index].as_str())
    }

    pub(crate) fn display(&self) -> Option<&str> {
        self.display.map(|index| self.refs[index].as_str())
    }

    pub(crate) fn push(&mut self, id: String) {
        self.refs.push(id);
    }

    pub(crate) fn push_parameter(&mut self, id: String) {
        self.parameter = Some(self.refs.len());
        self.refs.push(id);
    }

    #[cfg(test)]
    pub(crate) fn clear_parameter(&mut self) {
        self.parameter = None;
    }

    #[cfg(test)]
    pub(crate) fn clear_display(&mut self) {
        self.display = None;
    }
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub(super) struct Wire {
    scalar_refs: Vec<String>,
    #[serde(default)]
    parameter_scalar_ref: Option<String>,
    #[serde(default)]
    display_scalar_ref: Option<String>,
}

impl Serialize for RelationScalars {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("scalar_refs", &self.refs)?;
        if let Some(id) = self.parameter() {
            map.serialize_entry("parameter_scalar_ref", id)?;
        }
        if let Some(id) = self.display() {
            map.serialize_entry("display_scalar_ref", id)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for RelationScalars {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = Wire::deserialize(deserializer)?;
        Self::from_refs(
            wire.scalar_refs,
            wire.parameter_scalar_ref,
            wire.display_scalar_ref,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::RelationScalars;

    #[test]
    fn selected_scalars_are_members_on_the_flat_wire() {
        let wire = serde_json::json!({
            "scalar_refs": ["display", "driver"],
            "parameter_scalar_ref": "driver", "display_scalar_ref": "display"
        });
        let scalars: RelationScalars = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(scalars.parameter(), Some("driver"));
        assert_eq!(scalars.display(), Some("display"));
        assert_eq!(serde_json::to_value(scalars).unwrap(), wire);
        for field in ["parameter_scalar_ref", "display_scalar_ref"] {
            let mut invalid = wire.clone();
            invalid[field] = serde_json::json!("missing");
            let error = serde_json::from_value::<RelationScalars>(invalid).unwrap_err();
            assert!(error.to_string().contains(field));
        }
    }
}
