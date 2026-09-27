// SPDX-License-Identifier: Apache-2.0
//! Curve-from-equation feature transfer and assignment parameter ordering.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, SolvedCurveGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::{
    features::{
        DesignParameter, Feature, FeatureDefinition as IrFeatureDefinition,
        FeatureId as IrFeatureId, FeatureOperation as IrFeatureOperation, FeatureSourceContent,
        ParameterId, ParameterValue,
    },
    scalar::Length,
};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::container::ContainerScan;

use super::coverage::source_section;
use super::native::annotate;

pub(super) fn curve_expression_record_id(record: &crate::curve::CurveExpressionRecord) -> String {
    format!(
        "creo:depdb:curve_expression#{}-{}-{}",
        if record.backup { "backup" } else { "active" },
        record.entity_id,
        record.offset
    )
}

const EPS_HELIX_BASIS_LENGTH: f64 = 1.0e-12;
const EPS_HELIX_BASIS_ORIGIN: f64 = 1.0e-12;
const EPS_HELIX_UV_EQUAL: f64 = 1.0e-9;
const EPS_HELIX_UV_ORTHO: f64 = 1.0e-9;

type CurveExpressionParameterOrder = (Vec<u32>, HashSet<(usize, usize)>);

fn curve_expression_helix_definition(
    record: &crate::curve::CurveExpressionRecord,
) -> Option<ProceduralCurveDefinition> {
    let helix = crate::curve::expression_helix(record)?;
    let slots = record.local_system.as_ref()?.explicit_slots?.get();
    let u = Vector3::new(slots[0], slots[1], slots[2]);
    let v = Vector3::new(slots[6], slots[7], slots[8]);
    let u_norm = u.norm();
    let v_norm = v.norm();
    let scale = u_norm.max(v_norm).max(1.0);
    if !u_norm.is_finite()
        || !v_norm.is_finite()
        || u_norm <= EPS_HELIX_BASIS_LENGTH
        || v_norm <= EPS_HELIX_BASIS_LENGTH
        || (u_norm - v_norm).abs() > EPS_HELIX_UV_EQUAL * scale
        || (u.x * v.x + u.y * v.y + u.z * v.z).abs() > EPS_HELIX_UV_ORTHO * u_norm * v_norm
        || slots[3..6]
            .iter()
            .any(|value| value.abs() > EPS_HELIX_BASIS_ORIGIN)
    {
        return None;
    }
    let u = Vector3::new(u.x / u_norm, u.y / u_norm, u.z / u_norm);
    let v = Vector3::new(v.x / v_norm, v.y / v_norm, v.z / v_norm);
    let axis = Vector3::new(
        u.y * v.z - u.z * v.y,
        u.z * v.x - u.x * v.z,
        u.x * v.y - u.y * v.x,
    );
    let origin = Point3::new(slots[9], slots[10], slots[11]);
    let (sin, cos) = helix.start_angle.get().sin_cos();
    let major_direction = Vector3::new(
        u.x * cos + v.x * sin,
        u.y * cos + v.y * sin,
        u.z * cos + v.z * sin,
    );
    let tangent_direction = Vector3::new(
        -u.x * sin + v.x * cos,
        -u.y * sin + v.y * cos,
        -u.z * sin + v.z * cos,
    );
    let minor_direction = if helix.clockwise {
        Vector3::new(
            -tangent_direction.x,
            -tangent_direction.y,
            -tangent_direction.z,
        )
    } else {
        tangent_direction
    };
    Some(ProceduralCurveDefinition::Helix(
        cadmpeg_ir::geometry::HelixCurveConstruction::try_new(
            [0.0, helix.revolutions.get() * std::f64::consts::TAU],
            cadmpeg_ir::geometry::HelixFrame {
                center: Point3::new(
                    origin.x + axis.x * helix.z_start,
                    origin.y + axis.y * helix.z_start,
                    origin.z + axis.z * helix.z_start,
                ),
                major: Vector3::new(
                    major_direction.x * helix.radius.get(),
                    major_direction.y * helix.radius.get(),
                    major_direction.z * helix.radius.get(),
                ),
                minor: Vector3::new(
                    minor_direction.x * helix.radius.get(),
                    minor_direction.y * helix.radius.get(),
                    minor_direction.z * helix.radius.get(),
                ),
                pitch: Vector3::new(
                    axis.x * helix.height / helix.revolutions.get(),
                    axis.y * helix.height / helix.revolutions.get(),
                    axis.z * helix.height / helix.revolutions.get(),
                ),
                axis,
            },
            0.0,
            None,
        )
        .ok()?,
    ))
}

