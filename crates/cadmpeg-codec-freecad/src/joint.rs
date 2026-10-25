// SPDX-License-Identifier: Apache-2.0
//! Assembly joints recovered without executing Python proxy payloads.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::native::joint::{JointBody, JointConnectorRecord, JointRecord, PairedJointFamily};
use crate::native::{sole_named_property, LinkTarget, ObjectRecord, PropertyRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::products::{
    AssemblyJoint, JointConnector, JointId, JointLimits, JointOperand, Occurrence, PairedJointKind,
};
use cadmpeg_ir::scalar::FiniteReal;

pub(crate) fn transfer_source<'source>(
    ctx: &crate::property_reader::PropertyReader<'_, 'source>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    sources: &BTreeMap<String, crate::persistence::object_parts::ObjectSource<'source>>,
) -> Result<(Vec<JointRecord>, Vec<crate::design::UnformedProjection>), CodecError> {
    transfer_impl(ctx, objects, properties, Some(sources))
}

fn transfer_impl<'source>(
    ctx: &crate::property_reader::PropertyReader<'_, 'source>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    sources: Option<&BTreeMap<String, crate::persistence::object_parts::ObjectSource<'source>>>,
) -> Result<(Vec<JointRecord>, Vec<crate::design::UnformedProjection>), CodecError> {
    let mut by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in properties {
        if !by_owner.contains_key(property.owner.as_str()) {
            by_owner.insert(&property.owner, Vec::new());
        }
        if let Some(owned) = by_owner.get_mut(property.owner.as_str()) {
            owned.push(property);
        }
    }
    let mut output = Vec::new();
    let mut unformed = Vec::new();
    for object in objects {
        ctx.begin_projection(object.id())?;
        let source = by_owner
            .get(object.id().as_str())
            .map_or(&[][..], Vec::as_slice);
        let mut owned = Vec::new();
        owned.extend_from_slice(source);
        let grounded_property = sole_named_property("joint", &owned, "ObjectToGround")?;
        let joint_type_property = sole_named_property("joint", &owned, "JointType")?;
        if grounded_property.is_none() && joint_type_property.is_none() {
            let actual_grounded_role = ctx.unavailable_named_exact("ObjectToGround")?;
            let actual_kind_role = ctx.unavailable_named_exact("JointType")?;
            if !actual_grounded_role && !actual_kind_role {
                continue;
            }
        }
        let grounded_missing = ctx.missing_value(grounded_property, "ObjectToGround")?;
        let kind_missing = ctx.missing_value(joint_type_property, "JointType")?;
        if grounded_property.is_none()
            && joint_type_property.is_none()
            && !grounded_missing
            && !kind_missing
        {
            continue;
        }
        let id = crate::native::native_id("joint", object.name());
        let neutral_id = crate::native::model_id("joint", object.id(), "constraint");
        if joint_type_property
            .is_some_and(|property| property.type_name != "App::PropertyEnumeration")
        {
            return Err(CodecError::malformed(
                "joint JointType property has the wrong runtime type",
            ));
        }
        let mut required_missing = grounded_missing || kind_missing;
        let joint_type = if kind_missing {
            None
        } else {
            joint_type_property
                .map(|property| enumeration_value(ctx, property))
                .transpose()?
                .flatten()
        };
        required_missing |= joint_type_property.is_some() && joint_type.is_none();
        let roles: &[&str] = if grounded_property.is_some() || grounded_missing {
            &["Placement"]
        } else {
            &[
                "Reference1",
                "Reference2",
                "Placement1",
                "Placement2",
                "Offset1",
                "Offset2",
            ]
        };
        for name in roles {
            required_missing |=
                ctx.missing_value(sole_named_property("joint", &owned, name)?, name)?;
        }
        if required_missing {
            let homes = ctx.take_unavailable();
            if !homes.is_empty() {
                unformed.push(crate::design::UnformedProjection {
                    ids: if ctx.container_only() {
                        vec![id]
                    } else {
                        vec![id, neutral_id]
                    },
                    homes,
                });
            }
            continue;
        }
        let mut neutral_missing = false;
        if !ctx.container_only() {
            for name in ["Detach1", "Detach2", "Suppressed"] {
                neutral_missing |=
                    ctx.missing_value(sole_named_property("joint", &owned, name)?, name)?;
            }
        }
        if neutral_missing {
            let homes = ctx.take_unavailable();
            if !homes.is_empty() {
                unformed.push(crate::design::UnformedProjection {
                    ids: vec![neutral_id],
                    homes,
                });
            }
        }
        let construct = |actual_object: String| -> Result<JointRecord, CodecError> {
            if grounded_property.is_some() && joint_type_property.is_some() {
                return Err(CodecError::malformed(format_args!(
                    "joint object {} carries both ObjectToGround and JointType",
                    object.id()
                )));
            }
            if let Some(property) = grounded_property {
                let legacy_empty_sub = property.type_name == "App::PropertyLinkSub"
                    && property.links().len() == 1
                    && property.links()[0]
                        .as_ref()
                        .is_none_or(|link| link.subelements().iter().all(String::is_empty));
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
            let body = if grounded_property.is_some() {
                let placement = placement(&owned, "Placement")?.unwrap_or_default();
                let reference = grounded_property
                    .into_iter()
                    .flat_map(PropertyRecord::links)
                    .flatten()
                    .find(|link| link.document().is_some() || link.object().is_some())
                    .cloned();
                JointBody::Grounded {
                    reference,
                    placement,
                }
            } else if let Some(joint_type) = joint_type.clone() {
                let connector_record =
                    |owned: &[&PropertyRecord],
                     reference_name: &str,
                     placement_name: &str,
                     offset_name: &str|
                     -> Result<JointConnectorRecord, CodecError> {
                        Ok(JointConnectorRecord {
                            reference: connector(owned, reference_name)?,
                            placement: placement(owned, placement_name)?.unwrap_or_default(),
                            offset: placement(owned, offset_name)?.unwrap_or_default(),
                        })
                    };
                JointBody::Pair {
                    kind: PairedJointFamily::new(joint_type).map_err(CodecError::Malformed)?,
                    connectors: Box::new([
                        connector_record(&owned, "Reference1", "Placement1", "Offset1")?,
                        connector_record(&owned, "Reference2", "Placement2", "Offset2")?,
                    ]),
                }
            } else {
                return Err(CodecError::malformed("joint has no declared runtime value"));
            };
            let mut parameters = BTreeMap::new();
            for property in owned.iter().filter(|property| {
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
            }) {
                if let Some(value) = scalar_parameter(property)? {
                    parameters.insert(property.name.clone(), value);
                }
            }
            JointRecord::try_new(id.clone(), actual_object, body, parameters)
        };
        let record = match sources {
            Some(sources) => sources
                .get(object.id())
                .ok_or_else(|| {
                    CodecError::malformed("joint owner has no original source authority")
                })?
                .record(ctx, &id, construct)?,
            None => construct(object.id().to_owned())?,
        };
        output.push(record);
    }
    Ok((output, unformed))
}

pub(crate) fn transfer_neutral(
    records: &[JointRecord],
    occurrences: &[Occurrence],
    properties: &[PropertyRecord],
    unavailable: &BTreeSet<String>,
) -> Result<Vec<AssemblyJoint>, CodecError> {
    let mut occurrence_by_native = HashMap::new();
    for occurrence in occurrences {
        if let Some(native) = occurrence.native_ref.as_deref() {
            occurrence_by_native.insert(native, &occurrence.id);
        }
    }
    let mut output = Vec::new();
    for record in records {
        let model_id = crate::native::model_id("joint", record.object(), "constraint");
        if unavailable.contains(&model_id)
            || properties.iter().any(|property| {
                property.owner == record.object()
                    && matches!(property.name.as_str(), "Detach1" | "Detach2" | "Suppressed")
                    && property.body.is_none()
            })
        {
            continue;
        }
        let parameters = record.parameters();
        let bool_value = |name: &str| parameters.bool_value(name);
        let scalar = |name: &str| parameters.scalar_value(name);
        let enabled_limits =
            |minimum: &str, maximum: &str, enable_min: &str, enable_max: &str, scale: f64| {
                let scaled_bound = |value: FiniteReal| {
                    if scale == 1.0 {
                        Ok(value)
                    } else {
                        FiniteReal::new(value.get() * scale).ok_or_else(|| {
                            CodecError::Malformed(
                                "joint limits minimum/maximum must be finite and ordered".into(),
                            )
                        })
                    }
                };
                let minimum = bool_value(enable_min)
                    .is_some_and(|enabled| enabled)
                    .then(|| scalar(minimum))
                    .flatten()
                    .map(scaled_bound)
                    .transpose()?;
                let maximum = bool_value(enable_max)
                    .is_some_and(|enabled| enabled)
                    .then(|| scalar(maximum))
                    .flatten()
                    .map(scaled_bound)
                    .transpose()?;
                if minimum.is_none() && maximum.is_none() {
                    Ok(None)
                } else {
                    JointLimits::from_parts(minimum, maximum)
                        .map(Some)
                        .ok_or_else(|| {
                            CodecError::Malformed(
                                "joint limits minimum/maximum must be finite and ordered".into(),
                            )
                        })
                }
            };
        let operand = |reference: &LinkTarget| -> Result<Option<JointOperand>, CodecError> {
            let Some(name) = reference.object() else {
                return Ok(None);
            };
            let object = name.to_owned();
            let mut subelements = Vec::new();
            for name in reference
                .subelements()
                .iter()
                .filter(|name| !name.is_empty())
            {
                subelements.push(name.to_owned());
            }
            if let Some(document) = reference.document() {
                let document = crate::product::external_document_reference(
                    document.as_str(),
                    document.attribute(),
                );
                return Ok(Some(JointOperand::external(document, object, subelements)));
            }
            Ok(Some(match occurrence_by_native.get(name).copied() {
                Some(occurrence) => {
                    let identity = occurrence.clone();
                    JointOperand::occurrence(identity, object, subelements)
                }
                None => JointOperand::root(object, subelements),
            }))
        };
        let id = JointId::mint(crate::native::model_id(
            "joint",
            record.object(),
            "constraint",
        ))
        .map_err(CodecError::malformed)?;
        let angle = scalar("Angle").map(|value| value.get().to_radians());
        let distance = scalar("Distance");
        let distance2 = scalar("Distance2");
        let angular_limits = enabled_limits(
            "AngleMin",
            "AngleMax",
            "EnableAngleMin",
            "EnableAngleMax",
            std::f64::consts::PI / 180.0,
        )?;
        let linear_limits = enabled_limits(
            "LengthMin",
            "LengthMax",
            "EnableLengthMin",
            "EnableLengthMax",
            1.0,
        )?;
        let mut joint = match &record.body {
            JointBody::Grounded {
                reference,
                placement,
            } => {
                let Some(reference) = reference.as_ref() else {
                    continue;
                };
                let Some(operand) = operand(reference)? else {
                    continue;
                };
                AssemblyJoint::grounded(
                    id,
                    JointConnector {
                        operand,
                        frame: placement.transform(),
                        detached: bool_value("Detach1").is_some_and(|value| value),
                    },
                    None,
                )
            }
            JointBody::Pair { kind, connectors } => {
                let [first, second] = connectors.as_ref();
                let kind = joint_kind(
                    kind,
                    angle,
                    distance,
                    distance2,
                    angular_limits,
                    linear_limits,
                )?;
                let Some(first_reference) = first.reference.as_ref() else {
                    continue;
                };
                let Some(first_operand) = operand(first_reference)? else {
                    continue;
                };
                let Some(second_reference) = second.reference.as_ref() else {
                    continue;
                };
                let Some(second_operand) = operand(second_reference)? else {
                    continue;
                };
                AssemblyJoint::paired(
                    id,
                    kind,
                    [
                        JointConnector {
                            operand: first_operand,
                            frame: first.placement.transform(),
                            detached: bool_value("Detach1").is_some_and(|value| value),
                        },
                        JointConnector {
                            operand: second_operand,
                            frame: second.placement.transform(),
                            detached: bool_value("Detach2").is_some_and(|value| value),
                        },
                    ],
                    Some([first.offset.transform(), second.offset.transform()]),
                )
            }
        };
        joint.suppressed = bool_value("Suppressed").is_some_and(|value| value);
        joint.native_ref = Some(record.id().to_owned());

        output.push(joint);
    }
    Ok(output)
}

fn joint_kind(
    kind: &PairedJointFamily,
    angle: Option<f64>,
    distance: Option<FiniteReal>,
    distance2: Option<FiniteReal>,
    angular_limits: Option<JointLimits>,
    linear_limits: Option<JointLimits>,
) -> Result<PairedJointKind, CodecError> {
    let finite_angle = |value: f64| {
        FiniteReal::new(value)
            .ok_or_else(|| CodecError::Malformed("joint scalar must be finite".into()))
    };
    let angle = angle.map(finite_angle).transpose()?;
    let lower = kind.as_str().to_ascii_lowercase();
    Ok(match lower.as_str() {
        "fixed" => PairedJointKind::Fixed {
            angle,
            translation_offset: None,
            angular_limits,
            linear_limits,
        },
        "revolute" => PairedJointKind::Revolute {
            angle,
            angular_limits,
        },
        "slider" | "prismatic" => PairedJointKind::Slider {
            distance,
            translation_offset: None,
            linear_limits,
        },
        "cylindrical" => PairedJointKind::Cylindrical {
            angle,
            distance,
            angular_limits,
            linear_limits,
        },
        "ball" | "spherical" => PairedJointKind::Ball {},
        "distance" => PairedJointKind::Distance { distance },
        "parallel" => PairedJointKind::Parallel {},
        "perpendicular" => PairedJointKind::Perpendicular {},
        "angle" => PairedJointKind::Angle { angle },
        "rackpinion" | "rack_pinion" => PairedJointKind::RackPinion {
            distance,
            distance2,
        },
        "screw" => PairedJointKind::Screw { distance },
        "gears" => PairedJointKind::Gears {
            distance,
            distance2,
        },
        "belt" => PairedJointKind::Belt {
            distance,
            distance2,
        },
        _ => PairedJointKind::Native {
            name: kind.as_str().to_owned(),
            angle,
            translation_offset: None,
            distance,
            distance2,
            angular_limits,
            linear_limits,
        },
    })
}

fn enumeration_value(
    ctx: &crate::property_reader::PropertyReader<'_, '_>,
    property: &PropertyRecord,
) -> Result<Option<String>, CodecError> {
    let values = property.values();
    let Some(integer) = values
        .iter()
        .find(|value| value.parent.is_none() && value.tag == "Integer")
    else {
        ctx.unavailable(property)?;
        return Ok(None);
    };
    let index = integer
        .attributes
        .get("value")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| {
            CodecError::malformed("joint enumeration requires a nonnegative Integer value")
        })?;
    if integer.attributes.get("CustomEnum").map(String::as_str) != Some("true") {
        return Ok(Some(index.to_string()));
    }
    let Some(list) = values
        .iter()
        .find(|value| value.parent.is_none() && value.tag == "CustomEnumList")
    else {
        ctx.unavailable(property)?;
        return Ok(None);
    };
    let count = list
        .attributes
        .get("count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| CodecError::malformed("joint CustomEnumList requires count"))?;
    if index >= count {
        return Ok(Some(index.to_string()));
    }
    let enums = values
        .iter()
        .filter(|value| value.parent == Some(list.order) && value.tag == "Enum");
    let explicit = enums.clone().any(|value| value.item_index.is_some());
    let dense = !matches!(&property.body, Some(crate::native::PropertyBody::Persisted { values, .. }) if values.has_missing())
        && enums.clone().count() == count;
    let selected = enums.enumerate().find_map(|(ordinal, value)| {
        ((value.item_index == Some(index)) || (!explicit && dense && ordinal == index))
            .then_some(value)
    });
    match selected.and_then(|value| value.attributes.get("value")) {
        Some(value) => Ok(Some(value.clone())),
        None => {
            ctx.unavailable(property)?;
            Ok(None)
        }
    }
}

fn scalar_parameter(property: &PropertyRecord) -> Result<Option<String>, CodecError> {
    // Persistence admission already reported this unreadable scalar. Keep the
    // independently typed joint relation without inventing its missing value.
    if property.body.is_none() || matches!(property.body, None) {
        return Ok(None);
    }
    let (expected_type, expected_tag) = match property.name.as_str() {
        "Angle" | "AngleMin" | "AngleMax" => ("App::PropertyAngle", "Float"),
        "Distance" | "Distance2" | "LengthMin" | "LengthMax" => ("App::PropertyLength", "Float"),
        "EnableAngleMin" | "EnableAngleMax" | "EnableLengthMin" | "EnableLengthMax" | "Detach1"
        | "Detach2" | "Suppressed" => ("App::PropertyBool", "Bool"),
        _ => return Ok(None),
    };
    if property.type_name != expected_type {
        return Err(CodecError::malformed(format_args!(
            "joint parameter property {} has runtime type {}, expected {expected_type}",
            property.id, property.type_name
        )));
    }
    let [value] = property.values() else {
        return Err(CodecError::malformed(format_args!(
            "joint parameter property {} requires one {expected_tag} value",
            property.id
        )));
    };
    if value.tag != expected_tag {
        return Err(CodecError::malformed(format_args!(
            "joint parameter property {} requires a {expected_tag} value",
            property.id
        )));
    }
    let value = value.attributes.get("value").ok_or_else(|| {
        CodecError::malformed(format_args!(
            "joint parameter property {} has no value",
            property.id
        ))
    })?;
    if expected_tag == "Float"
        && value
            .parse::<f64>()
            .ok()
            .and_then(FiniteReal::new)
            .is_none()
    {
        return Err(CodecError::malformed(format_args!(
            "joint parameter property {}: joint parameter {} has an invalid value {value:?}",
            property.id, property.name,
        )));
    }
    Ok(Some(value.to_owned()))
}

fn connector(
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<crate::native::LinkTarget>, CodecError> {
    let Some(property) = sole_named_property("joint", properties, name)? else {
        return Err(CodecError::malformed(format_args!(
            "joint connector {name} is missing"
        )));
    };
    if !matches!(
        property.type_name.as_str(),
        "App::PropertyXLinkSub" | "App::PropertyXLinkSubHidden"
    ) {
        return Err(CodecError::malformed(format_args!(
            "joint connector {} has runtime type {}, expected App::PropertyXLinkSub",
            property.id, property.type_name
        )));
    }
    let unavailable_null = property.body.is_some()
        && property.values().is_empty()
        && matches!(property.links(), [None]);
    if !unavailable_null
        && property
            .values()
            .first()
            .is_none_or(|value| value.tag != "XLink")
    {
        return Err(CodecError::malformed(format_args!(
            "joint connector {} requires one XLink value",
            property.id
        )));
    }
    let [target] = property.links() else {
        return Err(CodecError::malformed(format_args!(
            "joint connector {} requires one target, found {}",
            property.id,
            property.links().len()
        )));
    };
    Ok(target.clone())
}

fn placement(
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<crate::native::frame::FiniteFrame>, CodecError> {
    let Some(property) = sole_named_property("joint", properties, name)? else {
        return Ok(None);
    };
    crate::placement::placement_matrix(property)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::joint_kind;
    use crate::test_support::test_archive::{archive, assert_valid_document};
    use crate::FcstdCodec;

    fn diagnostic_property(name: &str, type_name: &str) -> crate::native::PropertyRecord {
        crate::native::PropertyRecord {
            dynamic: None,
            id: "property".into(),
            owner: "joint".into(),
            name: name.into(),
            type_name: type_name.into(),
            family: crate::native::PropertyFamily::Unknown,
            status: None,
            body: Some(crate::native::PropertyBody::Transient),
            order: 0,
        }
    }

    fn scalar_property(type_name: &str, xml: &str) -> crate::native::PropertyRecord {
        let mut property = diagnostic_property("Angle", type_name);
        let document = roxmltree::Document::parse(xml).expect("valid scalar property XML");
        let values = document
            .root_element()
            .children()
            .filter(roxmltree::Node::is_element)
            .enumerate()
            .map(|(order, node)| crate::native::ValueRecord {
                item_index: None,
                link_index: None,
                parent: None,
                tag: node.tag_name().name().into(),
                order,
                attributes: node
                    .attributes()
                    .map(|attribute| (attribute.name().into(), attribute.value().into()))
                    .collect(),
                decoded_json: None,
                text: None,
            })
            .collect::<Vec<_>>();
        property.body = Some(crate::native::PropertyBody::Persisted {
            values: values.into(),
            links: Vec::new(),
            side_entries: Vec::new(),
        });
        property
    }

    #[test]
    fn joint_invalid_scalar_value_is_malformed() {
        let property = scalar_property(
            "App::PropertyAngle",
            "<Property><Float value=\"bad\"/></Property>",
        );
        let message = super::scalar_parameter(&property)
            .expect_err("invalid source scalar")
            .to_string();
        assert_eq!(message, "malformed container: joint parameter property property: joint parameter Angle has an invalid value \"bad\"");
    }
    use cadmpeg_ir::products::PairedJointKind;
    use cadmpeg_ir::{Codec, DecodeOptions};
    use cadmpeg_test_support::wire;
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
        ] {
            assert!(
                !matches!(
                    joint_kind(
                        &super::PairedJointFamily::new(family.into()).unwrap(),
                        None,
                        None,
                        None,
                        None,
                        None
                    )
                    .unwrap(),
                    PairedJointKind::Native { .. }
                ),
                "{family} must not fall through to a native joint family"
            );
        }
    }

    #[test]
    fn custom_joint_labels_keep_source_case_through_neutral_transfer() {
        for family in ["CustomCoupling", "My Joint V2"] {
            let document = format!(
                r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Assembly::AssemblyObject" name="Base"/><Object type="App::FeaturePython" name="Joint"/></Objects>
<ObjectData Count="2">
<Object name="Base"><Properties Count="0"/></Object>
<Object name="Joint"><Properties Count="3">
<Property name="JointType" type="App::PropertyEnumeration"><Integer value="0" CustomEnum="true"/><CustomEnumList count="1"><Enum value="{family}"/></CustomEnumList></Property>
<Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Base"/></Property>
<Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Base"/></Property>
</Properties></Object></ObjectData></Document>"#
            );
            let result = FcstdCodec
                .decode(
                    &mut Cursor::new(archive(&document)),
                    &DecodeOptions::default(),
                )
                .expect("custom joint family");
            let native = result
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as::<crate::native::joint::JointRecord>("joints")
                .unwrap();
            assert_eq!(native.len(), 1);
            assert_eq!(native[0].kind(), family);
            let [joint] = result.ir().model.assembly_joints.as_slice() else {
                panic!("one neutral joint")
            };
            assert!(matches!(
                wire::field::<cadmpeg_ir::products::JointOperands>(joint, "operands"),
                cadmpeg_ir::products::JointOperands::Pair { kind: PairedJointKind::Native { name, .. }, .. } if name == family
            ));
            assert_valid_document(result.ir());
            let wire = serde_json::to_string(result.ir()).unwrap();
            let restored = cadmpeg_ir::CadIr::from_json(&wire).unwrap();
            assert_valid_document(&restored);
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
            .arena_as::<crate::native::joint::JointRecord>("joints")
            .expect("joints");
        assert_eq!(joints.len(), 1);
        assert_eq!(joints[0].kind(), "Revolute");
        assert_eq!(joints[0].references().count(), 2);
        assert_eq!(
            joints[0]
                .references()
                .next()
                .and_then(crate::native::LinkTarget::object),
            Some("fcstd:native:object#Assembly")
        );
        let first_reference = joints[0]
            .references()
            .next()
            .expect("first joint reference");
        assert_eq!(first_reference.subelements(), ["A.Face1", "A.Edge2"]);
        assert_eq!(joints[0].placements()[1][0][3], 2.0);
        assert_eq!(joints[0].parameters().raw("Suppressed"), Some("true"));
        assert_eq!(result.ir().model.assembly_joints.len(), 1);
        let joint = &result.ir().model.assembly_joints[0];
        let cadmpeg_ir::products::JointOperands::Pair {
            kind:
                PairedJointKind::Revolute {
                    angle,
                    angular_limits,
                },
            offset_frames,
            ..
        } = wire::field(joint, "operands")
        else {
            panic!("revolute pair")
        };
        let connectors = joint.connectors().collect::<Vec<_>>();
        assert_eq!(connectors.len(), 2);
        assert!(connectors.iter().all(|connector| matches!(
            connector.operand.container,
            cadmpeg_ir::OperandContainer::Occurrence { .. }
        )));
        assert_eq!(connectors[1].frame.rows()[0][3], 2.0);
        let offset_frames = offset_frames.expect("offset frames");
        assert_eq!(offset_frames.len(), 2);
        assert_eq!(offset_frames[0].rows()[0][3], 0.5);
        assert_eq!(offset_frames[1].rows()[0][3], 1.5);
        assert!(joint.suppressed);
        assert_eq!(
            [connectors[0].detached, connectors[1].detached],
            [true, false]
        );
        assert!((angle.expect("angle").get() - 15_f64.to_radians()).abs() < EPS_JOINT_SCALAR);
        let cadmpeg_ir::products::JointLimits::Range(range) =
            angular_limits.expect("angular limits")
        else {
            panic!("bounded angular range")
        };
        assert!((range.minimum().get() - (-30_f64).to_radians()).abs() < EPS_JOINT_SCALAR);
        assert!((range.maximum().get() - 45_f64.to_radians()).abs() < EPS_JOINT_SCALAR);
        assert!(crate::test_support::validate_native(result.ir()).is_empty());
        assert_valid_document(result.ir());
        let mut wire = serde_json::to_value(&result.ir().model.assembly_joints[0])
            .expect("assembly joint wire");
        wire["operands"]["connectors"][0]["operand"]["external_document"] = serde_json::json!({
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
        assert!(matches!(
            wire::field::<cadmpeg_ir::products::JointOperands>(joint, "operands"),
            cadmpeg_ir::products::JointOperands::Grounded { .. }
        ));
        let connectors = joint.connectors().collect::<Vec<_>>();
        assert_eq!(connectors.len(), 1);
        assert!(matches!(
            connectors[0].operand.container,
            cadmpeg_ir::OperandContainer::Occurrence { .. }
        ));
        assert_eq!(connectors[0].frame.rows()[0][3], 7.0);
        assert_eq!(connectors[0].frame.rows()[1][3], 8.0);
        assert_eq!(connectors[0].frame.rows()[2][3], 9.0);
        assert!(crate::test_support::validate_native(result.ir()).is_empty());
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
        for (index, properties) in [
            r#"<Property name="Reference1" type="App::PropertyLinkSub"><LinkSub value="Base" count="0"/></Property>
<Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Base"/></Property>"#,
            r#"<Property name="Reference1" type="App::PropertyXLinkSubList"><XLinkSubList count="2"><XLink file="" name="Base"/><XLink file="" name="Other"/></XLinkSubList></Property>
<Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Base"/></Property>"#,
            r#"<Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Base"/></Property>"#,
        ].into_iter().enumerate() {
            let property_count = properties.lines().count() + 1;
            let document = format!(
                r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::FeaturePython" name="Joint"/></Objects>
<ObjectData Count="1"><Object name="Joint"><Properties Count="{property_count}">
<Property name="JointType" type="App::PropertyEnumeration"><Integer value="0"/></Property>
{properties}
</Properties></Object></ObjectData></Document>"#
            );
            let result = FcstdCodec.decode(&mut Cursor::new(archive(&document)),
                &DecodeOptions::default()).expect("never-declared connector targets fail their actual leaves");
            let expected: &[(&str, &str)] = match index {
                0 => &[
                    ("freecad.property_value", "<LinkSub value=\"Base\" count=\"0\"/>"),
                    ("freecad.property_value", "<XLink file=\"\" name=\"Base\"/>"),
                ],
                1 => &[
                    ("freecad.value_element", "<XLink file=\"\" name=\"Base\"/>"),
                    ("freecad.value_element", "<XLink file=\"\" name=\"Other\"/>"),
                    ("freecad.property_value", "<XLink file=\"\" name=\"Base\"/>"),
                ],
                2 => &[("freecad.property_value", "<XLink file=\"\" name=\"Base\"/>")],
                _ => unreachable!(),
            };
            crate::test_support::source_fidelity::assert_failed_xml_parts(&result, &document, expected);
            assert!(result.ir().native.namespace("fcstd").unwrap()
                .arena_as::<crate::native::joint::JointRecord>("joints").unwrap().is_empty());
            assert!(result.ir().model.assembly_joints.is_empty());
            assert_valid_document(result.ir());
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
        for (index, properties) in cases.into_iter().enumerate() {
            let document = format!(
                r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::FeaturePython" name="Joint"/></Objects>
<ObjectData Count="1"><Object name="Joint"><Properties Count="3">{properties}</Properties></Object></ObjectData>
</Document>"#
            );
            let result = FcstdCodec
                .decode(
                    &mut Cursor::new(archive(&document)),
                    &DecodeOptions::default(),
                )
                .expect("nested enum and dangling targets stay at their source homes");
            let mut expected = vec![
                ("freecad.property_value", "<XLink file=\"\" name=\"Base\"/>"),
                ("freecad.property_value", "<XLink file=\"\" name=\"Base\"/>"),
            ];
            match index {
                0 => expected.push(("freecad.value_element", "<Wrapper><Enum value=\"Fixed\"/></Wrapper>")),
                1 => {
                    expected.push(("freecad.property_value", "<Wrapper><Integer value=\"0\"/></Wrapper>"));
                    expected.push(("freecad.property_value", "<CustomEnumList count=\"1\"><Enum value=\"Fixed\"/></CustomEnumList>"));
                }
                2 => expected.push(("freecad.property_value", "<CustomEnumList count=\"1\"><Enum value=\"Fixed\"/><Enum value=\"Revolute\"/></CustomEnumList>")),
                3 => {},
                _ => unreachable!(),
            }
            crate::test_support::source_fidelity::assert_failed_xml_parts(
                &result, &document, &expected,
            );
            assert!(result
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as::<crate::native::joint::JointRecord>("joints")
                .unwrap()
                .is_empty());
            assert!(result.ir().model.assembly_joints.is_empty());
            assert_valid_document(result.ir());
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
            match FcstdCodec.decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            ) {
                Err(cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))) => {}
                Ok(decoded) => {
                    assert!(decoded
                        .report()
                        .losses
                        .iter()
                        .any(|loss| loss.code.local_code() == "record.unreadable"));
                    assert!(decoded
                        .source_fidelity()
                        .unrepresented()
                        .contains(&cadmpeg_ir::ArenaClass::Model));
                }
                Err(error) => panic!("unexpected joint failure: {error}"),
            }
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
        for (index, document) in documents.into_iter().enumerate() {
            if index < 3 {
                assert!(matches!(
                    FcstdCodec.decode(
                        &mut Cursor::new(archive(document)),
                        &DecodeOptions::default()
                    ),
                    Err(cadmpeg_ir::DecodeFailure::Codec(
                        cadmpeg_core::CodecError::Malformed(_)
                    ))
                ));
                continue;
            }
            let result = FcstdCodec
                .decode(
                    &mut Cursor::new(archive(document)),
                    &DecodeOptions::default(),
                )
                .expect("extra Placement fails without replacing first placement");
            crate::test_support::source_fidelity::assert_failed_xml_parts(&result, document,
                &[("freecad.property_value", "<PropertyPlacement Px=\"1\" Py=\"0\" Pz=\"0\" Q0=\"0\" Q1=\"0\" Q2=\"0\" Q3=\"1\"/>")]);
            let rows = result
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as::<crate::native::joint::JointRecord>("joints")
                .unwrap();
            assert_eq!(rows.len(), 1);
            assert!(
                matches!(&rows[0].body, crate::native::joint::JointBody::Grounded { reference: Some(target), placement }
                if target.object() == Some("fcstd:native:object#Base") && placement.rows()[0][3] == 0.0)
            );
            assert_valid_document(result.ir());
        }
    }

    #[test]
    fn malformed_joint_float_is_rejected_at_source_admission() {
        let document = r#"<Document SchemaVersion="4" FileVersion="1">
    <Objects Count="2"><Object type="Assembly::AssemblyObject" name="Base"/><Object type="App::FeaturePython" name="Joint"/></Objects>
    <ObjectData Count="2">
    <Object name="Base"><Properties Count="0"/></Object>
    <Object name="Joint"><Properties Count="4">
    <Property name="JointType" type="App::PropertyEnumeration"><Integer value="0" CustomEnum="true"/><CustomEnumList count="1"><Enum value="Revolute"/></CustomEnumList></Property>
    <Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Base"/></Property>
    <Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Base"/></Property>
    <Property name="Angle" type="App::PropertyAngle"><Float value="abc"/></Property>
    </Properties></Object>
    </ObjectData></Document>"#;
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect("independent joint relation survives unreadable angle");
        assert!(decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "record.unreadable"));
        assert!(decoded
            .source_fidelity()
            .unrepresented()
            .contains(&cadmpeg_ir::ArenaClass::Model));
        assert_eq!(decoded.ir().model.assembly_joints.len(), 1);
        assert_eq!(
            decoded
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as::<crate::native::joint::JointRecord>("joints")
                .unwrap()[0]
                .parameters()
                .raw("Angle"),
            None
        );
    }

    #[test]
    fn primary_bool_values_are_retained_and_projected_exactly() {
        for (raw, expected) in [
            ("true", Some(true)),
            ("false", Some(false)),
            ("1", Some(true)),
            ("0", Some(false)),
            ("TRUE", Some(true)),
            ("maybe", None),
        ] {
            let document = format!(
                r#"<Document SchemaVersion="4" FileVersion="1">
    <Objects Count="2"><Object type="Assembly::AssemblyObject" name="Base"/><Object type="App::FeaturePython" name="Joint"/></Objects>
    <ObjectData Count="2">
    <Object name="Base"><Properties Count="0"/></Object>
    <Object name="Joint"><Properties Count="4">
    <Property name="JointType" type="App::PropertyEnumeration"><Integer value="0" CustomEnum="true"/><CustomEnumList count="1"><Enum value="Revolute"/></CustomEnumList></Property>
    <Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Base"/></Property>
    <Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Base"/></Property>
    <Property name="Suppressed" type="App::PropertyBool"><Bool value="{raw}"/></Property>
    </Properties></Object>
    </ObjectData></Document>"#
            );
            let result = FcstdCodec
                .decode(
                    &mut Cursor::new(archive(&document)),
                    &DecodeOptions::default(),
                )
                .expect("primary bool values remain source-admissible");
            let joints = result
                .ir()
                .native
                .namespace("fcstd")
                .expect("native")
                .arena_as::<crate::native::joint::JointRecord>("joints")
                .expect("joints");
            assert_eq!(
                joints[0].parameters().raw("Suppressed"),
                expected.map(|_| raw)
            );
            assert_eq!(joints[0].parameters().bool_value("Suppressed"), expected);
            assert_eq!(
                result.ir().model.assembly_joints[0].suppressed,
                expected.unwrap_or(false)
            );
            let failures = result
                .source_fidelity()
                .parts()
                .values()
                .filter(|part| {
                    part.state() == &cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
                })
                .collect::<Vec<_>>();
            assert_eq!(failures.len(), usize::from(expected.is_none()));
            if expected.is_none() {
                assert_eq!(failures[0].level().as_str(), "freecad.property_value");
                assert!(result
                    .source_fidelity()
                    .unrepresented()
                    .contains(&cadmpeg_ir::ArenaClass::Model));
            }
            let id = crate::native::native_child_id(
                "property",
                &crate::native::native_id("object", "Joint"),
                "Suppressed",
            );
            let layout = &result.source_fidelity().record_layout()[&id];
            let raw_start = u64::try_from(
                document.rfind(&format!("value=\"{raw}\"")).unwrap() + "value=\"".len(),
            )
            .unwrap();
            let raw_end = raw_start + u64::try_from(raw.len()).unwrap();
            assert_eq!(
                layout.reads()[layout.stream()]
                    .iter()
                    .any(|range| range.start <= raw_start && range.end >= raw_end),
                expected.is_some()
            );
            let restored = cadmpeg_ir::CadIr::from_json(
                &serde_json::to_string(result.ir()).expect("CADIR serialization"),
            )
            .expect("complete CADIR admission");
            let restored_joints = restored
                .native
                .namespace("fcstd")
                .expect("native")
                .arena_as::<crate::native::joint::JointRecord>("joints")
                .expect("joints");
            assert_eq!(
                restored_joints[0].parameters().raw("Suppressed"),
                expected.map(|_| raw)
            );
            assert_eq!(
                restored.model.assembly_joints[0].suppressed,
                expected.unwrap_or(false)
            );
        }
    }
}
