// SPDX-License-Identifier: Apache-2.0
//! Native assembly joint payloads and wire admission.

use super::frame::FiniteFrame;
use super::LinkTarget;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Open paired-joint label; `grounded` denotes a different payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PairedJointFamily(String);

impl PairedJointFamily {
    /// Retain the source label unless it denotes a grounded constraint.
    pub(crate) fn new(value: String) -> Result<Self, String> {
        if value.eq_ignore_ascii_case("grounded") {
            return Err("paired joint cannot use the grounded family".into());
        }
        Ok(Self(value))
    }

    /// Source label, including custom enumeration values.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// A checked joint parameter value that retains its source spelling.
#[derive(Debug, Clone, PartialEq)]
enum JointParameter {
    Scalar { raw: String, value: f64 },
    Boolean { raw: String, value: bool },
    Native { raw: String },
}

impl JointParameter {
    fn from_raw(name: &str, raw: String) -> Result<Self, String> {
        match name {
            "Angle" | "AngleMin" | "AngleMax" | "Distance" | "Distance2" | "LengthMin"
            | "LengthMax" => {
                let value = raw
                    .parse::<f64>()
                    .map_err(|_| format!("joint parameter {name} has an invalid value {raw:?}"))?;
                if !value.is_finite() {
                    return Err(format!(
                        "joint parameter {name} has an invalid value {raw:?}"
                    ));
                }
                Ok(Self::Scalar { raw, value })
            }
            "EnableAngleMin" | "EnableAngleMax" | "EnableLengthMin" | "EnableLengthMax"
            | "Detach1" | "Detach2" | "Suppressed" => Ok(Self::Boolean {
                value: raw == "true",
                raw,
            }),
            _ => Ok(Self::Native { raw }),
        }
    }

    fn raw(&self) -> &str {
        match self {
            Self::Scalar { raw, .. } | Self::Boolean { raw, .. } | Self::Native { raw } => raw,
        }
    }
}

/// Checked joint parameters with lossless source text.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct JointParameters(BTreeMap<String, JointParameter>);

impl JointParameters {
    fn from_raw(parameters: BTreeMap<String, String>, joint_id: &str) -> Result<Self, String> {
        parameters
            .into_iter()
            .map(|(name, raw)| {
                let parameter = JointParameter::from_raw(&name, raw)
                    .map_err(|error| format!("joint {joint_id}: {error}"))?;
                Ok((name, parameter))
            })
            .collect::<Result<BTreeMap<_, _>, String>>()
            .map(Self)
    }