fn curve_expression_helix_feature_definition(
    helix: &crate::curve::CurveExpressionHelix,
    procedural: &ProceduralCurveDefinition,
) -> Option<IrFeatureDefinition> {
    let ProceduralCurveDefinition::Helix(helix_payload) = procedural else {
        return None;
    };
    let pitch = helix_payload.pitch();
    let axis = helix_payload.axis();

    let axial_pitch = pitch.x * axis.x + pitch.y * axis.y + pitch.z * axis.z;
    let pitch = cadmpeg_ir::scalar::NonZeroLength::new(axial_pitch)?;
    Some(IrFeatureDefinition::Operation(IrFeatureOperation::Helix {
        axis_origin: *helix_payload.center(),
        axis_direction: cadmpeg_ir::features::FeatureDirection3::new(axis.get())?,
        radius: helix.radius,
        shape: cadmpeg_ir::features::HelixShape::Cylindrical { pitch },
        revolutions: helix.revolutions,
        start_angle: helix.start_angle,
        clockwise: helix.clockwise,
        segment_turns: None,
        construction_style: None,
    }))
}

fn expression_dependency_reaches(
    ctx: &DecodeContext<'_>,
    dependencies: &[Vec<usize>],
    start: usize,
    target: usize,
) -> Result<bool, CodecError> {
    let mut pending = ctx.alloc_filled(1, start, "creo curve-expression pending dependency")?;
    let mut visited = ctx.alloc_filled(
        dependencies.len(),
        false,
        "creo curve-expression visited dependencies",
    )?;
    while let Some(index) = pending.pop() {
        ctx.charge_work(1, "walk Creo curve-expression dependencies")?;
        if index == target {
            return Ok(true);
        }
        if !visited[index] {
            visited[index] = true;
            ctx.try_reserve_items(
                &mut pending,
                dependencies[index].len(),
                "creo curve-expression pending dependencies",
            )?;
            pending.extend(dependencies[index].iter().copied());
        }
    }
    Ok(false)
}

fn curve_expression_parameter_order(
    ctx: &DecodeContext<'_>,
    record: &crate::curve::CurveExpressionRecord,
    unique_assignment_indices: &BTreeMap<String, usize>,
) -> Result<Option<CurveExpressionParameterOrder>, CodecError> {
    let mut dependencies = ctx.alloc_filled(
        record.assignments.len(),
        Vec::new(),
        "creo curve-expression dependency rows",
    )?;
    for (row, assignment) in dependencies.iter_mut().zip(&record.assignments) {
        for name in &assignment.dependencies {
            let (mut key, _reservation) =
                ctx.copy_scoped_text(name, "creo curve-expression ordering lookup")?;
            key.make_ascii_lowercase();
            let Some(&index) = unique_assignment_indices.get(&key) else {
                continue;
            };
            if row.contains(&index) {
                continue;
            }
            ctx.try_reserve_items(row, 1, "creo curve-expression dependency indices")?;
            row.push(index);
        }
    }
    let mut cyclic_edges = HashSet::new();
    for (consumer, dependency_indices) in dependencies.iter().enumerate() {
        for &dependency in dependency_indices {
            if expression_dependency_reaches(ctx, &dependencies, dependency, consumer)? {
                ctx.try_collection(1, "creo curve-expression cyclic edges", || {
                    cyclic_edges.try_reserve(1)
                })?;
                cyclic_edges.insert((consumer, dependency));
            }
        }
    }
    let mut ordinals = ctx.alloc_filled(
        dependencies.len(),
        0u32,
        "creo curve-expression parameter ordinals",
    )?;
    let mut assigned = ctx.alloc_filled(
        dependencies.len(),
        false,
        "creo curve-expression assigned ordinals",
    )?;
    for ordinal in 0..dependencies.len() {
        let Some(index) = (0..dependencies.len()).find(|&candidate| {
            !assigned[candidate]
                && dependencies[candidate].iter().all(|dependency| {
                    cyclic_edges.contains(&(candidate, *dependency)) || assigned[*dependency]
                })
        }) else {
            return Ok(None);
        };
        let Ok(ordinal) = u32::try_from(ordinal) else {
            return Ok(None);
        };
        ordinals[index] = ordinal;
        assigned[index] = true;
    }
    Ok(Some((ordinals, cyclic_edges)))
}

