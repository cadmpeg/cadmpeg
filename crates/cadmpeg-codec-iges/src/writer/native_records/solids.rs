// SPDX-License-Identifier: Apache-2.0
//! Types 150 through 168, 180, 182, 412 and 414 rebuilt from typed records.

use super::{entity, integer, real, Entity, Fields, Value};

pub(in crate::writer) fn primitive_feature(
    ir: &super::CadIr,
    feature: &cadmpeg_ir::features::Feature,
) -> Result<Entity, String> {
    use cadmpeg_ir::features::{
        BooleanOp, Dimension, FeatureDefinition, FeatureOperation, PrimitiveSolidKind,
        RadialDimension,
    };
    use cadmpeg_ir::scalar::{Angle, Length};
    use std::f64::consts::{FRAC_PI_2, PI, TAU};
    let FeatureDefinition::Operation(FeatureOperation::Primitive {
        solid,
        placement,
        op,
    }) = feature.evaluation.definition()
    else {
        return Err("linked feature is not a primitive solid".into());
    };
    if *op != BooleanOp::NewBody {
        return Err("primitive Boolean operation has no exact IGES primitive form".into());
    }
    let length = |value: &Dimension<Length>| {
        ir.dimension_value(value)
            .map(Length::get)
            .ok_or_else(|| "primitive length is unresolved".to_owned())
    };
    let radius = |value: &RadialDimension<Length>| {
        value
            .resolved_radius(ir)
            .map(Length::get)
            .ok_or_else(|| "primitive radius is unresolved".to_owned())
    };
    let angle = |value: &Dimension<Angle>, expected: f64| -> Result<(), String> {
        if ir
            .dimension_value(value)
            .is_some_and(|value| value.get() == expected)
        {
            Ok(())
        } else {
            Err("primitive angular bounds have no exact IGES primitive form".into())
        }
    };
    let (type_code, dimensions) = match solid.kind() {
        PrimitiveSolidKind::Box {
            length: x,
            width: y,
            height: z,
        } => (150, vec![length(x)?, length(y)?, length(z)?]),
        PrimitiveSolidKind::Cylinder {
            radius: r,
            height,
            angle: a,
        } => {
            angle(a, TAU)?;
            (154, vec![length(height)?, radius(r)?])
        }
        PrimitiveSolidKind::Cone {
            radius1,
            radius2,
            height,
            angle: a,
        } => {
            angle(a, TAU)?;
            (
                156,
                vec![length(height)?, radius(radius1)?, radius(radius2)?],
            )
        }
        PrimitiveSolidKind::Sphere {
            radius: r,
            latitude1,
            latitude2,
            longitude,
        } => {
            angle(latitude1, -FRAC_PI_2)?;
            angle(latitude2, FRAC_PI_2)?;
            angle(longitude, TAU)?;
            (158, vec![radius(r)?])
        }
        PrimitiveSolidKind::Torus {
            major_radius,
            minor_radius,
            latitude1,
            latitude2,
            longitude,
        } => {
            angle(latitude1, -PI)?;
            angle(latitude2, PI)?;
            angle(longitude, TAU)?;
            (160, vec![radius(major_radius)?, radius(minor_radius)?])
        }
        PrimitiveSolidKind::Ellipsoid {
            x_radius,
            y_radius,
            z_radius,
            latitude1,
            latitude2,
            longitude,
        } => {
            angle(latitude1, -FRAC_PI_2)?;
            angle(latitude2, FRAC_PI_2)?;
            angle(longitude, TAU)?;
            (
                168,
                vec![radius(x_radius)?, radius(y_radius)?, radius(z_radius)?],
            )
        }
        PrimitiveSolidKind::Wedge {
            xmin,
            ymin,
            zmin,
            x2min,
            z2min,
            xmax,
            ymax,
            zmax,
            x2max,
            z2max,
        } => {
            if [
                length(xmin)?,
                length(ymin)?,
                length(zmin)?,
                length(x2min)?,
                length(z2min)?,
            ]
            .iter()
            .any(|value| *value != 0.0)
                || length(z2max)? != length(zmax)?
            {
                return Err("wedge bounds have no exact IGES primitive form".into());
            }
            (
                152,
                vec![length(xmax)?, length(ymax)?, length(zmax)?, length(x2max)?],
            )
        }
        PrimitiveSolidKind::Prism { .. } => {
            return Err("prism has no exact IGES primitive form".into())
        }
    };
    let placement = placement
        .as_ref()
        .ok_or("primitive placement is unresolved")?;
    let affine = placement.affine_rows();
    let x = cadmpeg_ir::math::Vector3::new(affine[0][0], affine[1][0], affine[2][0]);
    let z = cadmpeg_ir::math::Vector3::new(affine[0][2], affine[1][2], affine[2][2]);
    let standard_z = cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0);
    let standard_x = cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0);
    let derived_x = if z.x.abs() < z.y.abs() {
        standard_x.cross(z).unit()
    } else {
        cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
            .cross(z)
            .unit()
    }
    .and_then(|x| z.cross(x).unit())
    .and_then(|y| y.cross(z).unit());
    let matches_axis = |left: cadmpeg_ir::math::Vector3, right: cadmpeg_ir::math::Vector3| {
        left.x == right.x && left.y == right.y && left.z == right.z
    };
    let transformed = match type_code {
        158 => !matches_axis(x, standard_x) || !matches_axis(z, standard_z),
        154 | 156 | 160 => derived_x.is_none_or(|derived| !matches_axis(x, derived)),
        _ => false,
    };
    let mut values = dimensions
        .into_iter()
        .map(|value| {
            super::FiniteReal::new(value)
                .map(super::number)
                .ok_or_else(|| "primitive dimension is non-finite".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let vector = |values: [f64; 3]| {
        values
            .into_iter()
            .map(|value| {
                super::FiniteReal::new(value)
                    .map(super::number)
                    .ok_or_else(|| "primitive placement is non-finite".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()
    };
    let origin = if transformed {
        [0.0, 0.0, 0.0]
    } else {
        [affine[0][3], affine[1][3], affine[2][3]]
    };
    values.extend(vector(origin)?);
    if matches!(type_code, 150 | 152 | 168) {
        values.extend(vector(if transformed {
            [1.0, 0.0, 0.0]
        } else {
            [x.x, x.y, x.z]
        })?);
    }
    if matches!(type_code, 150 | 152 | 154 | 156 | 160 | 168) {
        values.extend(vector(if transformed {
            [0.0, 0.0, 1.0]
        } else {
            [z.x, z.y, z.z]
        })?);
    }
    let mut output = entity(type_code, 0, values);
    if transformed {
        let mut rows = [[super::FiniteReal::ONE; 4]; 3];
        for (row, values) in rows.iter_mut().zip(affine) {
            for (slot, value) in row.iter_mut().zip(values) {
                *slot = super::FiniteReal::new(value).ok_or("primitive placement is non-finite")?;
            }
        }
        output.transform = Some(super::super::Placement { rows });
    }
    if let Some(label) = feature.name.as_deref().and_then(super::directory_label) {
        output.label = label;
    }
    output.source_targets.push(feature.id.to_string());
    Ok(output)
}

/// Write primitive features in their current ordinal order.
pub(in crate::writer) fn append_primitive_features(ir: &super::CadIr, entities: &mut Vec<Entity>) {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let native = ir
        .native
        .namespace("iges")
        .and_then(|namespace| namespace.arenas().get("primitive_solids"));
    let mut features = ir.model.features.iter().collect::<Vec<_>>();
    features.sort_by_key(|feature| feature.ordinal);
    for feature in features {
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Primitive { .. })
        ) {
            continue;
        }
        if let Ok(mut entity) = primitive_feature(ir, feature) {
            if let Some(record) = feature.native_ref.as_ref().and_then(|id| {
                native.and_then(|records| records.iter().find(|record| record.id() == id))
            }) {
                entity.source_targets.push(record.id().to_owned());
                if let Some(source) = record
                    .field("source_entity")
                    .and_then(|value| value.as_str().map(str::to_owned))
                {
                    entity.source_targets.push(source);
                }
            }
            entities.push(entity);
        }
    }
}

fn operand(fields: &mut Fields<'_>, term: &Value) -> Result<String, String> {
    let item = term
        .as_object()
        .ok_or("boolean operand has no typed fields")?;
    match item.get("entity") {
        // Boolean operands are negated Directory pointers.
        Some(value) if !value.is_null() => Ok(format!("-{}", fields.reference(Some(value))?)),
        _ => item
            .get("raw")
            .and_then(Value::as_i64)
            .map(|raw| raw.to_string())
            .ok_or("boolean operand has no raw pointer".into()),
    }
}

fn dimensions(dimensions: &Value, names: &[&str]) -> Result<Vec<String>, String> {
    let values = dimensions
        .as_object()
        .ok_or("primitive solid has no typed dimensions")?;
    names
        .iter()
        .map(|name| {
            real(Some(
                values
                    .get(*name)
                    .ok_or("primitive solid omits a dimension")?,
            ))
        })
        .collect()
}

fn axis(values: &[Value]) -> Result<Vec<String>, String> {
    values.iter().map(|value| real(Some(value))).collect()
}

pub(super) fn shape(arena: &str, record: &super::NativeRecord) -> Result<(u32, i64), String> {
    let form = || -> Result<i64, String> {
        record
            .field("form")
            .as_ref()
            .and_then(Value::as_i64)
            .ok_or("record has no typed form".into())
    };
    match arena {
        "primitive_solids" => {
            let kind_field = record.field("kind");
            let kind = kind_field
                .as_ref()
                .and_then(Value::as_str)
                .ok_or_else(|| "primitive solid has no typed kind".to_owned())?;
            let type_code = match kind {
                "block" => 150,
                "right_angular_wedge" => 152,
                "right_circular_cylinder" => 154,
                "right_circular_cone_frustum" => 156,
                "sphere" => 158,
                "torus" => 160,
                "ellipsoid" => 168,
                _ => return Err("primitive solid has no exact typed kind".into()),
            };
            Ok((type_code, 0))
        }
        "procedural_solids" => {
            let kind_field = record.field("kind");
            let kind = kind_field
                .as_ref()
                .and_then(Value::as_str)
                .ok_or_else(|| "procedural solid has no typed kind".to_owned())?;
            match kind {
                "revolution" => Ok((162, form()?)),
                "linear_extrusion" => Ok((164, form()?)),
                _ => Err("procedural solid has no exact typed kind".into()),
            }
        }
        "boolean_trees" => Ok((180, form()?)),
        "selected_components" => Ok((182, 0)),
        "rectangular_arrays" => Ok((412, 0)),
        "circular_arrays" => Ok((414, 0)),
        _ => Err("record is not a constructive solid or array".into()),
    }
}

pub(super) fn encode(
    arena: &str,
    fields: &mut Fields<'_>,
    type_code: u32,
    form: i64,
) -> Result<Entity, String> {
    match arena {
        "primitive_solids" => {
            let table = fields
                .value("dimensions")
                .cloned()
                .ok_or("primitive solid has no typed dimensions")?;
            let mut values = match type_code {
                150 => dimensions(&table, &["x_length", "y_length", "z_length"])?,
                152 => dimensions(
                    &table,
                    &["x_length", "y_length", "z_length", "top_x_length"],
                )?,
                154 => dimensions(&table, &["height", "radius"])?,
                156 => dimensions(&table, &["height", "large_radius", "small_radius"])?,
                158 => dimensions(&table, &["radius"])?,
                160 => dimensions(&table, &["major_radius", "minor_radius"])?,
                168 => dimensions(&table, &["x_radius", "y_radius", "z_radius"])?,
                _ => return Err("primitive solid has no exact typed kind".into()),
            };
            let origin = fields
                .value("origin")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 3)
                .cloned()
                .unwrap_or_default();
            values.extend(axis(&origin)?);
            for lane in ["x_axis", "z_axis"] {
                let present = match type_code {
                    150 | 152 | 168 => true,
                    154 | 156 | 160 => lane == "z_axis",
                    _ => false,
                };
                if !present {
                    continue;
                }
                match fields.value(lane).and_then(Value::as_array) {
                    Some(lane) if lane.len() == 3 => values.extend(axis(lane)?),
                    _ => values.extend([String::new(), String::new(), String::new()]),
                }
            }
            Ok(entity(type_code, form, values))
        }
        "procedural_solids" => {
            let profile = fields.value("profile").cloned();
            let mut values = vec![match profile {
                Some(value) if !value.is_null() => fields.reference(Some(&value))?,
                _ => "0".into(),
            }];
            values.push(real(fields.value("amount"))?);
            if type_code == 162 {
                let origin = fields
                    .value("origin")
                    .and_then(Value::as_array)
                    .filter(|values| values.len() == 3)
                    .cloned()
                    .unwrap_or_default();
                values.extend(axis(&origin)?);
            }
            let direction = fields
                .value("direction")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 3)
                .cloned()
                .unwrap_or_default();
            values.extend(axis(&direction)?);
            Ok(entity(type_code, form, values))
        }
        "boolean_trees" => {
            let mut values = vec![integer(fields.value("declared_length"))?];
            for term in fields.array("terms")? {
                let table = term.as_object().ok_or("boolean term has no typed fields")?;
                match table.get("kind").and_then(Value::as_str) {
                    Some("operand") => values.push(operand(fields, &term)?),
                    Some("operation") => values.push(integer(table.get("operation"))?),
                    _ => return Err("boolean term has no typed kind".into()),
                }
            }
            Ok(entity(type_code, form, values))
        }
        "selected_components" => {
            let tree = fields.value("boolean_tree").cloned();
            let mut values = vec![match tree {
                Some(value) if !value.is_null() => fields.reference(Some(&value))?,
                _ => "0".into(),
            }];
            let point = fields
                .value("selection_point")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 3)
                .cloned()
                .unwrap_or_default();
            values.extend(axis(&point)?);
            Ok(entity(type_code, form, values))
        }
        "rectangular_arrays" => {
            let base = fields.value("base").cloned();
            let mut values = vec![match base {
                Some(value) if !value.is_null() => fields.reference(Some(&value))?,
                _ => return Err("rectangular array has no typed base entity".into()),
            }];
            values.push(real(fields.value("scale"))?);
            let origin = fields
                .value("origin")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 3)
                .cloned()
                .unwrap_or_default();
            values.extend(axis(&origin)?);
            values.push(integer(fields.value("columns"))?);
            values.push(integer(fields.value("rows"))?);
            values.push(real(fields.value("column_spacing"))?);
            values.push(real(fields.value("row_spacing"))?);
            values.push(real(fields.value("rotation"))?);
            let positions = fields.array("positions")?;
            values.push(positions.len().to_string());
            values.push(integer(fields.value("do_dont_flag"))?);
            for position in &positions {
                values.push(integer(Some(position))?);
            }
            Ok(entity(type_code, form, values))
        }
        "circular_arrays" => {
            let base = fields.value("base").cloned();
            let mut values = vec![match base {
                Some(value) if !value.is_null() => fields.reference(Some(&value))?,
                _ => return Err("circular array has no typed base entity".into()),
            }];
            values.push(integer(fields.value("location_count"))?);
            let center = fields
                .value("center")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 3)
                .cloned()
                .unwrap_or_default();
            values.extend(axis(&center)?);
            values.push(real(fields.value("radius"))?);
            values.push(real(fields.value("start_angle"))?);
            values.push(real(fields.value("delta_angle"))?);
            let positions = fields.array("positions")?;
            values.push(positions.len().to_string());
            values.push(integer(fields.value("do_dont_flag"))?);
            for position in &positions {
                values.push(integer(Some(position))?);
            }
            Ok(entity(type_code, form, values))
        }
        _ => Err("record is not a constructive solid or array".into()),
    }
}

