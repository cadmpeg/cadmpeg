// SPDX-License-Identifier: Apache-2.0
//! Native assembly joint payloads and wire admission.

use super::frame::FiniteFrame;
use super::LinkTarget;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Open paired-joint label; `grounded` denotes a different payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedJointFamily(String);

impl PairedJointFamily {
    /// Retain the source label unless it denotes a grounded constraint.
    pub fn new(value: String) -> Result<Self, String> {
        if value.eq_ignore_ascii_case("grounded") {
            return Err("paired joint cannot use the grounded family".into());
        }
        Ok(Self(value))
    }

    /// Source label, including custom enumeration values.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One assembly joint or grounded-object constraint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "JointRecordWire", into = "JointRecordWire")]
pub struct JointRecord {
    /// Stable joint identity.
    pub id: String,
    /// Owning application object.
    pub object: String,
    /// Grounded object or paired connectors.
    pub body: JointBody,
    /// Joint scalar, limit, detach, enable, and suppression properties.
    pub parameters: BTreeMap<String, String>,
}

/// Joint payload discriminated by grounded vs paired connectors.
#[derive(Debug, Clone, PartialEq)]
// Paired connector arrays stay inline and preserve their fixed cardinality without allocation.
#[allow(clippy::large_enum_variant)]
pub enum JointBody {
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
pub struct JointConnectorRecord {
    /// Connector reference with subelement paths.
    pub reference: Option<LinkTarget>,
    /// Connector-local coordinate frame.
    pub placement: FiniteFrame,
    /// Connector attachment-offset frame.
    pub offset: FiniteFrame,
}

impl JointRecord {
    /// Persisted joint family code, or `grounded`.
    pub fn kind(&self) -> &str {
        match &self.body {
            JointBody::Grounded { .. } => "grounded",
            JointBody::Pair { kind, .. } => kind.as_str(),
        }
    }

    /// Ordered connector references.
    pub fn references(&self) -> Vec<&LinkTarget> {
        match &self.body {
            JointBody::Grounded { reference, .. } => reference.iter().collect(),
            JointBody::Pair { connectors, .. } => connectors
                .iter()
                .filter_map(|connector| connector.reference.as_ref())
                .collect(),
        }
    }

    /// Connector-local coordinate frames in connector order.
    pub fn placements(&self) -> Vec<[[f64; 4]; 4]> {
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
    pub fn offsets(&self) -> Vec<[[f64; 4]; 4]> {
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
            parameters: value.parameters,
        }
    }
}

impl TryFrom<JointRecordWire> for JointRecord {
    type Error = String;

    fn try_from(wire: JointRecordWire) -> Result<Self, Self::Error> {
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
            parameters: wire.parameters,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            "references":[{"document":null,"document_attribute":null,"object":"","subelements":[]}, {"document":null,"document_attribute":null,"object":"second","subelements":[]}],
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
        let record = JointRecord {
            id: "joint".into(),
            object: "object".into(),
            body: JointBody::Pair {
                kind: PairedJointFamily::new("CustomCoupling".into()).unwrap(),
                connectors: [connector.clone(), connector],
            },
            parameters: BTreeMap::new(),
        };
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
}