fn curve_expression_parameter_names(
    ctx: &DecodeContext<'_>,
    assignments: &[crate::curve::CurveExpressionAssignment],
) -> Result<Vec<Option<String>>, CodecError> {
    let mut counts = BTreeMap::new();
    for assignment in assignments {
        if let Some((name, _)) = assignment.parameter_target() {
            let mut key = ctx.copy_retained_text(name, "creo curve-expression name key")?;
            key.make_ascii_lowercase();
            match counts.entry(key) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    ctx.charge_collection_items(1, "creo curve-expression unique names")?;
                    entry.insert(1usize);
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    *entry.get_mut() += 1;
                }
            }
        }
    }
    let mut occurrences = BTreeMap::new();
    let mut names = Vec::new();
    for assignment in assignments {
        let name = if let Some((name, _)) = assignment.parameter_target() {
            let mut key = ctx.copy_retained_text(name, "creo curve-expression occurrence key")?;
            key.make_ascii_lowercase();
            if counts[&key] == 1 {
                Some(ctx.copy_retained_text(name, "creo curve-expression parameter name")?)
            } else {
                let occurrence = match occurrences.entry(key) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        ctx.charge_collection_items(1, "creo curve-expression occurrences")?;
                        entry.insert(0usize)
                    }
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                };
                *occurrence += 1;
                let mut output =
                    ctx.copy_retained_text(name, "creo curve-expression parameter name")?;
                let mut digits = [0u8; 20];
                let mut value = *occurrence;
                let mut width = 0;
                loop {
                    digits[width] = (value % 10) as u8;
                    width += 1;
                    value /= 10;
                    if value == 0 {
                        break;
                    }
                }
                ctx.try_reserve_retained_text(
                    &mut output,
                    width + 1,
                    "creo curve-expression parameter suffix",
                )?;
                output.push('#');
                for &digit in digits[..width].iter().rev() {
                    output.push(char::from(b'0' + digit));
                }
                Some(output)
            }
        } else {
            None
        };
        ctx.try_reserve_items(&mut names, 1, "creo curve-expression parameter name slots")?;
        names.push(name);
    }
    Ok(names)
}

fn curve_expression_assignment_indices(
    ctx: &DecodeContext<'_>,
    record: &crate::curve::CurveExpressionRecord,
) -> Result<(BTreeMap<String, Option<usize>>, BTreeMap<String, usize>), CodecError> {
    let mut by_name = BTreeMap::<String, Option<usize>>::new();
    for (ordinal, assignment) in record.assignments.iter().enumerate() {
        if assignment.activation == crate::curve::CurveExpressionActivation::Inactive {
            continue;
        }
        let Some((name, _)) = assignment.parameter_target() else {
            continue;
        };
        let mut key = ctx.copy_retained_text(name, "creo curve-expression assignment key")?;
        key.make_ascii_lowercase();
        match by_name.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "creo curve-expression assignment indices")?;
                entry.insert(Some(ordinal));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                *entry.get_mut() = None;
            }
        }
    }
    let mut unique = BTreeMap::new();
    for (name, index) in &by_name {
        if let Some(index) = index {
            let key = ctx.copy_retained_text(name, "creo curve-expression unique key")?;
            ctx.charge_collection_items(1, "creo curve-expression unique indices")?;
            unique.insert(key, *index);
        }
    }
    Ok((by_name, unique))
}