/// Apply a current neutral Boolean feature to its retained postfix tree.
pub(super) fn apply_combine(
    ir: &super::CadIr,
    record: &super::NativeRecord,
    fields: &mut Fields<'_>,
) -> Result<Option<String>, String> {
    use cadmpeg_ir::features::{BodySelection, BooleanKind, FeatureDefinition, FeatureOperation};
    let mut features = ir
        .model
        .features
        .iter()
        .filter(|feature| feature.native_ref.as_deref() == Some(record.id()));
    let Some(feature) = features.next() else {
        return Ok(None);
    };
    if features.next().is_some() {
        return Err("multiple features name one Boolean tree".into());
    }
    let FeatureDefinition::Operation(FeatureOperation::Combine { operands, op, .. }) =
        feature.evaluation.definition()
    else {
        return Err("current feature is not an IGES Boolean combination".into());
    };
    let selection = |value: &BodySelection| -> Result<Vec<String>, String> {
        match value {
            BodySelection::Native(value) => Ok(vec![value.clone()]),
            BodySelection::NativeSet(values) => Ok(values.as_slice().to_vec()),
            _ => Err("Boolean operand has no exact native entity selection".into()),
        }
    };
    let mut operands_in_order = selection(operands.target())?;
    if operands_in_order.len() != 1 {
        return Err("Boolean target must name one entity".into());
    }
    operands_in_order.extend(selection(operands.tools())?);
    for operand in &mut operands_in_order {
        if let Some(source) = ir
            .native
            .namespace("iges")
            .and_then(|namespace| {
                namespace
                    .arenas()
                    .values()
                    .flatten()
                    .find(|native| native.id() == operand.as_str())
            })
            .and_then(|native| native.field("source_entity"))
            .and_then(|value| value.as_str().map(str::to_owned))
        {
            *operand = source;
        }
    }
    let operation = match op {
        BooleanKind::Join => 1,
        BooleanKind::Cut => 2,
        BooleanKind::Intersect => 3,
    };
    let terms = fields
        .values
        .get_mut("terms")
        .and_then(Value::as_array_mut)
        .ok_or("Boolean tree has no typed terms")?;
    if terms
        .iter()
        .filter(|term| term.get("kind").and_then(Value::as_str) == Some("operand"))
        .count()
        != operands_in_order.len()
    {
        return Err("current operands cannot retain the Boolean tree's grouping".into());
    }
    let mut operands = operands_in_order.into_iter();
    for term in terms {
        let table = term
            .as_object_mut()
            .ok_or("Boolean term has no typed fields")?;
        match table.get("kind").and_then(Value::as_str) {
            Some("operand") => {
                table.insert(
                    "entity".into(),
                    Value::String(operands.next().expect("count was checked")),
                );
            }
            Some("operation") => {
                table.insert("operation".into(), Value::from(operation));
            }
            _ => return Err("Boolean tree contains an unsupported term".into()),
        }
    }
    Ok(Some(feature.id.to_string()))
}

