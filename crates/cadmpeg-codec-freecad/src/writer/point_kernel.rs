// SPDX-License-Identifier: Apache-2.0
//! Encode point-kernel coordinates from current neutral points.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::transform::Transform;
use std::collections::{BTreeMap, HashMap, HashSet};

use super::xml::{self, Object};
use crate::loss::FreecadLossCode;
use crate::native::{PropertyBody, PropertyRecord};
use cadmpeg_ir::report::loss::LossNote;

pub(super) fn transform(property: &crate::native::PropertyRecord) -> Option<Transform> {
    let root = property
        .values()
        .iter()
        .find(|value| value.parent.is_none() && value.tag == "Points")?;
    let Some(value) = root.attributes.get("mtrx") else {
        return Some(Transform::identity());
    };
    let mut tokens = value.split_whitespace();
    let mut values = [0.0_f64; 16];
    for value in &mut values {
        *value = tokens.next()?.parse().ok()?;
    }
    if tokens.next().is_some() {
        return None;
    }
    if !values.iter().all(|value| value.is_finite()) {
        return None;
    }
    Transform::affine([
        [values[0], values[1], values[2], values[3]],
        [values[4], values[5], values[6], values[7]],
        [values[8], values[9], values[10], values[11]],
    ])
}

pub(super) struct PointKernel<'a> {
    pub(super) bytes: Vec<u8>,
    pub(super) transform: Transform,
    pub(super) included: Vec<&'a str>,
    pub(super) excluded: Vec<&'a str>,
}

pub(super) fn encode<'a>(
    ctx: &DecodeContext<'_>,
    points: &[&'a Point],
    transform: Transform,
) -> Result<Option<PointKernel<'a>>, CodecError> {
    if points.is_empty() {
        let mut bytes = Vec::new();
        ctx.reserve_working(&mut bytes, 4, "FreeCAD point-kernel bytes")?;
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        return Ok(Some(PointKernel {
            bytes,
            transform,
            included: Vec::new(),
            excluded: Vec::new(),
        }));
    }
    let (transform, inverse) = match transform.try_inverse_affine() {
        Ok(inverse) => (transform, inverse),
        Err(_) => (Transform::identity(), Transform::identity()),
    };
    if u32::try_from(points.len()).is_err() {
        return Ok(None);
    }
    let Some(capacity) = points
        .len()
        .checked_mul(12)
        .and_then(|bytes| bytes.checked_add(4))
    else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    ctx.reserve_working(&mut bytes, capacity, "FreeCAD point-kernel bytes")?;
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    let mut included = Vec::new();
    let mut excluded = Vec::new();
    let mut count = 0_u32;
    for point in points {
        let values = inverse
            .apply_point(point.position().get())
            .and_then(|local| {
                let local = local.get();
                Some([
                    cadmpeg_core::convert::f32_from_f64(local.x)?,
                    cadmpeg_core::convert::f32_from_f64(local.y)?,
                    cadmpeg_core::convert::f32_from_f64(local.z)?,
                ])
            });
        let exact = values.is_some_and(|values| {
            let local = values.map(f64::from);
            let placed = transform
                .affine_rows()
                .map(|row| row[0] * local[0] + row[1] * local[1] + row[2] * local[2] + row[3]);
            placed.iter().all(|value| value.is_finite())
                && Point3::new(placed[0], placed[1], placed[2]) == point.position().get()
        });
        if let Some(values) = values.filter(|_| exact) {
            ctx.reserve_working(&mut included, 1, "FreeCAD represented point identities")?;
            included.push(point.id.as_str());
            for value in values {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            count += 1;
        } else {
            ctx.reserve_working(&mut excluded, 1, "FreeCAD independent point identities")?;
            excluded.push(point.id.as_str());
        }
    }
    bytes[..4].copy_from_slice(&count.to_le_bytes());
    Ok(Some(PointKernel {
        bytes,
        transform,
        included,
        excluded,
    }))
}