fn curve_expression_emitted_ordinals(
    ctx: &DecodeContext<'_>,
    record: &crate::curve::CurveExpressionRecord,
    parameter_ordinals: &[u32],
) -> Result<BTreeMap<usize, u32>, CodecError> {
    let count = record
        .assignments
        .iter()
        .filter(|assignment| assignment.parameter_target().is_some())
        .count();
    let mut indices = Vec::new();
    ctx.try_reserve_items(&mut indices, count, "creo curve-expression emitted indices")?;
    for (index, assignment) in record.assignments.iter().enumerate() {
        if assignment.parameter_target().is_some() {
            indices.push(index);
        }
    }
    indices.sort_by_key(|index| parameter_ordinals[*index]);
    let mut emitted = BTreeMap::new();
    for (ordinal, index) in indices.into_iter().enumerate() {
        let ordinal = u32::try_from(ordinal)
            .map_err(|_| CodecError::malformed("curve expression parameter ordinal exceeds u32"))?;
        ctx.charge_collection_items(1, "creo curve-expression emitted ordinals")?;
        emitted.insert(index, ordinal);
    }
    Ok(emitted)
}

fn joined_dependency_names(
    ctx: &DecodeContext<'_>,
    names: &[String],
    include: impl Fn(&str) -> Result<bool, CodecError>,
    operation: &'static str,
) -> Result<Option<String>, CodecError> {
    let mut count = 0usize;
    let mut bytes = 0usize;
    for name in names {
        if include(name)? {
            bytes = bytes
                .checked_add(name.len())
                .ok_or_else(|| ctx.refuse_codec_limit(operation, usize::MAX as u64, u64::MAX))?;
            count += 1;
        }
    }
    if count == 0 {
        return Ok(None);
    }
    bytes = bytes
        .checked_add(count - 1)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, usize::MAX as u64, u64::MAX))?;
    let mut joined = String::new();
    ctx.try_reserve_retained_text(&mut joined, bytes, operation)?;
    let mut emitted = false;
    for name in names {
        if include(name)? {
            if emitted {
                joined.push(',');
            }
            joined.push_str(name);
            emitted = true;
        }
    }
    Ok(Some(joined))
}

fn insert_curve_expression_property(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<String, String>,
    name: &'static str,
    value: String,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, "creo curve-expression property nodes")?;
    let key = ctx.copy_retained_text(name, "creo curve-expression property key")?;
    properties.insert(key, value);
    Ok(())
}

fn join_cyclic_dependency_names(
    ctx: &DecodeContext<'_>,
    names: &[&str],
) -> Result<String, CodecError> {
    let bytes = names
        .iter()
        .try_fold(0usize, |total, name| total.checked_add(name.len()))
        .and_then(|total| total.checked_add(if names.is_empty() { 0 } else { names.len() - 1 }))
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "creo curve-expression cyclic dependency text",
                usize::MAX as u64,
                u64::MAX,
            )
        })?;
    let mut joined = String::new();
    ctx.try_reserve_retained_text(
        &mut joined,
        bytes,
        "creo curve-expression cyclic dependency text",
    )?;
    for (index, name) in names.iter().enumerate() {
        if index > 0 {
            joined.push(',');
        }
        joined.push_str(name);
    }
    Ok(joined)
}

fn curve_expression_source_text(
    ctx: &DecodeContext<'_>,
    lines: &[crate::curve::CurveExpressionLine],
) -> Result<String, CodecError> {
    let bytes = lines
        .iter()
        .try_fold(0usize, |total, line| total.checked_add(line.text.len()))
        .and_then(|total| total.checked_add(if lines.is_empty() { 0 } else { lines.len() - 1 }))
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "creo curve-expression feature source text",
                usize::MAX as u64,
                u64::MAX,
            )
        })?;
    let mut text = String::new();
    ctx.try_reserve_retained_text(
        &mut text,
        bytes,
        "creo curve-expression feature source text",
    )?;
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            text.push('\n');
        }
        text.push_str(&line.text);
    }
    Ok(text)
}