/// Rebuild a rectangular array's grid from its current neutral pattern.
pub(super) fn apply_rectangular_pattern(
    ir: &super::CadIr,
    record: &super::NativeRecord,
    fields: &mut Fields<'_>,
) -> Result<Option<String>, String> {
    use cadmpeg_ir::features::patterns::PatternTransform;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let mut features = ir
        .model
        .features
        .iter()
        .filter(|feature| feature.native_ref.as_deref() == Some(record.id()));
    let Some(feature) = features.next() else {
        return Ok(None);
    };
    if features.next().is_some() {
        return Err("multiple features name one rectangular array".into());
    }
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) =
        feature.evaluation.definition()
    else {
        return Err("current feature is not a rectangular pattern".into());
    };
    if !seeds.is_empty() || fields.value("transformation").is_some() {
        return Err("rectangular pattern has unsupported seeds or placement".into());
    }
    let PatternTransform::Linear {
        direction: Some(direction),
        distance,
        count,
        second: Some(second),
        omitted_instances,
    } = pattern.definition()
    else {
        return Err("current pattern has no two-direction rectangular grid".into());
    };
    let columns = ir
        .dimension_value(count)
        .ok_or("rectangular column count is unresolved")?;
    let rows = ir
        .dimension_value(&second.count)
        .ok_or("rectangular row count is unresolved")?;
    let column_spacing = distance
        .resolved_spacing(ir, count)
        .ok_or("rectangular column spacing is unresolved")?
        .get();
    let row_spacing = second
        .distance
        .resolved_spacing(ir, &second.count)
        .ok_or("rectangular row spacing is unresolved")?
        .get();
    let first = direction.get();
    let other = second.direction.get();
    let rotation = first.y.atan2(first.x);
    let (sin, cos) = rotation.sin_cos();
    if first.z != 0.0
        || other.z != 0.0
        || first.x != cos
        || first.y != sin
        || other.x != -sin
        || other.y != cos
    {
        return Err("rectangular directions have no exact IGES rotation".into());
    }
    let flag = fields
        .value("do_dont_flag")
        .and_then(Value::as_i64)
        .filter(|flag| matches!(flag, 0 | 1))
        .ok_or("rectangular array has no admitted selection flag")?;
    let mut omitted = std::collections::BTreeSet::new();
    for [column, row] in omitted_instances {
        if *column >= columns || *row >= rows {
            return Err("rectangular omitted instance is outside the grid".into());
        }
        let position = u64::from(*column)
            .checked_mul(u64::from(rows))
            .and_then(|value| value.checked_add(u64::from(*row)))
            .and_then(|value| value.checked_add(1))
            .ok_or("rectangular position overflows")?;
        omitted.insert(position);
    }
    let total = u64::from(columns) * u64::from(rows);
    let omitted_count =
        u64::try_from(omitted.len()).map_err(|_| "rectangular mask count overflows")?;
    let mut positions = Vec::new();
    if !omitted.is_empty() {
        let count = if flag == 0 {
            total - omitted_count
        } else {
            omitted_count
        };
        if (flag == 0 && count > total / 2) || (flag == 1 && count < total.div_ceil(2)) {
            return Err("rectangular omission mask has no admitted IGES representation".into());
        }
        let capacity = usize::try_from(count).map_err(|_| "rectangular mask count overflows")?;
        positions
            .try_reserve_exact(capacity)
            .map_err(|_| "rectangular mask allocation failed")?;
        if flag == 0 {
            for position in 1..=total {
                if !omitted.contains(&position) {
                    positions.push(Value::from(i64::try_from(position).map_err(|_| {
                        "rectangular position overflows the IGES integer field"
                    })?));
                }
            }
        } else {
            for position in omitted {
                positions.push(Value::from(i64::try_from(position).map_err(|_| {
                    "rectangular position overflows the IGES integer field"
                })?));
            }
        }
    }
    fields.values.insert("columns".into(), Value::from(columns));
    fields.values.insert("rows".into(), Value::from(rows));
    fields
        .values
        .insert("column_spacing".into(), serde_json::json!(column_spacing));
    fields
        .values
        .insert("row_spacing".into(), serde_json::json!(row_spacing));
    fields
        .values
        .insert("rotation".into(), serde_json::json!(rotation));
    fields
        .values
        .insert("do_dont_flag".into(), Value::from(flag));
    fields
        .values
        .insert("positions".into(), Value::Array(positions));
    Ok(Some(feature.id.to_string()))
}