    fn into_raw(self) -> BTreeMap<String, String> {
        self.0
            .into_iter()
            .map(|(name, value)| (name, value.raw().to_owned()))
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn raw(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(JointParameter::raw)
    }

    pub(crate) fn bool_value(&self, name: &str) -> Option<bool> {
        match self.0.get(name) {
            Some(JointParameter::Boolean { value, .. }) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn scalar_value(&self, name: &str) -> Option<f64> {
        match self.0.get(name) {
            Some(JointParameter::Scalar { value, .. }) => Some(*value),
            _ => None,
        }
    }
}

/// One assembly joint or grounded-object constraint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "JointRecordWire", into = "JointRecordWire")]
pub(crate) struct JointRecord {
    /// Stable joint identity.
    pub(crate) id: String,
    /// Owning application object.
    pub(crate) object: String,
    /// Grounded object or paired connectors.
    pub(crate) body: JointBody,
    /// Joint scalar, limit, detach, enable, and suppression properties.
    parameters: JointParameters,
}

/// Joint payload discriminated by grounded vs paired connectors.
#[derive(Debug, Clone, PartialEq)]
// Paired connector arrays stay inline and preserve their fixed cardinality without allocation.
#[allow(clippy::large_enum_variant)]
pub(crate) enum JointBody {
    /// Object-to-ground constraint.
    Grounded {
        /// Grounded object reference.
        reference: Option<LinkTarget>,
        /// Connector-local coordinate frame.
        placement: FiniteFrame,
    },
    /// Two-connector joint.
    Pair {
        /// Persisted joint family code.
        kind: PairedJointFamily,
        /// Ordered connectors.
        connectors: [JointConnectorRecord; 2],
    },
}

/// One paired-joint connector.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JointConnectorRecord {
    /// Connector reference with subelement paths.
    pub(crate) reference: Option<LinkTarget>,
    /// Connector-local coordinate frame.
    pub(crate) placement: FiniteFrame,
    /// Connector attachment-offset frame.
    pub(crate) offset: FiniteFrame,
}

impl JointRecord {
    pub(crate) fn try_new(
        id: String,
        object: String,
        body: JointBody,
        parameters: BTreeMap<String, String>,
    ) -> Result<Self, String> {
        let parameters = JointParameters::from_raw(parameters, &id)?;
        Ok(Self {
            id,
            object,
            body,
            parameters,
        })
    }

    pub(crate) fn parameters(&self) -> &JointParameters {
        &self.parameters
    }

    /// Persisted joint family code, or `grounded`.
    pub(crate) fn kind(&self) -> &str {
        match &self.body {
            JointBody::Grounded { .. } => "grounded",
            JointBody::Pair { kind, .. } => kind.as_str(),
        }
    }

    /// Ordered connector references.
    pub(crate) fn references(&self) -> Vec<&LinkTarget> {
        match &self.body {
            JointBody::Grounded { reference, .. } => reference.iter().collect(),
            JointBody::Pair { connectors, .. } => connectors
                .iter()
                .filter_map(|connector| connector.reference.as_ref())
                .collect(),
        }
    }

    /// Connector-local coordinate frames in connector order.
    pub(crate) fn placements(&self) -> Vec<[[f64; 4]; 4]> {
        match &self.body {
            JointBody::Grounded { placement, .. } => vec![placement.rows()],
            JointBody::Pair { connectors, .. } => {
                vec![
                    connectors[0].placement.rows(),
                    connectors[1].placement.rows(),
                ]
            }
        }
    }

    /// Connector attachment-offset frames in connector order.
    fn offsets(&self) -> Vec<[[f64; 4]; 4]> {
        match &self.body {
            JointBody::Grounded { .. } => Vec::new(),
            JointBody::Pair { connectors, .. } => {
                vec![connectors[0].offset.rows(), connectors[1].offset.rows()]
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
struct JointRecordWire {
    id: String,
    object: String,
    kind: String,
    references: Vec<Option<LinkTarget>>,
    placements: Vec<[[f64; 4]; 4]>,
    offsets: Vec<[[f64; 4]; 4]>,
    parameters: BTreeMap<String, String>,
}

/// Validate a joint parameter through the checked source-value carrier.
///
/// Unknown parameter names remain native extension data. Known scalar names
/// carry finite floating-point values. `FreeCAD`'s `PropertyBool` reader stores
/// true only for exact lowercase `true` and stores false for every other raw
/// spelling, so the raw text is retained alongside that typed value.
pub(crate) fn validate_parameter_value(name: &str, value: &str) -> Result<(), String> {
    JointParameter::from_raw(name, value.to_owned()).map(|_| ())
}

impl From<JointRecord> for JointRecordWire {
    fn from(value: JointRecord) -> Self {
        let kind = value.kind().to_owned();
        let references = match &value.body {
            JointBody::Grounded { reference, .. } => vec![reference.clone()],
            JointBody::Pair { connectors, .. } => connectors
                .iter()
                .map(|connector| connector.reference.clone())
                .collect(),
        };
        let placements = value.placements();
        let offsets = value.offsets();
        Self {
            id: value.id,
            object: value.object,
            kind,
            references,
            placements,
            offsets,
            parameters: value.parameters.into_raw(),
        }
    }
}

impl TryFrom<JointRecordWire> for JointRecord {
    type Error = String;

    fn try_from(wire: JointRecordWire) -> Result<Self, Self::Error> {
        let parameters = JointParameters::from_raw(wire.parameters, &wire.id)?;
        let body = if wire.kind == "grounded" {
            let [placement] = <[_; 1]>::try_from(wire.placements)
                .map_err(|_| "grounded joint must carry exactly one placement".to_owned())?;
            if !wire.offsets.is_empty() {
                return Err("grounded joint cannot carry offsets".to_owned());
            }
            let mut references = wire.references.into_iter();
            let reference = references.next().flatten();
            if references.next().is_some() {
                return Err("grounded joint must carry at most one reference".to_owned());
            }
            JointBody::Grounded {
                reference,
                placement: placement
                    .try_into()
                    .map_err(|error| format!("placements: {error}"))?,
            }
        } else {
            let [first_placement, second_placement] = <[_; 2]>::try_from(wire.placements)
                .map_err(|_| "paired joint must carry two placements".to_owned())?;
            let [first_offset, second_offset] = <[_; 2]>::try_from(wire.offsets)
                .map_err(|_| "paired joint must carry two offsets".to_owned())?;
            let mut references = wire.references.into_iter();
            let first_reference = references.next().flatten();
            let second_reference = references.next().flatten();
            if references.next().is_some() {
                return Err("paired joint carries more than two references".to_owned());
            }
            JointBody::Pair {
                kind: PairedJointFamily::new(wire.kind)?,
                connectors: [
                    JointConnectorRecord {
                        reference: first_reference,
                        placement: first_placement
                            .try_into()
                            .map_err(|error| format!("placements: {error}"))?,
                        offset: first_offset
                            .try_into()
                            .map_err(|error| format!("offsets: {error}"))?,
                    },
                    JointConnectorRecord {
                        reference: second_reference,
                        placement: second_placement
                            .try_into()
                            .map_err(|error| format!("placements: {error}"))?,
                        offset: second_offset
                            .try_into()
                            .map_err(|error| format!("offsets: {error}"))?,
                    },
                ],
            }
        };
        Ok(Self {
            id: wire.id,
            object: wire.object,
            body,
            parameters,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{JointBody, JointConnectorRecord, JointRecord, JointRecordWire, PairedJointFamily};

    #[test]
    fn wire_admission_rejects_nonfinite_connector_frames() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for kind in ["grounded", "Fixed"] {
                for offset in [false, true] {
                    if kind == "grounded" && offset {
                        continue;
                    }
                    let mut wire = JointRecordWire {
                        id: "joint".into(),
                        object: "object".into(),
                        kind: kind.into(),
                        references: vec![],
                        placements: vec![
                            cadmpeg_ir::transform::Transform::identity().rows();
                            if kind == "grounded" { 1 } else { 2 }
                        ],
                        offsets: if kind == "grounded" {
                            vec![]
                        } else {
                            vec![cadmpeg_ir::transform::Transform::identity().rows(); 2]
                        },
                        parameters: BTreeMap::new(),
                    };
                    if offset {
                        wire.offsets[0][0][3] = bad;
                    } else {
                        wire.placements[0][0][3] = bad;
                    }
                    assert!(JointRecord::try_from(wire)
                        .unwrap_err()
                        .contains(if offset { "offsets" } else { "placements" }));
                }
            }
        }
    }

    #[test]
    fn missing_first_reference_keeps_second_wire_position() {
        let identity = cadmpeg_ir::transform::Transform::identity().rows();
        let wire = serde_json::json!({"id":"joint", "object":"object", "kind":"Fixed",
            "references":[null, {"document":null,"document_attribute":null,"object":"second","subelements":[]}],
            "placements":[identity,identity], "offsets":[identity,identity], "parameters":{}});
        let record = serde_json::from_value::<JointRecord>(wire.clone()).unwrap();
        let JointBody::Pair { connectors, .. } = &record.body else {
            panic!("paired joint")
        };
        assert!(connectors[0].reference.is_none());
        assert_eq!(
            connectors[1].reference.as_ref().unwrap().object(),
            Some("second")
        );
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }

    #[test]
    fn paired_families_reserve_grounded_and_retain_custom_labels() {
        for name in ["grounded", "Grounded", "GROUNDED"] {
            assert!(PairedJointFamily::new(name.into()).is_err());
        }
        let connector = JointConnectorRecord {
            reference: None,
            placement: cadmpeg_ir::transform::Transform::identity()
                .rows()
                .try_into()
                .unwrap(),
            offset: cadmpeg_ir::transform::Transform::identity()
                .rows()
                .try_into()
                .unwrap(),
        };
        let record = JointRecord::try_new(
            "joint".into(),
            "object".into(),
            JointBody::Pair {
                kind: PairedJointFamily::new("CustomCoupling".into()).unwrap(),
                connectors: [connector.clone(), connector],
            },
            BTreeMap::new(),
        )
        .unwrap();
        let wire = serde_json::to_value(&record).unwrap();
        assert_eq!(wire["kind"], "CustomCoupling");
        assert_eq!(
            serde_json::from_value::<JointRecord>(wire.clone()).unwrap(),
            record
        );
        for name in ["grounded", "Grounded"] {
            let mut invalid = wire.clone();
            invalid["kind"] = serde_json::json!(name);
            assert!(serde_json::from_value::<JointRecord>(invalid).is_err());
        }
    }

    #[test]
    fn wire_admission_rejects_invalid_known_parameter_values() {
        let identity = cadmpeg_ir::transform::Transform::identity().rows();
        for (name, value) in [("Angle", "abc"), ("Angle", "NaN")] {
            let parameters =
                serde_json::Map::from_iter([(name.to_owned(), serde_json::json!(value))]);
            let wire = serde_json::json!({
                "id": "joint",
                "object": "object",
                "kind": "Fixed",
                "references": [null, null],
                "placements": [identity, identity],
                "offsets": [identity, identity],
                "parameters": parameters
            });
            let error = serde_json::from_value::<JointRecord>(wire).unwrap_err();
            assert!(error.to_string().contains("invalid value"), "{error}");
        }

        for (name, value) in [("Angle", "15.5"), ("Suppressed", "false")] {
            let parameters =
                serde_json::Map::from_iter([(name.to_owned(), serde_json::json!(value))]);
            let wire = serde_json::json!({
                "id": "joint",
                "object": "object",
                "kind": "Fixed",
                "references": [null, null],
                "placements": [identity, identity],
                "offsets": [identity, identity],
                "parameters": parameters
            });
            let record = serde_json::from_value::<JointRecord>(wire.clone())
                .expect("valid known parameter values remain admissible");
            if name == "Angle" {
                assert_eq!(record.parameters().scalar_value(name), Some(15.5));
            }
            assert_eq!(serde_json::to_value(record).unwrap(), wire);
        }
    }

    #[test]
    fn bool_parameters_follow_primary_restore_and_retain_raw_text() {
        let identity = cadmpeg_ir::transform::Transform::identity().rows();
        for (raw, expected) in [
            ("true", true),
            ("false", false),
            ("1", false),
            ("0", false),
            ("TRUE", false),
            ("maybe", false),
        ] {
            let wire = serde_json::json!({
                "id": "joint",
                "object": "object",
                "kind": "Fixed",
                "references": [null, null],
                "placements": [identity, identity],
                "offsets": [identity, identity],
                "parameters": {"Suppressed": raw}
            });
            let record = serde_json::from_value::<JointRecord>(wire.clone())
                .expect("primary bool spellings remain admissible");
            assert_eq!(record.parameters().raw("Suppressed"), Some(raw));
            assert_eq!(record.parameters().bool_value("Suppressed"), Some(expected));
            assert_eq!(serde_json::to_value(record).unwrap(), wire);
        }
    }

    #[test]
    fn unknown_parameter_names_retain_their_wire_text() {
        let identity = cadmpeg_ir::transform::Transform::identity().rows();
        let wire = serde_json::json!({
            "id": "joint",
            "object": "object",
            "kind": "Fixed",
            "references": [null, null],
            "placements": [identity, identity],
            "offsets": [identity, identity],
            "parameters": {"FutureJointSetting": "vendor spelling"}
        });
        let record = serde_json::from_value::<JointRecord>(wire.clone()).unwrap();
        assert_eq!(
            record.parameters().raw("FutureJointSetting"),
            Some("vendor spelling")
        );
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }
}
