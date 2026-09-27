// SPDX-License-Identifier: Apache-2.0
//! Assembly joints recovered without executing Python proxy payloads.

use std::collections::{BTreeMap, HashMap};

use crate::native::joint::{JointBody, JointConnectorRecord, JointRecord, PairedJointFamily};
use crate::native::{malformed, sole_named_property, LinkTarget, ObjectRecord, PropertyRecord};
use crate::resource::{collection_allocation_failed, collection_vec, materialized_bytes, reserve_vec_items, retained_string};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::products::{
    AssemblyJoint, JointConnector, JointId, JointLimits, JointOperand, Occurrence, PairedJointKind,
};
use cadmpeg_ir::scalar::FiniteReal;

pub(crate) fn transfer(
    ctx: &DecodeContext<'_>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Result<Vec<JointRecord>, CodecError> {
    let mut by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in properties {
        if !by_owner.contains_key(property.owner.as_str()) {
            ctx.charge_collection_items(1, "fcstd joint owner index")?;
            by_owner.try_reserve(1).map_err(|_| collection_allocation_failed(ctx, 1, "fcstd joint owner index"))?;
            by_owner.insert(&property.owner, Vec::new());
        }
        if let Some(owned) = by_owner.get_mut(property.owner.as_str()) {
            reserve_vec_items(ctx, owned, 1, "fcstd joint owner properties")?;
            owned.push(property);
        }
    }
    let mut output = Vec::new();
    for object in objects {
        let source = by_owner.get(object.id.as_str()).map(Vec::as_slice).unwrap_or(&[]);
        let mut owned = collection_vec(ctx, source.len(), "fcstd joint selected properties")?;
        owned.extend_from_slice(source);
        let grounded_property = sole_named_property("joint", &owned, "ObjectToGround")?;
        let joint_type_property = sole_named_property("joint", &owned, "JointType")?;
        if grounded_property.is_some() && joint_type_property.is_some() {
            return Err(CodecError::malformed(format_args!(
                "joint object {} carries both ObjectToGround and JointType",
                object.id
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
        let joint_type = joint_type_property.map(|property| enumeration_value(ctx, property)).transpose()?;
        let body = if grounded_property.is_some() {
            let placement = placement(ctx, &owned, "Placement")?.unwrap_or_default();
            let reference = grounded_property
                .into_iter()
                .flat_map(PropertyRecord::links)
                .flatten()
                .find(|link| link.document().is_some() || link.object().is_some())
                .map(|link| link.clone_with_context(ctx))
                .transpose()?;
            JointBody::Grounded {
                reference,
                placement,
            }
        } else if let Some(joint_type) = joint_type {
            let connector_record = |owned: &[&PropertyRecord],
                                    reference_name: &str,
                                    placement_name: &str,
                                    offset_name: &str|
             -> Result<JointConnectorRecord, CodecError> {
                Ok(JointConnectorRecord {
                    reference: connector(ctx, owned, reference_name)?,
                    placement: placement(ctx, owned, placement_name)?.unwrap_or_default(),
                    offset: placement(ctx, owned, offset_name)?.unwrap_or_default(),
                })
            };
            JointBody::Pair {
                kind: PairedJointFamily::new(joint_type).map_err(CodecError::Malformed)?,
                connectors: [
                    connector_record(&owned, "Reference1", "Placement1", "Offset1")?,
                    connector_record(&owned, "Reference2", "Placement2", "Offset2")?,
                ],
            }
        } else {
            continue;
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
            if let Some(value) = scalar_parameter(ctx, property)? {
                ctx.charge_collection_items(1, "fcstd joint parameters")?;
                parameters.insert(retained_string(ctx, &property.name, "fcstd joint parameter name")?, value);
            }
        }
        reserve_vec_items(ctx, &mut output, 1, "fcstd joint records")?;
        output.push(
            JointRecord::try_new(
                crate::native::native_id_charged(ctx, "joint", &object.name)?,
                retained_string(ctx, &object.id, "fcstd joint object")?,
                body,
                parameters,
            )
            .map_err(CodecError::Malformed)?,
        );
    }
    Ok(output)
}

pub(crate) fn transfer_neutral(
    ctx: &DecodeContext<'_>,
    records: &[JointRecord],
    occurrences: &[Occurrence],
) -> Result<Vec<AssemblyJoint>, CodecError> {
    let count = occurrences.iter().filter(|occurrence| occurrence.native_ref.is_some()).count();
    let mut occurrence_by_native = HashMap::new();
    ctx.charge_collection_items(count as u64, "fcstd joint occurrence index")?;
    occurrence_by_native.try_reserve(count)
        .map_err(|_| collection_allocation_failed(ctx, count as u64, "fcstd joint occurrence index"))?;
    for occurrence in occurrences {
        if let Some(native) = occurrence.native_ref.as_deref() {
            occurrence_by_native.insert(native, &occurrence.id);
        }
    }
    let mut output = Vec::new();
    for record in records {
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
            let object = retained_string(ctx, name, "fcstd joint operand object")?;
            let mut subelements = collection_vec(ctx, reference.subelements().len(), "fcstd joint operand subelements")?;
            for name in reference.subelements().iter().filter(|name| !name.is_empty()) {
                subelements.push(retained_string(ctx, name, "fcstd joint operand subelement")?);
            }
            if let Some(document) = reference.document() {
                let document = crate::product::external_document_reference_charged(
                    ctx, document.as_str(), document.attribute(),
                )?;
                return Ok(Some(JointOperand::external(document, object, subelements)));
            }
            Ok(Some(match occurrence_by_native.get(name).copied() {
                Some(occurrence) => {
                    let identity = cadmpeg_ir::ids::OccurrenceId::mint(retained_string(
                        ctx, occurrence.as_str(), "fcstd joint occurrence identity",
                    )?).map_err(CodecError::malformed)?;
                    JointOperand::occurrence(identity, object, subelements)
                }
                None => JointOperand::root(object, subelements),
            }))
        };
        let id = JointId::mint(crate::native::model_id_charged(
            ctx, "joint", &record.object, "constraint",
        )?).map_err(CodecError::malformed)?;
        let angle = scalar("Angle").map(|value| value.get().to_radians());
        let distance = scalar("Distance");
        let distance2 = scalar("Distance2");
        let angular_limits = enabled_limits(
            "AngleMin", "AngleMax", "EnableAngleMin", "EnableAngleMax",
            std::f64::consts::PI / 180.0,
        )?;
        let linear_limits = enabled_limits(
            "LengthMin", "LengthMax", "EnableLengthMin", "EnableLengthMax", 1.0,
        )?;
        let mut joint = match &record.body {
            JointBody::Grounded { reference, placement } => {
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
            JointBody::Pair { kind, connectors: [first, second] } => {
                let kind = joint_kind(ctx, kind, angle, distance, distance2, angular_limits, linear_limits)?;
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
        joint.native_ref = Some(retained_string(ctx, &record.id, "fcstd joint native reference")?);
        reserve_vec_items(ctx, &mut output, 1, "fcstd neutral joints")?;
        output.push(joint);
    }
    Ok(output)
}

fn joint_kind(
    ctx: &DecodeContext<'_>,
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
    let (mut lower, _reservation) = materialized_bytes(ctx, kind.as_str().len(), "fcstd joint kind matching")?;
    lower.extend_from_slice(kind.as_str().as_bytes());
    lower.make_ascii_lowercase();
    let lower = std::str::from_utf8(&lower)
        .map_err(|_| CodecError::Malformed("joint kind lost UTF-8 encoding".into()))?;
    Ok(match lower {
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
            name: retained_string(ctx, kind.as_str(), "fcstd native joint kind")?,
            angle,
            translation_offset: None,
            distance,
            distance2,
            angular_limits,
            linear_limits,
        },
    })
}

fn enumeration_value(ctx: &DecodeContext<'_>, property: &PropertyRecord) -> Result<String, CodecError> {
    let document = roxmltree::Document::parse(property.xml.text()).map_err(|error| {
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
    let mut values = root.children().filter(roxmltree::Node::is_element);
    let Some(integer) = values.next().filter(|value| value.has_tag_name("Integer"))
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
    let custom_list = values.next();
    if values.next().is_some()
        || custom_list.is_some_and(|value| !value.has_tag_name("CustomEnumList"))
    {
        return Err(malformed(format!(
            "joint enumeration property {} has extra direct value roots",
            property.id
        )));
    }
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
    let selected = if let Some(custom_list) = custom_list {
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
        let values = custom_list.children().filter(roxmltree::Node::is_element);
        let found = values.clone().count();
        if found != count || values.clone().any(|value| !value.has_tag_name("Enum")) {
            return Err(malformed(format!(
                "joint enumeration property {} CustomEnumList count={count} but {} direct Enum values were found",
                property.id,
                found
            )));
        }
        let mut selected = None;
        for (position, value) in values.enumerate() {
            if value.children().any(|child| child.is_element()) {
                return Err(malformed(format!(
                    "joint enumeration property {} has nested Enum values",
                    property.id
                )));
            }
            let text = value.attribute("value").ok_or_else(|| {
                malformed(format!(
                    "joint enumeration property {} Enum has no value",
                    property.id
                ))
            })?;
            if position == index {
                selected = Some(text);
            }
        }
        selected
    } else {
        None
    };
    selected.map(|value| retained_string(ctx, value, "fcstd joint enumeration value"))
        .transpose().map(|value| value.unwrap_or_else(|| index.to_string()))
}

fn scalar_parameter(ctx: &DecodeContext<'_>, property: &PropertyRecord) -> Result<Option<String>, CodecError> {
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
    let [value] = property.values() else {
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
    let value = value.attributes.get("value").ok_or_else(|| {
        malformed(format!(
            "joint parameter property {} has no value",
            property.id
        ))
    })?;
    crate::native::joint::validate_parameter_value(&property.name, value)
        .map_err(|error| malformed(format!("joint parameter property {}: {error}", property.id)))?;
    Ok(Some(retained_string(ctx, value, "fcstd joint scalar parameter")?))
}

fn connector(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<crate::native::LinkTarget>, CodecError> {
    let Some(property) = sole_named_property("joint", properties, name)? else {
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
        .values()
        .first()
        .is_none_or(|value| value.tag != "XLink")
    {
        return Err(malformed(format!(
            "joint connector {} requires one XLink value",
            property.id
        )));
    }
    let [target] = property.links() else {
        return Err(malformed(format!(
            "joint connector {} requires one target, found {}",
            property.id,
            property.links().len()
        )));
    };
    target.as_ref().map(|target| target.clone_with_context(ctx)).transpose()
}

fn placement(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<crate::native::frame::FiniteFrame>, CodecError> {
    let Some(property) = sole_named_property("joint", properties, name)? else {
        return Ok(None);
    };
    crate::placement::placement_matrix(ctx, property)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::joint_kind;
    use crate::test_support::test_archive::{archive, assert_valid_document};
    use crate::FcstdCodec;

    #[test]
    fn joint_record_collection_refuses_at_caller_limit() {
        let object = crate::native::ObjectRecord {
            id: "fcstd:native:object#Joint".into(),
            name: "Joint".into(),
            type_name: "App::FeaturePython".into(),
            persistent_id: None,
            view_type: None,
            attributes: Default::default(),
            dependencies: Vec::new(),
            dependency_allow_partial: None,
            order: 0,
            data: None,
        };
        let property = crate::native::PropertyRecord {
            id: "property".into(),
            owner: object.id.clone(),
            name: "ObjectToGround".into(),
            type_name: "App::PropertyLink".into(),
            family: crate::native::PropertyFamily::Unknown,
            status: None,
            body: crate::native::PropertyBody::Transient,
            order: 0,
            xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
                .expect("valid XML span"),
        };
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = 3;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        assert!(matches!(super::transfer(&ctx, &[object], &[property]),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "fcstd joint records"));
    }

    #[test]
    fn joint_native_identity_refuses_at_retained_limit() {
        let object = crate::native::ObjectRecord {
            id: "fcstd:native:object#Joint".into(),
            name: "Joint".into(),
            type_name: "App::FeaturePython".into(),
            persistent_id: None,
            view_type: None,
            attributes: Default::default(),
            dependencies: Vec::new(),
            dependency_allow_partial: None,
            order: 0,
            data: None,
        };
        let property = crate::native::PropertyRecord {
            id: "property".into(),
            owner: object.id.clone(),
            name: "ObjectToGround".into(),
            type_name: "App::PropertyLink".into(),
            family: crate::native::PropertyFamily::Unknown,
            status: None,
            body: crate::native::PropertyBody::Transient,
            order: 0,
            xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
                .expect("valid XML span"),
        };
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = crate::native::native_id("joint", &object.name).len() as u64 - 1;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        assert!(matches!(super::transfer(&ctx, &[object], &[property]),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "FreeCAD native identity"));
    }

    #[test]
    fn joint_model_identity_refuses_at_retained_limit() {
        let record = crate::native::joint::JointRecord::try_new(
            "fcstd:native:joint#Joint".into(),
            "fcstd:native:object#Joint".into(),
            crate::native::joint::JointBody::Grounded {
                reference: None,
                placement: Default::default(),
            },
            Default::default(),
        ).expect("grounded joint record");
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = crate::native::model_id(
            "joint", &record.object, "constraint").len() as u64 - 1;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        assert!(matches!(super::transfer_neutral(&ctx, &[record], &[]),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "FreeCAD model identity"));
    }
    use cadmpeg_ir::products::PairedJointKind;
    use cadmpeg_ir::{Codec, DecodeOptions};
    use cadmpeg_test_support::wire;
    use std::io::Cursor;

    const EPS_JOINT_SCALAR: f64 = 1.0e-12;

    #[test]
    fn every_primary_joint_family_has_a_neutral_variant() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
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
                        &ctx,
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
    fn neutral_joint_kind_refuses_at_materialized_limit() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        let kind = super::PairedJointFamily::new("Fixed".into()).expect("valid joint family");
        assert!(matches!(joint_kind(&ctx, &kind, None, None, None, None, None),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "fcstd joint kind matching"));
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
        assert_eq!(joints[0].references().len(), 2);
        assert_eq!(
            joints[0].references()[0].object(),
            Some("fcstd:native:object#Assembly")
        );
        assert_eq!(
            joints[0].references()[0].subelements(),
            ["A.Face1", "A.Edge2"]
        );
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
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect_err("joint with malformed numeric scalar is rejected");
        assert!(error.to_string().contains("invalid value"), "{error}");
    }

    #[test]
    fn primary_bool_values_are_retained_and_projected_exactly() {
        for (raw, expected) in [
            ("true", true),
            ("false", false),
            ("1", false),
            ("0", false),
            ("TRUE", false),
            ("maybe", false),
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
            assert_eq!(joints[0].parameters().raw("Suppressed"), Some(raw));
            assert_eq!(
                joints[0].parameters().bool_value("Suppressed"),
                Some(expected)
            );
            assert_eq!(result.ir().model.assembly_joints[0].suppressed, expected);
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
            assert_eq!(restored_joints[0].parameters().raw("Suppressed"), Some(raw));
            assert_eq!(restored.model.assembly_joints[0].suppressed, expected);
        }
    }
}