fn exclude<'a>(
    ctx: &DecodeContext<'_>,
    points: impl IntoIterator<Item = &'a str>,
    excluded: &mut Vec<&'a str>,
    losses: &mut Vec<LossNote>,
    reason: &str,
) -> Result<(), CodecError> {
    for point in points {
        ctx.reserve_working(excluded, 1, "FreeCAD independent point identities")?;
        excluded.push(point);
        losses.push(FreecadLossCode::ExportGeometryNotTransferred.note(format!(
            "point {point} cannot retain its point-cloud association because {reason}; it is written as an exact vertex"
        )));
    }
    Ok(())
}

pub(super) fn write<'a>(
    ctx: &DecodeContext<'_>,
    ir: &'a cadmpeg_ir::CadIr,
    properties: &[PropertyRecord],
    names: &HashMap<String, String>,
    (entry_names, objects, entries): (
        &HashSet<String>,
        &mut [Object],
        &mut BTreeMap<String, Vec<u8>>,
    ),
    losses: &mut Vec<LossNote>,
) -> Result<Vec<&'a str>, CodecError> {
    let mut represented = Vec::new();
    let mut excluded = Vec::new();
    if !ir.scope.model {
        return Ok(represented);
    }
    for property in properties
        .iter()
        .filter(|property| property.type_name == "Points::PropertyPointKernel")
    {
        let Some(name) = names.get(&property.owner) else {
            continue;
        };
        let Some(object) = objects.iter_mut().find(|object| object.name == *name) else {
            continue;
        };
        let prefix = crate::native::model_id("point", &property.id, "");
        let siblings: Vec<_> = properties
            .iter()
            .filter(|other| other.owner == property.owner && other.type_name == property.type_name)
            .collect();
        let matching = |point: &&Point| {
            point.source_object.as_ref().is_some_and(|source| {
                source.format.as_str() == "fcstd"
                    && source.object_id.as_str() == property.owner
                    && source.instance_path.is_empty()
                    && (point
                        .id
                        .as_str()
                        .strip_prefix(&prefix)
                        .is_some_and(|index| index.parse::<usize>().is_ok())
                        || (siblings
                            .first()
                            .is_some_and(|first| first.id == property.id)
                            && !siblings.iter().any(|other| {
                                let other_prefix = crate::native::model_id("point", &other.id, "");
                                point
                                    .id
                                    .as_str()
                                    .strip_prefix(&other_prefix)
                                    .is_some_and(|index| index.parse::<usize>().is_ok())
                            })))
            })
        };
        let mut points = Vec::new();
        ctx.reserve_working(
            &mut points,
            ir.model.points.iter().filter(matching).count(),
            "FreeCAD point-kernel points",
        )?;
        points.extend(ir.model.points.iter().filter(matching));
        points.sort_by_key(|point| {
            let index = point
                .id
                .as_str()
                .strip_prefix(&prefix)
                .and_then(|index| index.parse::<usize>().ok());
            (index.is_none(), index)
        });
        let Some(original_file) = property
            .side_entries()
            .first()
            .filter(|_| property.side_entries().len() == 1)
        else {
            object.properties.shift_remove(&property.name);
            exclude(
                ctx,
                points.iter().copied().map(|point| point.id.as_str()),
                &mut excluded,
                losses,
                "the point-kernel side entry is absent",
            )?;
            continue;
        };
        let mut kernel_points = Vec::new();
        ctx.reserve_working(
            &mut kernel_points,
            points.len(),
            "FreeCAD point-kernel independent points",
        )?;
        for point in points.iter().copied() {
            if ir
                .model
                .vertices
                .iter()
                .any(|vertex| vertex.point == point.id)
            {
                exclude(
                    ctx,
                    [point.id.as_str()],
                    &mut excluded,
                    losses,
                    "the same point is used by a BRep vertex",
                )?;
            } else {
                kernel_points.push(point);
            }
        }
        let original_transform = transform(property);
        let Some(mut kernel) = encode(ctx, &kernel_points, original_transform.unwrap_or_default())?
        else {
            object.properties.shift_remove(&property.name);
            exclude(
                ctx,
                kernel_points.iter().copied().map(|point| point.id.as_str()),
                &mut excluded,
                losses,
                "the point-kernel population exceeds its binary representation",
            )?;
            continue;
        };
        if !kernel.excluded.is_empty() && kernel.transform != Transform::identity() {
            if let Some(identity) = encode(ctx, &kernel_points, Transform::identity())? {
                if identity.excluded.len() < kernel.excluded.len() {
                    kernel = identity;
                }
            }
        }
        let shared = properties
            .iter()
            .filter(|other| other.side_entries().contains(original_file))
            .count()
            != 1;
        let mut file = original_file.clone();
        if shared || entries.contains_key(&file) {
            let mut ordinal = 0_usize;
            loop {
                file = format!("PointCloud{ordinal}.bin");
                if !entry_names.contains(&file) && !entries.contains_key(&file) {
                    break;
                }
                let Some(next) = ordinal.checked_add(1) else {
                    object.properties.shift_remove(&property.name);
                    exclude(
                        ctx,
                        kernel_points.iter().copied().map(|point| point.id.as_str()),
                        &mut excluded,
                        losses,
                        "no distinct point-kernel entry name is available",
                    )?;
                    represented.sort_unstable();
                    represented.dedup();
                    return Ok(represented);
                };
                ordinal = next;
            }
        }
        let mut generated = property.clone();
        let Some(PropertyBody::Persisted {
            values,
            side_entries,
            ..
        }) = generated.body.as_mut()
        else {
            object.properties.shift_remove(&property.name);
            exclude(
                ctx,
                kernel_points.iter().copied().map(|point| point.id.as_str()),
                &mut excluded,
                losses,
                "the point-kernel property has no persisted value",
            )?;
            continue;
        };
        let Some(root) = values
            .iter_mut()
            .find(|value| value.parent.is_none() && value.tag == "Points")
        else {
            object.properties.shift_remove(&property.name);
            exclude(
                ctx,
                kernel_points.iter().copied().map(|point| point.id.as_str()),
                &mut excluded,
                losses,
                "the point-kernel value has no Points element",
            )?;
            continue;
        };
        root.attributes.insert("file".into(), file.clone());
        if original_transform != Some(kernel.transform) {
            root.attributes.insert(
                "mtrx".into(),
                kernel
                    .transform
                    .rows()
                    .iter()
                    .flatten()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        }
        side_entries.clear();
        side_entries.push(file.clone());
        let available = HashSet::from([file.clone()]);
        let property_xml = xml::native_named(&generated, names, &available)
            .ok()
            .flatten();
        let Some(property_xml) = property_xml else {
            object.properties.shift_remove(&property.name);
            exclude(
                ctx,
                kernel_points.iter().copied().map(|point| point.id.as_str()),
                &mut excluded,
                losses,
                "the point-kernel property cannot be emitted",
            )?;
            continue;
        };
        object.set(&property.name, property_xml);
        entries.insert(file, kernel.bytes);
        ctx.reserve_working(
            &mut represented,
            kernel.included.len(),
            "FreeCAD represented point identities",
        )?;
        represented.extend(kernel.included);
        exclude(
            ctx,
            kernel.excluded,
            &mut excluded,
            losses,
            "the current coordinate is not exactly representable in the binary32 kernel",
        )?;
    }
    represented.sort_unstable();
    represented.dedup();
    excluded.sort_unstable();
    excluded.dedup();
    for point in &ir.model.points {
        if point
            .source_object
            .as_ref()
            .is_some_and(|source| source.format.as_str() == "fcstd")
            && represented.binary_search(&point.id.as_str()).is_err()
            && excluded.binary_search(&point.id.as_str()).is_err()
            && !ir
                .model
                .vertices
                .iter()
                .any(|vertex| vertex.point == point.id)
        {
            losses.push(FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                "point {} cannot retain its point-cloud association because its source object has no writable point kernel; it is written as an exact independent vertex",
                point.id
            )));
        }
    }
    Ok(represented)
}