fn curve_expression_properties(
    ctx: &DecodeContext<'_>,
    assignment: &crate::curve::CurveExpressionAssignment,
    assignment_ordinal: usize,
    parameter_name: &str,
    parameter_id: &ParameterId,
    assignment_indices_by_name: &BTreeMap<String, Option<usize>>,
    unique_assignment_indices: &BTreeMap<String, usize>,
    dimension_parameters: &BTreeMap<String, ParameterId>,
    cyclic_edges: &HashSet<(usize, usize)>,
) -> Result<BTreeMap<cadmpeg_core::text::NonBlankString, String>, CodecError> {
    let Some((assignment_name, declared_unit)) = assignment.parameter_target() else {
        return Err(CodecError::malformed(
            "curve expression assignment has no parameter target",
        ));
    };
    let external_dependencies = joined_dependency_names(
        ctx,
        &assignment.dependencies,
        |name| {
            let (mut key, _reservation) =
                ctx.copy_scoped_text(name, "creo curve-expression external lookup")?;
            key.make_ascii_lowercase();
            Ok(key != "t"
                && !assignment_indices_by_name.contains_key(&key)
                && !dimension_parameters.contains_key(&key))
        },
        "creo curve-expression external dependency text",
    )?;
    let ambiguous_dependencies = joined_dependency_names(
        ctx,
        &assignment.dependencies,
        |name| {
            let (mut key, _reservation) =
                ctx.copy_scoped_text(name, "creo curve-expression ambiguous lookup")?;
            key.make_ascii_lowercase();
            Ok(matches!(assignment_indices_by_name.get(&key), Some(None)))
        },
        "creo curve-expression ambiguous dependency text",
    )?;
    let intrinsic_dependencies = joined_dependency_names(
        ctx,
        &assignment.dependencies,
        |name| Ok(name.eq_ignore_ascii_case("t")),
        "creo curve-expression intrinsic dependency text",
    )?;
    let mut properties = BTreeMap::new();
    if let Some(value) = external_dependencies {
        insert_curve_expression_property(ctx, &mut properties, "external_dependencies", value)?;
    }
    if let Some(value) = ambiguous_dependencies {
        insert_curve_expression_property(ctx, &mut properties, "ambiguous_dependencies", value)?;
    }
    insert_curve_expression_property(
        ctx,
        &mut properties,
        "source_assignment_ordinal",
        assignment_ordinal.to_string(),
    )?;
    insert_curve_expression_property(
        ctx,
        &mut properties,
        "activation",
        assignment.activation.token().to_string(),
    )?;
    if let Some(unit) = declared_unit {
        let unit = ctx.copy_retained_text(unit, "creo curve-expression declared unit")?;
        insert_curve_expression_property(ctx, &mut properties, "declared_unit", unit)?;
    }
    if let Some(crate::curve::CurveExpressionValue::Quantity(quantity)) = &assignment.value {
        insert_curve_expression_property(
            ctx,
            &mut properties,
            "evaluated_canonical_value",
            quantity.value.to_string(),
        )?;
        insert_curve_expression_property(
            ctx,
            &mut properties,
            "evaluated_dimension",
            format!(
                "length:{},mass:{},time:{},angle:{},temperature:{}",
                quantity.length_power,
                quantity.mass_power,
                quantity.time_power,
                quantity.angle_power,
                quantity.temperature_power
            ),
        )?;
    }
    if parameter_name != assignment_name {
        let source_name =
            ctx.copy_retained_text(assignment_name, "creo curve-expression source name")?;
        insert_curve_expression_property(ctx, &mut properties, "source_name", source_name)?;
    }
    if let Some(value) = intrinsic_dependencies {
        insert_curve_expression_property(ctx, &mut properties, "independent_variables", value)?;
    }
    let mut cyclic_dependencies = Vec::new();
    for name in &assignment.dependencies {
        let (mut key, _reservation) =
            ctx.copy_scoped_text(name, "creo curve-expression cyclic lookup")?;
        key.make_ascii_lowercase();
        if unique_assignment_indices
            .get(&key)
            .is_some_and(|dependency| cyclic_edges.contains(&(assignment_ordinal, *dependency)))
        {
            ctx.try_reserve_items(
                &mut cyclic_dependencies,
                1,
                "creo curve-expression cyclic dependency names",
            )?;
            cyclic_dependencies.push(name.as_str());
        }
    }
    cyclic_dependencies.sort_unstable();
    cyclic_dependencies.dedup();
    if !cyclic_dependencies.is_empty() {
        let value = join_cyclic_dependency_names(ctx, &cyclic_dependencies)?;
        insert_curve_expression_property(ctx, &mut properties, "cyclic_dependencies", value)?;
    }
    ctx.charge_collection_items(
        properties.len() as u64,
        "creo curve-expression named properties",
    )?;
    Ok(cadmpeg_core::text::named_entries(
        parameter_id.as_str(),
        properties,
    )?)
}

