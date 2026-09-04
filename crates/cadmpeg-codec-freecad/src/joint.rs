// SPDX-License-Identifier: Apache-2.0
//! Assembly joints recovered without executing Python proxy payloads.

use std::collections::{BTreeMap, HashMap};

use crate::native::{JointRecord, ObjectRecord, PropertyRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::products::{
    AssemblyJoint, JointConnector, JointId, JointLimits, JointOperand, Occurrence, PairedJointKind,
};
use cadmpeg_ir::transform::Transform;

pub(crate) fn transfer(
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Result<Vec<JointRecord>, CodecError> {
    let by_owner = properties.iter().fold(
        HashMap::<&str, Vec<&PropertyRecord>>::new(),
        |mut map, property| {
            map.entry(&property.owner).or_default().push(property);
            map
        },
    );
    let mut output = Vec::new();
    for object in objects {
        let owned = by_owner
            .get(object.id.as_str())
            .cloned()
            .unwrap_or_default();
        let grounded_property = unique_property(&owned, "ObjectToGround")?;
        let joint_type_property = unique_property(&owned, "JointType")?;
        if grounded_property.is_some() && joint_type_property.is_some() {
            return Err(CodecError::malformed(format_args!(
                "joint object {} carries both ObjectToGround and JointType",
                object.id
            )));
        }
        if let Some(property) = grounded_property {
            let legacy_empty_sub = property.type_name == "App::PropertyLinkSub"
                && property.links.len() == 1
                && property.links[0].subelements.iter().all(String::is_empty);
            if !matches!(
                property.type_name.as_str(),
                "App::PropertyLinkGlobal" | "App::PropertyLink"
            ) && !legacy_empty_sub
            {
                return Err(CodecError::malformed(format_args!(
                    "joint property {} has the wrong runtime type for ObjectToGround",
                    property.id
                )));
            }
        }
        if let Some(property) = joint_type_property {
            if property.type_name != "App::PropertyEnumeration" {
                return Err(CodecError::malformed(format_args!(
                    "joint property {} has the wrong runtime type for JointType",
                    property.id
                )));
            }
        }
        let grounded = grounded_property.is_some();
        let joint_type = joint_type_property.map(enumeration_value).transpose()?;
        if !grounded && joint_type.is_none() {
            continue;
        }
        let (references, placements, offsets) = if grounded {
            let placement = placement(&owned, "Placement")?;
            (
                links(&owned, "ObjectToGround"),
                placement.into_iter().collect(),
                Vec::new(),
            )
        } else {
            let placement1 = placement(&owned, "Placement1")?;
            let offset1 = placement(&owned, "Offset1")?;
            let placement2 = placement(&owned, "Placement2")?;
            let offset2 = placement(&owned, "Offset2")?;
            let reference1 = connector(&owned, "Reference1")?;
            let reference2 = connector(&owned, "Reference2")?;
            let slots = [
                (reference1, placement1, offset1),
                (reference2, placement2, offset2),
            ];
            let references = slots
                .iter()
                .flat_map(|(references, _, _)| references.iter().cloned())
                .collect();
            let placements = slots
                .iter()
                .map(|(_, placement, _)| placement.unwrap_or_else(crate::product::identity))
                .collect();
            let offsets = slots
                .iter()
                .map(|(_, _, offset)| offset.unwrap_or_else(crate::product::identity))
                .collect();
            (references, placements, offsets)
        };
        let parameters = owned
            .iter()
            .filter(|property| {
                matches!(
                    property.name.as_str(),
                    "Angle"
                        | "AngleMin"
                        | "AngleMax"
                        | "Distance"
                        | "Distance2"
                        | "LengthMin"
                        | "LengthMax"
                        | "EnableAngleMin"
                        | "EnableAngleMax"
                        | "EnableLengthMin"
                        | "EnableLengthMax"
                        | "Detach1"
                        | "Detach2"
                        | "Suppressed"
                )
            })
            .map(|property| {
                scalar_parameter(property)
                    .map(|value| value.map(|value| (property.name.clone(), value)))
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<BTreeMap<_, _>>();
        output.push(JointRecord {
            id: crate::native::native_id("joint", &object.name),
            object: object.id.clone(),
            kind: if grounded {
                "grounded".into()
            } else {
                joint_type.unwrap_or_else(|| "unknown".into())
            },
            references,
            placements,
            offsets,
            parameters,
        });
    }
    Ok(output)
}

pub(crate) fn transfer_neutral(
    records: &[JointRecord],
    occurrences: &[Occurrence],
) -> Vec<AssemblyJoint> {
    let occurrence_by_native = occurrences
        .iter()
        .filter_map(|occurrence| {
            let native = occurrence.native_ref.as_deref()?;
            Some((native, &occurrence.id))
        })
        .collect::<HashMap<_, _>>();
    records
        .iter()
        .filter_map(|record| {
            let bool_value = |name: &str| {
                record
                    .parameters
                    .get(name)
                    .and_then(|value| parse_bool(value))
            };
            let scalar = |name: &str| {
                record
                    .parameters
                    .get(name)
                    .and_then(|value| value.parse().ok())
            };
            let enabled_limits =
                |minimum: &str, maximum: &str, enable_min: &str, enable_max: &str, scale: f64| {
                    let minimum = bool_value(enable_min)
                        .unwrap_or(false)
                        .then(|| scalar(minimum))
                        .flatten()
                        .map(|value: f64| value * scale);
                    let maximum = bool_value(enable_max)
                        .unwrap_or(false)
                        .then(|| scalar(maximum))
                        .flatten()
                        .map(|value: f64| value * scale);
                    JointLimits::new(minimum, maximum)
                };
            let operands = record
                .references
                .iter()
                .map(|reference| {
                    let object = reference.object.clone()?;
                    let subelements = reference
                        .subelements
                        .iter()
                        .filter(|subelement| !subelement.is_empty())
                        .cloned()
                        .collect();
                    if let Some(document) = reference.document.as_deref() {
                        return Some(JointOperand::external(
                            crate::product::external_document_reference(
                                document,
                                reference.document_attribute.as_deref(),
                            ),
                            object,
                            subelements,
                        ));
                    }
                    Some(
                        match occurrence_by_native.get(object.as_str()).copied().cloned() {
                            Some(occurrence) => {
                                JointOperand::occurrence(occurrence, object, subelements)
                            }
                            None => JointOperand::root(object, subelements),
                        },
                    )
                })
                .collect::<Option<Vec<_>>>()?;
            let frames = record
                .placements
                .iter()
                .copied()
                .map(|rows| Transform::from_rows(rows).expect("affine transform"))
                .collect::<Vec<_>>();
            let offsets = record
                .offsets
                .iter()
                .copied()
                .map(|rows| Transform::from_rows(rows).expect("affine transform"))
                .collect::<Vec<_>>();
            let id = JointId(crate::native::model_id(
                "joint",
                &record.object,
                "constraint",
            ));
            let kind = joint_kind(&record.kind);
            let angle = scalar("Angle").map(f64::to_radians);
            let distance = scalar("Distance");
            let distance2 = scalar("Distance2");
            let angular_limits = enabled_limits(
                "AngleMin",
                "AngleMax",
                "EnableAngleMin",
                "EnableAngleMax",
                std::f64::consts::PI / 180.0,
            );
            let linear_limits = enabled_limits(
                "LengthMin",
                "LengthMax",
                "EnableLengthMin",
                "EnableLengthMax",
                1.0,
            );
            let mut joint = match kind {
                None => {
                    let [operand] = operands.try_into().ok()?;
                    let [frame] = frames.try_into().ok()?;
                    let offset_frame = match offsets.as_slice() {
                        [] => None,
                        [offset] => Some(*offset),
                        _ => return None,
                    };
                    AssemblyJoint::grounded(
                        id,
                        JointConnector {
                            operand,
                            frame,
                            detached: bool_value("Detach1").unwrap_or(false),
                        },
                        offset_frame,
                    )
                }
                Some(kind) => {
                    let kind = kind.with_scalars(
                        angle,
                        None,
                        distance,
                        distance2,
                        angular_limits,
                        linear_limits,
                    );
                    let [first_operand, second_operand] = operands.try_into().ok()?;
                    let [first_frame, second_frame] = frames.try_into().ok()?;
                    let offset_frames = if offsets.is_empty() {
                        None
                    } else {
                        Some(<Vec<Transform> as TryInto<[Transform; 2]>>::try_into(offsets).ok()?)
                    };
                    AssemblyJoint::paired(
                        id,
                        kind,
                        [
                            JointConnector {
                                operand: first_operand,
                                frame: first_frame,
                                detached: bool_value("Detach1").unwrap_or(false),
                            },
                            JointConnector {
                                operand: second_operand,
                                frame: second_frame,
                                detached: bool_value("Detach2").unwrap_or(false),
                            },
                        ],
                        offset_frames,
                    )
                }
            };
            joint.suppressed = bool_value("Suppressed").unwrap_or(false);
            joint.native_ref = Some(record.id.clone());
            Some(joint)
        })
        .collect()
}

fn joint_kind(kind: &str) -> Option<PairedJointKind> {
    Some(match kind.to_ascii_lowercase().as_str() {
        "fixed" => PairedJointKind::Fixed {
            angle: None,
            translation_offset: None,
            angular_limits: None,
            linear_limits: None,
        },
        "revolute" => PairedJointKind::Revolute {
            angle: None,
            angular_limits: None,
        },
        "slider" | "prismatic" => PairedJointKind::Slider {
            distance: None,
            translation_offset: None,
            linear_limits: None,
        },
        "cylindrical" => PairedJointKind::Cylindrical {
            angle: None,
            distance: None,
            angular_limits: None,
            linear_limits: None,
        },
        "ball" | "spherical" => PairedJointKind::Ball,
        "distance" => PairedJointKind::Distance { distance: None },
        "parallel" => PairedJointKind::Parallel,
        "perpendicular" => PairedJointKind::Perpendicular,
        "angle" => PairedJointKind::Angle { angle: None },
        "rackpinion" | "rack_pinion" => PairedJointKind::RackPinion {
            distance: None,
            distance2: None,
        },
        "screw" => PairedJointKind::Screw { distance: None },
        "gears" => PairedJointKind::Gears {
            distance: None,
            distance2: None,
        },
        "belt" => PairedJointKind::Belt {
            distance: None,
            distance2: None,
        },
        "grounded" => return None,
        other => PairedJointKind::Native {
            name: other.to_owned(),
            angle: None,
            translation_offset: None,
            distance: None,
            distance2: None,
            angular_limits: None,
            linear_limits: None,
        },
    })
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

fn enumeration_value(property: &PropertyRecord) -> Result<String, CodecError> {
    let document = roxmltree::Document::parse(&property.raw_xml).map_err(|error| {
        malformed(format!(
            "joint enumeration property {} has invalid XML: {error}",
            property.id
        ))
    })?;
    let root = document.root_element();
    if !root.has_tag_name("Property") {
        return Err(malformed(format!(
            "joint enumeration property {} has no Property root",
            property.id
        )));
    }
    let values = root
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let Some(integer) = values
        .first()
        .copied()
        .filter(|value| value.has_tag_name("Integer"))
    else {
        return Err(malformed(format!(
            "joint enumeration property {} requires one direct Integer value",
            property.id
        )));
    };
    if integer.children().any(|value| value.is_element()) {
        return Err(malformed(format!(
            "joint enumeration property {} has nested Integer values",
            property.id
        )));
    }
    if values.len() > 2
        || values
            .get(1)
            .is_some_and(|value| !value.has_tag_name("CustomEnumList"))
    {
        return Err(malformed(format!(
            "joint enumeration property {} has extra direct value roots",
            property.id
        )));
    }
    let custom_list = values.get(1).copied();
    let custom = match integer.attribute("CustomEnum") {
        None => false,
        Some("true") => true,
        Some(_) => {
            return Err(malformed(format!(
                "joint enumeration property {} has an invalid CustomEnum marker",
                property.id
            )));
        }
    };
    if custom != custom_list.is_some() {
        return Err(malformed(format!(
            "joint enumeration property {} has inconsistent custom enumeration carriers",
            property.id
        )));
    }
    let index = integer
        .attribute("value")
        .ok_or_else(|| {
            malformed(format!(
                "joint enumeration property {} has no Integer value",
                property.id
            ))
        })?
        .parse::<usize>()
        .map_err(|_| {
            malformed(format!(
                "joint enumeration property {} has an invalid Integer value",
                property.id
            ))
        })?;
    let enum_values = if let Some(custom_list) = custom_list {
        let count = custom_list
            .attribute("count")
            .ok_or_else(|| {
                malformed(format!(
                    "joint enumeration property {} CustomEnumList has no count",
                    property.id
                ))
            })?
            .parse::<usize>()
            .map_err(|_| {
                malformed(format!(
                    "joint enumeration property {} CustomEnumList count is invalid",
                    property.id
                ))
            })?;
        let values = custom_list
            .children()
            .filter(roxmltree::Node::is_element)
            .collect::<Vec<_>>();
        if values.len() != count || values.iter().any(|value| !value.has_tag_name("Enum")) {
            return Err(malformed(format!(
                "joint enumeration property {} CustomEnumList count={count} but {} direct Enum values were found",
                property.id,
                values.len()
            )));
        }
        values
            .into_iter()
            .map(|value| {
                if value.children().any(|child| child.is_element()) {
                    return Err(malformed(format!(
                        "joint enumeration property {} has nested Enum values",
                        property.id
                    )));
                }
                value.attribute("value").map(str::to_owned).ok_or_else(|| {
                    malformed(format!(
                        "joint enumeration property {} Enum has no value",
                        property.id
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };
    Ok(enum_values
        .get(index)
        .cloned()
        .unwrap_or_else(|| index.to_string()))
}

fn scalar_parameter(property: &PropertyRecord) -> Result<Option<String>, CodecError> {
    let (expected_type, expected_tag) = match property.name.as_str() {
        "Angle" | "AngleMin" | "AngleMax" => ("App::PropertyAngle", "Float"),
        "Distance" | "Distance2" | "LengthMin" | "LengthMax" => ("App::PropertyLength", "Float"),
        "EnableAngleMin" | "EnableAngleMax" | "EnableLengthMin" | "EnableLengthMax" | "Detach1"
        | "Detach2" | "Suppressed" => ("App::PropertyBool", "Bool"),
        _ => return Ok(None),
    };
    if property.type_name != expected_type {
        return Err(malformed(format!(
            "joint parameter property {} has runtime type {}, expected {expected_type}",
            property.id, property.type_name
        )));
    }
    let [value] = property.values.as_slice() else {
        return Err(malformed(format!(
            "joint parameter property {} requires one {expected_tag} value",
            property.id
        )));
    };
    if value.tag != expected_tag {
        return Err(malformed(format!(
            "joint parameter property {} requires a {expected_tag} value",
            property.id
        )));
    }
    value
        .attributes
        .get("value")
        .cloned()
        .ok_or_else(|| {
            malformed(format!(
                "joint parameter property {} has no value",
                property.id
            ))
        })
        .map(Some)
}

fn unique_property<'a>(
    properties: &[&'a PropertyRecord],
    name: &str,
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    let mut matches = properties
        .iter()
        .copied()
        .filter(|property| property.name == name);
    let Some(property) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(CodecError::malformed(format_args!(
            "joint property {name} occurs more than once"
        )));
    }
    Ok(Some(property))
}

fn links(properties: &[&PropertyRecord], name: &str) -> Vec<crate::native::LinkTarget> {
    properties
        .iter()
        .find(|property| property.name == name)
        .map(|property| {
            property
                .links
                .iter()
                .filter(|link| {
                    link.document.is_some()
                        || link
                            .object
                            .as_deref()
                            .is_some_and(|object| !object.is_empty())
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

fn connector(
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Vec<crate::native::LinkTarget>, CodecError> {
    let Some(property) = unique_property(properties, name)? else {
        return Err(malformed(format!("joint connector {name} is missing")));
    };
    if !matches!(
        property.type_name.as_str(),
        "App::PropertyXLinkSub" | "App::PropertyXLinkSubHidden"
    ) {
        return Err(malformed(format!(
            "joint connector {} has runtime type {}, expected App::PropertyXLinkSub",
            property.id, property.type_name
        )));
    }
    if property
        .values
        .first()
        .is_none_or(|value| value.tag != "XLink")
    {
        return Err(malformed(format!(
            "joint connector {} requires one XLink value",
            property.id
        )));
    }
    if property.links.len() != 1 {
        return Err(malformed(format!(
            "joint connector {} requires one target, found {}",
            property.id,
            property.links.len()
        )));
    }
    Ok(property.links.clone())
}

fn placement(
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<[[f64; 4]; 4]>, CodecError> {
    let Some(property) = unique_property(properties, name)? else {
        return Ok(None);
    };
    crate::product::placement_matrix(property)
}

fn malformed(message: impl Into<String>) -> CodecError {
    CodecError::Malformed(message.into())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::joint_kind;
    use crate::test_support::*;
    use crate::FcstdCodec;
    use cadmpeg_ir::products::PairedJointKind;
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;

    const EPS_JOINT_SCALAR: f64 = 1.0e-12;

    #[test]
    fn every_primary_joint_family_has_a_neutral_variant() {
        for family in [
            "Fixed",
            "Revolute",
            "Cylindrical",
            "Slider",
            "Ball",
            "Distance",
            "Parallel",
            "Perpendicular",
            "Angle",
            "RackPinion",
            "Screw",
            "Gears",
            "Belt",
            "grounded",
        ] {
            assert!(
                !matches!(joint_kind(family), Some(PairedJointKind::Native { .. })),
                "{family} must not fall through to a native joint family"
            );
        }
    }

    #[test]
    pub(crate) fn recovers_assembly_joint_operands_frames_and_state() {
        let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Assembly::AssemblyObject" name="Assembly" id="1"/>
 <Object type="App::FeaturePython" name="Joint" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Assembly"><Properties Count="0"/></Object>
 <Object name="Joint"><Properties Count="14">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="1" CustomEnum="true"/><CustomEnumList count="2"><Enum value="Fixed"/><Enum value="Revolute"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSubHidden"><XLink file="" name="Assembly" count="2"><Sub value="A.Face1"/><Sub value="A.Edge2"/></XLink></Property>
  <Property name="Reference2" type="App::PropertyXLinkSubHidden"><XLink file="" name="Assembly" count="1"><Sub value="B.Edge3"/></XLink></Property>
  <Property name="Placement1" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Placement2" type="App::PropertyPlacement"><PropertyPlacement Px="2" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Suppressed" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="15"/></Property>
  <Property name="AngleMin" type="App::PropertyAngle"><Float value="-30"/></Property>
  <Property name="AngleMax" type="App::PropertyAngle"><Float value="45"/></Property>
  <Property name="EnableAngleMin" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="EnableAngleMax" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Detach1" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Offset1" type="App::PropertyPlacement"><PropertyPlacement Px="0.5" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Offset2" type="App::PropertyPlacement"><PropertyPlacement Px="1.5" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect("joint");
        let joints = result
            .ir()
            .native
            .namespace("fcstd")
            .expect("native")
            .arena_as::<crate::native::JointRecord>("joints")
            .expect("joints");
        assert_eq!(joints.len(), 1);
        assert_eq!(joints[0].kind, "Revolute");
        assert_eq!(joints[0].references.len(), 2);
        assert_eq!(
            joints[0].references[0].object.as_deref(),
            Some("fcstd:native:object#Assembly")
        );
        assert_eq!(joints[0].references[0].subelements, ["A.Face1", "A.Edge2"]);
        assert_eq!(joints[0].placements[1][0][3], 2.0);
        assert_eq!(
            joints[0].parameters.get("Suppressed").map(String::as_str),
            Some("true")
        );
        assert_eq!(result.ir().model.assembly_joints.len(), 1);
        let joint = &result.ir().model.assembly_joints[0];
        assert!(matches!(
            joint.paired_kind(),
            Some(PairedJointKind::Revolute { .. })
        ));
        let connectors = joint.connectors().collect::<Vec<_>>();
        assert_eq!(connectors.len(), 2);
        assert!(connectors.iter().all(|connector| matches!(
            connector.operand.container,
            cadmpeg_ir::OperandContainer::Occurrence(_)
        )));
        assert_eq!(connectors[1].frame.rows()[0][3], 2.0);
        let offset_frames = joint.offset_frames().collect::<Vec<_>>();
        assert_eq!(offset_frames.len(), 2);
        assert_eq!(offset_frames[0].rows()[0][3], 0.5);
        assert_eq!(offset_frames[1].rows()[0][3], 1.5);
        assert!(joint.suppressed);
        assert_eq!(joint.detached(), [true, false]);
        assert!((joint.angle().expect("angle") - 15_f64.to_radians()).abs() < EPS_JOINT_SCALAR);
        let limits = joint.angular_limits().expect("angular limits");
        assert!(
            (limits.minimum().expect("minimum") - (-30_f64).to_radians()).abs() < EPS_JOINT_SCALAR
        );
        assert!(
            (limits.maximum().expect("maximum") - 45_f64.to_radians()).abs() < EPS_JOINT_SCALAR
        );
        assert!(crate::validate_native(result.ir()).is_empty());
        assert_valid_document(result.ir());
        let mut corrupted = result.ir().clone();
        corrupted.model.assembly_joints[0].set_angular_limits(Some(
            cadmpeg_ir::JointLimits::Both {
                minimum: 2.0,
                maximum: 1.0,
            },
        ));
        assert!(cadmpeg_ir::validate_neutral(&corrupted, Vec::new())
            .findings
            .iter()
            .any(|finding| finding.message.contains("invalid assembly joint")));
        let mut wire = serde_json::to_value(&result.ir().model.assembly_joints[0])
            .expect("assembly joint wire");
        wire["operands"][0]["external_document"] = serde_json::json!({
            "path": "external.FCStd",
            "resolution": "unresolved"
        });
        assert!(serde_json::from_value::<cadmpeg_ir::AssemblyJoint>(wire).is_err());
    }

    #[test]
    fn transfers_grounded_assembly_state_with_resolved_component() {
        let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Part::Feature" name="BasePlate" id="1"/>
 <Object type="App::FeaturePython" name="Ground" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="BasePlate"><Properties Count="0"/></Object>
 <Object name="Ground"><Properties Count="2">
  <Property name="ObjectToGround" type="App::PropertyLinkGlobal"><Link value="BasePlate"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="7" Py="8" Pz="9" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect("grounded assembly object");
        assert_eq!(result.ir().model.assembly_joints.len(), 1);
        let joint = &result.ir().model.assembly_joints[0];
        assert!(joint.is_grounded());
        let connectors = joint.connectors().collect::<Vec<_>>();
        assert_eq!(connectors.len(), 1);
        assert!(matches!(
            connectors[0].operand.container,
            cadmpeg_ir::OperandContainer::Occurrence(_)
        ));
        assert_eq!(connectors[0].frame.rows()[0][3], 7.0);
        assert_eq!(connectors[0].frame.rows()[1][3], 8.0);
        assert_eq!(connectors[0].frame.rows()[2][3], 9.0);
        assert!(crate::validate_native(result.ir()).is_empty());
        assert_valid_document(result.ir());
    }

    #[test]
    fn rejects_wrong_runtime_types_for_joint_carriers() {
        for property in [
            r#"<Property name="ObjectToGround" type="App::PropertyString"><String value="Base"/></Property>"#,
            r#"<Property name="JointType" type="App::PropertyInteger"><Integer value="0"/></Property>"#,
        ] {
            let document = format!(
                r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Base"/><Object type="App::FeaturePython" name="Joint"/></Objects>
<ObjectData Count="2"><Object name="Base"><Properties Count="0"/></Object><Object name="Joint"><Properties Count="1">{property}</Properties></Object></ObjectData>
</Document>"#
            );
            assert!(matches!(
                FcstdCodec.decode(
                    &mut Cursor::new(archive(&document)),
                    &DecodeOptions::default(),
                ),
                Err(cadmpeg_ir::DecodeFailure::Codec(
                    cadmpeg_core::CodecError::Malformed(_)
                ))
            ));
        }
    }

    #[test]
    fn rejects_wrong_connector_type_and_target_cardinality() {
        for properties in [
            r#"<Property name="Reference1" type="App::PropertyLinkSub"><LinkSub value="Base" count="0"/></Property>
<Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Base"/></Property>"#,
            r#"<Property name="Reference1" type="App::PropertyXLinkSubList"><XLinkSubList count="2"><XLink file="" name="Base"/><XLink file="" name="Other"/></XLinkSubList></Property>
<Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Base"/></Property>"#,
            r#"<Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Base"/></Property>"#,
        ] {
            let property_count = properties.lines().count() + 1;
            let document = format!(
                r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::FeaturePython" name="Joint"/></Objects>
<ObjectData Count="1"><Object name="Joint"><Properties Count="{property_count}">
<Property name="JointType" type="App::PropertyEnumeration"><Integer value="0"/></Property>
{properties}
</Properties></Object></ObjectData></Document>"#
            );
            assert!(matches!(
                FcstdCodec.decode(
                    &mut Cursor::new(archive(&document)),
                    &DecodeOptions::default(),
                ),
                Err(cadmpeg_ir::DecodeFailure::Codec(
                    cadmpeg_core::CodecError::Malformed(_)
                ))
            ));
        }
    }

    #[test]
    fn rejects_nested_joint_enumeration_carriers() {
        let cases = [
            r#"<Property name="JointType" type="App::PropertyEnumeration"><Integer value="0" CustomEnum="true"/><CustomEnumList count="1"><Wrapper><Enum value="Fixed"/></Wrapper></CustomEnumList></Property>
<Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Base"/></Property>
<Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Base"/></Property>"#,
            r#"<Property name="JointType" type="App::PropertyEnumeration"><Wrapper><Integer value="0"/></Wrapper><CustomEnumList count="1"><Enum value="Fixed"/></CustomEnumList></Property>
<Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Base"/></Property>
<Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Base"/></Property>"#,
            r#"<Property name="JointType" type="App::PropertyEnumeration"><Integer value="0" CustomEnum="true"/><CustomEnumList count="1"><Enum value="Fixed"/><Enum value="Revolute"/></CustomEnumList></Property>
<Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Base"/></Property>
<Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Base"/></Property>"#,
            r#"<Property name="JointType" type="App::PropertyEnumeration"><Integer value="0"/><CustomEnumList count="1"><Enum value="Fixed"/></CustomEnumList></Property>
<Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Base"/></Property>
<Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Base"/></Property>"#,
        ];
        for properties in cases {
            let document = format!(
                r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::FeaturePython" name="Joint"/></Objects>
<ObjectData Count="1"><Object name="Joint"><Properties Count="3">{properties}</Properties></Object></ObjectData>
</Document>"#
            );
            assert!(matches!(
                FcstdCodec.decode(
                    &mut Cursor::new(archive(&document)),
                    &DecodeOptions::default(),
                ),
                Err(cadmpeg_ir::DecodeFailure::Codec(
                    cadmpeg_core::CodecError::Malformed(_)
                ))
            ));
        }
    }

    #[test]
    fn rejects_wrong_joint_scalar_runtime_types_and_value_tags() {
        for property in [
            r#"<Property name="Angle" type="App::PropertyFloat"><Float value="15"/></Property>"#,
            r#"<Property name="Distance" type="App::PropertyLength"><Integer value="3"/></Property>"#,
            r#"<Property name="EnableAngleMin" type="App::PropertyBool"><Bool value="true"/><Bool value="false"/></Property>"#,
            r#"<Property name="Suppressed" type="App::PropertyString"><String value="true"/></Property>"#,
        ] {
            let document = format!(
                r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::FeaturePython" name="Joint"/></Objects>
<ObjectData Count="1"><Object name="Joint"><Properties Count="4">
<Property name="JointType" type="App::PropertyEnumeration"><Integer value="0"/></Property>
<Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name=""/></Property>
<Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name=""/></Property>
{property}
</Properties></Object></ObjectData></Document>"#
            );
            assert!(matches!(
                FcstdCodec.decode(
                    &mut Cursor::new(archive(&document)),
                    &DecodeOptions::default(),
                ),
                Err(cadmpeg_ir::DecodeFailure::Codec(
                    cadmpeg_core::CodecError::Malformed(_)
                ))
            ));
        }
    }

    #[test]
    fn rejects_ambiguous_joint_kind_and_scalar_carriers() {
        let documents = [
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Base"/><Object type="App::FeaturePython" name="Joint"/></Objects>
<ObjectData Count="2"><Object name="Base"><Properties Count="0"/></Object><Object name="Joint"><Properties Count="3">
<Property name="ObjectToGround" type="App::PropertyLinkGlobal"><Link value="Base"/></Property>
<Property name="JointType" type="App::PropertyEnumeration"><Integer value="0" CustomEnum="true"/><CustomEnumList count="1"><Enum value="Fixed"/></CustomEnumList></Property>
<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
</Properties></Object></ObjectData></Document>"#,
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::FeaturePython" name="Joint"/></Objects>
<ObjectData Count="1"><Object name="Joint"><Properties Count="1">
<Property name="JointType" type="App::PropertyEnumeration"><Integer value="0" CustomEnum="true"/><Integer value="1"/><CustomEnumList count="2"><Enum value="Fixed"/><Enum value="Revolute"/></CustomEnumList></Property>
</Properties></Object></ObjectData></Document>"#,
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::FeaturePython" name="Joint"/></Objects>
<ObjectData Count="1"><Object name="Joint"><Properties Count="2">
<Property name="JointType" type="App::PropertyEnumeration"><Integer value="0" CustomEnum="true"/><CustomEnumList count="1"><Enum value="Fixed"/></CustomEnumList></Property>
<Property name="Suppressed" type="App::PropertyBool"><Bool value="true"/><Bool value="false"/></Property>
</Properties></Object></ObjectData></Document>"#,
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Base"/><Object type="App::FeaturePython" name="Joint"/></Objects>
<ObjectData Count="2"><Object name="Base"><Properties Count="0"/></Object><Object name="Joint"><Properties Count="2">
<Property name="ObjectToGround" type="App::PropertyLinkGlobal"><Link value="Base"/></Property>
<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/><PropertyPlacement Px="1" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
</Properties></Object></ObjectData></Document>"#,
        ];
        for document in documents {
            assert!(matches!(
                FcstdCodec.decode(
                    &mut Cursor::new(archive(document)),
                    &DecodeOptions::default(),
                ),
                Err(cadmpeg_ir::DecodeFailure::Codec(
                    cadmpeg_core::CodecError::Malformed(_)
                ))
            ));
        }
    }
}
