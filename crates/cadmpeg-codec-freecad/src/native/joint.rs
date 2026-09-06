// SPDX-License-Identifier: Apache-2.0
//! Native assembly joint payloads and wire admission.

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
        reference: LinkTarget,
        /// Connector-local coordinate frame.
        placement: [[f64; 4]; 4],
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
    pub reference: LinkTarget,
    /// Connector-local coordinate frame.
    pub placement: [[f64; 4]; 4],
    /// Connector attachment-offset frame.
    pub offset: [[f64; 4]; 4],
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
            JointBody::Grounded { reference, .. } => vec![reference],
            JointBody::Pair { connectors, .. } => {
                vec![&connectors[0].reference, &connectors[1].reference]
            }
        }
    }

    /// Connector-local coordinate frames in connector order.
    pub fn placements(&self) -> Vec<[[f64; 4]; 4]> {
        match &self.body {
            JointBody::Grounded { placement, .. } => vec![*placement],
            JointBody::Pair { connectors, .. } => {
                vec![connectors[0].placement, connectors[1].placement]
            }
        }
    }

    /// Connector attachment-offset frames in connector order.
    pub fn offsets(&self) -> Vec<[[f64; 4]; 4]> {
        match &self.body {
            JointBody::Grounded { .. } => Vec::new(),
            JointBody::Pair { connectors, .. } => {
                vec![connectors[0].offset, connectors[1].offset]
            }
        }
    }
}

pub(crate) fn empty_link_target() -> LinkTarget {
    LinkTarget {
        document: None,
        object: None,
        subelements: Vec::new(),
    }
}

#[derive(Serialize, Deserialize)]
struct JointRecordWire {
    id: String,
    object: String,
    kind: String,
    references: Vec<LinkTarget>,
    placements: Vec<[[f64; 4]; 4]>,
    offsets: Vec<[[f64; 4]; 4]>,
    parameters: BTreeMap<String, String>,
}

impl From<JointRecord> for JointRecordWire {
    fn from(value: JointRecord) -> Self {
        let kind = value.kind().to_owned();
        let references = value.references().into_iter().cloned().collect();
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
            let reference = match wire.references.len() {
                0 => empty_link_target(),
                1 => wire.references.into_iter().next().expect("one reference"),
                _ => return Err("grounded joint must carry at most one reference".to_owned()),
            };
            JointBody::Grounded {
                reference,
                placement,
            }
        } else {
            let [first_placement, second_placement] = <[_; 2]>::try_from(wire.placements)
                .map_err(|_| "paired joint must carry two placements".to_owned())?;
            let [first_offset, second_offset] = <[_; 2]>::try_from(wire.offsets)
                .map_err(|_| "paired joint must carry two offsets".to_owned())?;
            let mut references = wire.references.into_iter();
            let first_reference = references.next().unwrap_or_else(empty_link_target);
            let second_reference = references.next().unwrap_or_else(empty_link_target);
            if references.next().is_some() {
                return Err("paired joint carries more than two references".to_owned());
            }
            JointBody::Pair {
                kind: PairedJointFamily::new(wire.kind)?,
                connectors: [
                    JointConnectorRecord {
                        reference: first_reference,
                        placement: first_placement,
                        offset: first_offset,
                    },
                    JointConnectorRecord {
                        reference: second_reference,
                        placement: second_placement,
                        offset: second_offset,
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
    fn paired_families_reserve_grounded_and_retain_custom_labels() {
        for name in ["grounded", "Grounded", "GROUNDED"] {
            assert!(PairedJointFamily::new(name.into()).is_err());
        }
        let connector = JointConnectorRecord {
            reference: empty_link_target(),
            placement: crate::product::identity(),
            offset: crate::product::identity(),
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