pub(super) fn transfer_curve_expression_features(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    dimension_parameters: &BTreeMap<String, ParameterId>,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let ordinal_base = ir
        .model
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .max()
        .map_or(0, |value| value + 1);
    let mut transferred_parameter_count = 0;
    for (expression_ordinal, record) in scan
        .curves
        .expressions
        .iter()
        .filter(|record| !record.backup)
        .enumerate()
    {
        let source_section = source_section(scan, record.offset);
        let ordinal = ordinal_base + expression_ordinal as u64;
        let feature_id = IrFeatureId::compose(
            &crate::identity::DEPDB_CURVE_EXPRESSION_FEATURE,
            cadmpeg_ir::ids::IdentityKey::from(record.entity_id).dash(record.offset),
        );
        let (assignment_indices_by_name, unique_assignment_indices) =
            curve_expression_assignment_indices(ctx, record)?;
        let Some((parameter_ordinals, cyclic_edges)) =
            curve_expression_parameter_order(ctx, record, &unique_assignment_indices)?
        else {
            continue;
        };
        let parameter_names = curve_expression_parameter_names(ctx, &record.assignments)?;
        let emitted_ordinals = curve_expression_emitted_ordinals(ctx, record, &parameter_ordinals)?;
        let mut source_content = Vec::new();
        ctx.try_reserve_items(
            &mut source_content,
            emitted_ordinals.len(),
            "creo curve-expression source content",
        )?;
        let parameter_start = ir.model.parameters.len();
        for (assignment_ordinal, assignment) in record.assignments.iter().enumerate() {
            let Some((_assignment_name, _declared_unit)) = assignment.parameter_target() else {
                continue;
            };
            let Some(&ordinal) = emitted_ordinals.get(&assignment_ordinal) else {
                continue;
            };
            let Some(parameter_name) = parameter_names
                .get(assignment_ordinal)
                .and_then(Option::as_ref)
            else {
                return Err(cadmpeg_core::CodecError::malformed(format!(
                    "curve expression record {} assignment {} has no parameter name",
                    record.entity_id, assignment_ordinal
                )));
            };
            let parameter_id = ParameterId::compose(
                &crate::identity::DEPDB_CURVE_EXPRESSION_PARAMETER,
                cadmpeg_ir::ids::IdentityKey::from(record.entity_id)
                    .dash(record.offset)
                    .dash(assignment_ordinal),
            );
            let mut seen = BTreeSet::new();
            let mut dependencies = Vec::new();
            let mut dimension_dependencies = Vec::new();
            for name in &assignment.dependencies {
                let (mut key, _key_reservation) =
                    ctx.copy_scoped_text(name, "creo curve-expression dependency key")?;
                key.make_ascii_lowercase();
                if let Some(&dependency) = unique_assignment_indices.get(&key) {
                    if cyclic_edges.contains(&(assignment_ordinal, dependency))
                        || seen.contains(&dependency)
                    {
                        continue;
                    }
                    ctx.charge_collection_items(1, "creo curve-expression seen dependencies")?;
                    seen.insert(dependency);
                    ctx.try_reserve_items(
                        &mut dependencies,
                        1,
                        "creo curve-expression parameter dependencies",
                    )?;
                    dependencies.push(ParameterId::compose(
                        &crate::identity::DEPDB_CURVE_EXPRESSION_PARAMETER,
                        cadmpeg_ir::ids::IdentityKey::from(record.entity_id)
                            .dash(record.offset)
                            .dash(dependency),
                    ));
                }
                if assignment_indices_by_name.contains_key(&key) {
                    continue;
                }
                let Some(parameter) = dimension_parameters.get(&key) else {
                    continue;
                };
                if dependencies.contains(parameter) || dimension_dependencies.contains(&parameter) {
                    continue;
                }
                ctx.try_reserve_items(
                    &mut dimension_dependencies,
                    1,
                    "creo curve-expression dimension candidates",
                )?;
                dimension_dependencies.push(parameter);
            }
            for parameter in dimension_dependencies {
                ctx.try_reserve_items(
                    &mut dependencies,
                    1,
                    "creo curve-expression dimension dependencies",
                )?;
                let copied = ctx.copy_retained_text(
                    parameter.as_str(),
                    "creo curve-expression dimension parameter id",
                )?;
                let copied = ParameterId::try_from(copied).map_err(|_| {
                    CodecError::malformed("curve expression dimension parameter id is invalid")
                })?;
                dependencies.push(copied);
            }
            annotate(
                annotations,
                parameter_id.as_str(),
                &source_section,
                assignment.offset as u64,
                "curve_expression_assignment",
                Exactness::Derived,
            );
            ctx.charge_entities(1, "admit Creo model parameters")?;
            for prior_members in 0..dependencies.len() {
                ctx.charge_work(
                    prior_members as u64,
                    "validate Creo curve-expression dependency uniqueness",
                )?;
            }
            let value = match assignment.value.as_ref() {
                Some(crate::curve::CurveExpressionValue::String(value)) => {
                    Some(ParameterValue::String(ctx.copy_retained_text(
                        value,
                        "creo curve-expression parameter string value",
                    )?))
                }
                other => other.and_then(|value| match value {
                    crate::curve::CurveExpressionValue::Number(value) => Some(
                        ParameterValue::Real(cadmpeg_ir::scalar::FiniteReal::new(*value)?),
                    ),
                    crate::curve::CurveExpressionValue::Length(value) => Some(
                        ParameterValue::Length(cadmpeg_ir::scalar::Length::new(*value)?),
                    ),
                    crate::curve::CurveExpressionValue::Angle(value) => Some(
                        ParameterValue::Angle(cadmpeg_ir::scalar::Angle::new(value.to_radians())?),
                    ),
                    crate::curve::CurveExpressionValue::Quantity(_)
                    | crate::curve::CurveExpressionValue::String(_) => None,
                }),
            };
            source_carriers.admit_parameter(
                ir,
                DesignParameter {
                    id: parameter_id.clone(),
                    owner: Some(feature_id.clone()),
                    ordinal,
                    name: ctx.copy_retained_text(
                        parameter_name,
                        "creo curve-expression IR parameter name",
                    )?,
                    expression: ctx.copy_retained_text(
                        &assignment.expression,
                        "creo curve-expression IR expression",
                    )?,
                    display: None,
                    value,
                    dependencies: cadmpeg_ir::features::DistinctMembers::try_from_reserved_vec(
                        dependencies,
                    )
                    .map_err(CodecError::malformed)?,
                    properties: BTreeMap::new(),
                    pmi: None,
                    native_ref: Some(curve_expression_record_id(record)),
                },
            )?;
            transferred_parameter_count += 1;
            source_content.push(FeatureSourceContent::Parameter(parameter_id.clone()));
        }
        let mut parameter_index = parameter_start;
        for (assignment_ordinal, assignment) in record.assignments.iter().enumerate() {
            if assignment.parameter_target().is_none()
                || !emitted_ordinals.contains_key(&assignment_ordinal)
            {
                continue;
            }
            let parameter = ir
                .model
                .parameters
                .get_mut(parameter_index)
                .ok_or_else(|| {
                    CodecError::malformed("curve-expression parameter sequence is incomplete")
                })?;
            parameter.properties = curve_expression_properties(
                ctx,
                assignment,
                assignment_ordinal,
                &parameter.name,
                &parameter.id,
                &assignment_indices_by_name,
                &unique_assignment_indices,
                dimension_parameters,
                &cyclic_edges,
            )?;
            parameter_index += 1;
        }
        annotate(
            annotations,
            feature_id.as_str(),
            &source_section,
            record.expression_offset as u64,
            "curve_expression_feature",
            Exactness::Derived,
        );
        let helix = crate::curve::expression_helix(record);
        let placed_helix = curve_expression_helix_definition(record);
        let neutral_helix =
            helix
                .as_ref()
                .zip(placed_helix.as_ref())
                .and_then(|(helix, procedural)| {
                    curve_expression_helix_feature_definition(helix, procedural)
                });
        if let Some(procedural_definition) = placed_helix {
            let key = cadmpeg_ir::ids::IdentityKey::from(record.entity_id).dash(record.offset);
            let curve_id =
                CurveId::compose(&crate::identity::DEPDB_CURVE_EXPRESSION_CURVE, key.clone());
            let procedural_id =
                ProceduralCurveId::compose(&crate::identity::DEPDB_CURVE_EXPRESSION_HELIX, key);
            annotate(
                annotations,
                curve_id.as_str(),
                &source_section,
                record.offset as u64,
                "curve_expression_carrier",
                Exactness::Unknown,
            );
            annotate(
                annotations,
                procedural_id.as_str(),
                &source_section,
                record.offset as u64,
                "curve_expression_helix",
                Exactness::Derived,
            );
            ctx.charge_entities(1, "admit Creo model curves")?;
            source_carriers.admit_curve(
                ir,
                Curve {
                    id: curve_id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                    source_object: None,
                },
            )?;
            source_carriers.admit_procedural_curve(
                ir,
                curve_id,
                ProceduralCurve::new(procedural_id, procedural_definition),
            )?;
        }
        let axis_definition = neutral_helix.or_else(|| {
            let helix = helix?;
            Some(IrFeatureDefinition::Operation(
                IrFeatureOperation::HelixNativeAxis {
                    axis_native_ref: cadmpeg_core::text::NonBlankString::new(
                        curve_expression_record_id(record),
                    )?,
                    axial_rise: Length::new(helix.height)?,
                    pitch: Length::new(helix.height / helix.revolutions.get())?,
                    revolutions: helix.revolutions,
                    start_angle: helix.start_angle,
                    clockwise: helix.clockwise,
                },
            ))
        });
        let definition = if let Some(definition) = axis_definition {
            definition
        } else {
            ctx.charge_collection_items(2, "creo curve-expression native parameters")?;
            IrFeatureDefinition::Operation(IrFeatureOperation::Native {
                kind: "CurveFromEquation".into(),
                parameters: BTreeMap::from([
                    (
                        cadmpeg_core::nonblank_literal!("entity_id"),
                        record.entity_id.to_string(),
                    ),
                    (
                        cadmpeg_core::nonblank_literal!("assignment_count"),
                        record.assignments.len().to_string(),
                    ),
                ]),
            })
        };
        ctx.charge_entities(1, "admit Creo model features")?;
        source_carriers.admit_feature(
            ir,
            Feature {
                id: feature_id,
                ordinal,
                name: Some(format!("Curve Equation {}", record.entity_id)),
                suppressed: Some(false),
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                source_properties: BTreeMap::new(),
                source_tag: Some("crv_fr_eqn".to_string()),
                source_text: Some(curve_expression_source_text(ctx, &record.lines)?),
                source_content: source_content.try_into().map_err(|message: &'static str| {
                    cadmpeg_core::CodecError::Malformed(message.into())
                })?,

                evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
                native_ref: Some(curve_expression_record_id(record)),
            },
        )?;
    }
    Ok(transferred_parameter_count)
}

#[cfg(test)]
mod tests;
