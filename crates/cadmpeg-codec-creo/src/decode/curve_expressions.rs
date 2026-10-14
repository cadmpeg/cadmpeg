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

#[derive(Debug)]
struct CurveExpressionParameterOrder {
    ordinals: Vec<u32>,
    cyclic_edges: HashSet<(usize, usize)>,
}

fn curve_expression_helix_definition(
    record: &crate::curve::CurveExpressionRecord,
    helix: &crate::curve::CurveExpressionHelix,
) -> Option<ProceduralCurveDefinition> {
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
                    origin.x + axis.x * helix.z_start.get(),
                    origin.y + axis.y * helix.z_start.get(),
                    origin.z + axis.z * helix.z_start.get(),
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
                    axis.x * helix.height.get() / helix.revolutions.get(),
                    axis.y * helix.height.get() / helix.revolutions.get(),
                    axis.z * helix.height.get() / helix.revolutions.get(),
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
        if index == target {
            return Ok(true);
        }
        if !visited[index] {
            visited[index] = true;
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
            let mut key = name.clone();
            key.make_ascii_lowercase();
            let Some(&index) = unique_assignment_indices.get(&key) else {
                continue;
            };
            if row.contains(&index) {
                continue;
            }
            row.push(index);
        }
    }
    let mut cyclic_edges = HashSet::new();
    for (consumer, dependency_indices) in dependencies.iter().enumerate() {
        for &dependency in dependency_indices {
            if expression_dependency_reaches(ctx, &dependencies, dependency, consumer)? {
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
    Ok(Some(CurveExpressionParameterOrder {
        ordinals,
        cyclic_edges,
    }))
}

fn curve_expression_parameter_names(
    assignments: &[crate::curve::CurveExpressionAssignment],
) -> Result<Vec<Option<String>>, CodecError> {
    let mut counts = BTreeMap::new();
    for assignment in assignments {
        if let Some((name, _)) = assignment.parameter_target() {
            let mut key = name.to_owned();
            key.make_ascii_lowercase();
            match counts.entry(key) {
                std::collections::btree_map::Entry::Vacant(entry) => {
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
            let mut key = name.to_owned();
            key.make_ascii_lowercase();
            if counts[&key] == 1 {
                Some(name.to_owned())
            } else {
                let occurrence = match occurrences.entry(key) {
                    std::collections::btree_map::Entry::Vacant(entry) => entry.insert(0usize),
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                };
                *occurrence += 1;
                let mut output = name.to_owned();
                let mut digits = [0u8; 20];
                let mut value = *occurrence;
                let mut width = 0;
                loop {
                    digits[width] = u8::try_from(value % 10).map_err(|_| {
                        cadmpeg_core::CodecError::malformed(
                            "Creo occurrence decimal digit exceeds u8",
                        )
                    })?;
                    width += 1;
                    value /= 10;
                    if value == 0 {
                        break;
                    }
                }
                output.push('#');
                for &digit in digits[..width].iter().rev() {
                    output.push(char::from(b'0' + digit));
                }
                Some(output)
            }
        } else {
            None
        };
        names.push(name);
    }
    Ok(names)
}

#[derive(Debug)]
struct AssignmentIndices {
    by_name: BTreeMap<String, Option<usize>>,
    unique: BTreeMap<String, usize>,
}

fn curve_expression_assignment_indices(
    record: &crate::curve::CurveExpressionRecord,
) -> AssignmentIndices {
    let mut by_name = BTreeMap::<String, Option<usize>>::new();
    for (ordinal, assignment) in record.assignments.iter().enumerate() {
        if assignment.activation == crate::curve::CurveExpressionActivation::Inactive {
            continue;
        }
        let Some((name, _)) = assignment.parameter_target() else {
            continue;
        };
        let mut key = name.to_owned();
        key.make_ascii_lowercase();
        match by_name.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
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
            let key = name.to_owned();
            unique.insert(key, *index);
        }
    }
    AssignmentIndices { by_name, unique }
}

fn curve_expression_emitted_ordinals(
    record: &crate::curve::CurveExpressionRecord,
    parameter_ordinals: &[u32],
) -> Result<BTreeMap<usize, u32>, CodecError> {
    let mut indices = Vec::new();
    for (index, assignment) in record.assignments.iter().enumerate() {
        if assignment.parameter_target().is_some() {
            indices.push(index);
        }
    }
    indices
        .as_mut_slice()
        .sort_by(|left, right| parameter_ordinals[*left].cmp(&parameter_ordinals[*right]));
    let mut emitted = BTreeMap::new();
    for (ordinal, index) in indices.into_iter().enumerate() {
        let ordinal = u32::try_from(ordinal)
            .map_err(|_| CodecError::malformed("curve expression parameter ordinal exceeds u32"))?;
        emitted.insert(index, ordinal);
    }
    Ok(emitted)
}

fn joined_dependency_names(names: &[String], include: impl Fn(&str) -> bool) -> Option<String> {
    let mut joined = String::new();
    let mut emitted = false;
    for name in names {
        if include(name) {
            if emitted {
                joined.push(',');
            }
            joined.push_str(name);
            emitted = true;
        }
    }
    emitted.then_some(joined)
}

fn insert_curve_expression_property(
    properties: &mut BTreeMap<String, String>,
    name: &'static str,
    value: String,
) {
    let key = name.to_owned();
    properties.insert(key, value);
}

fn join_cyclic_dependency_names(names: &[&str]) -> String {
    let mut joined = String::new();
    for (index, name) in names.iter().enumerate() {
        if index > 0 {
            joined.push(',');
        }
        joined.push_str(name);
    }
    joined
}

fn curve_expression_source_text(lines: &[crate::curve::CurveExpressionLine]) -> String {
    let mut text = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            text.push('\n');
        }
        text.push_str(&line.text);
    }
    text
}

fn curve_expression_properties(
    ctx: &DecodeContext<'_>,
    assignment: &crate::curve::CurveExpressionAssignment,
    assignment_ordinal: usize,
    parameter: (&str, &ParameterId),
    indices: (&BTreeMap<String, Option<usize>>, &BTreeMap<String, usize>),
    dimension_parameters: &BTreeMap<String, ParameterId>,
    cyclic_edges: &HashSet<(usize, usize)>,
) -> Result<BTreeMap<cadmpeg_core::text::NonBlankString, String>, CodecError> {
    let (assignment_indices_by_name, unique_assignment_indices) = indices;

    let (parameter_name, parameter_id) = parameter;

    let Some((assignment_name, declared_unit)) = assignment.parameter_target() else {
        return Err(CodecError::malformed(
            "curve expression assignment has no parameter target",
        ));
    };
    let external_dependencies = joined_dependency_names(&assignment.dependencies, |name| {
        let key = name.to_ascii_lowercase();
        key != "t"
            && !assignment_indices_by_name.contains_key(&key)
            && !dimension_parameters.contains_key(&key)
    });
    let ambiguous_dependencies = joined_dependency_names(&assignment.dependencies, |name| {
        let key = name.to_ascii_lowercase();
        matches!(assignment_indices_by_name.get(&key), Some(None))
    });
    let intrinsic_dependencies = joined_dependency_names(&assignment.dependencies, |name| {
        name.eq_ignore_ascii_case("t")
    });
    let mut properties = BTreeMap::new();
    if let Some(value) = external_dependencies {
        insert_curve_expression_property(&mut properties, "external_dependencies", value);
    }
    if let Some(value) = ambiguous_dependencies {
        insert_curve_expression_property(&mut properties, "ambiguous_dependencies", value);
    }
    insert_curve_expression_property(
        &mut properties,
        "source_assignment_ordinal",
        format!("{assignment_ordinal}"),
    );
    insert_curve_expression_property(
        &mut properties,
        "activation",
        assignment.activation.token().to_owned(),
    );
    if let Some(unit) = declared_unit {
        let unit = unit.to_owned();
        insert_curve_expression_property(&mut properties, "declared_unit", unit);
    }
    if let Some(crate::curve::CurveExpressionValue::Quantity(quantity)) = &assignment.value {
        insert_curve_expression_property(
            &mut properties,
            "evaluated_canonical_value",
            format!("{}", quantity.value()),
        );
        insert_curve_expression_property(
            &mut properties,
            "evaluated_dimension",
            format!(
                "length:{},mass:{},time:{},angle:{},temperature:{}",
                quantity.powers()[0],
                quantity.powers()[1],
                quantity.powers()[2],
                quantity.powers()[3],
                quantity.powers()[4]
            ),
        );
    }
    if parameter_name != assignment_name {
        let source_name = assignment_name.to_owned();
        insert_curve_expression_property(&mut properties, "source_name", source_name);
    }
    if let Some(value) = intrinsic_dependencies {
        insert_curve_expression_property(&mut properties, "independent_variables", value);
    }
    let mut cyclic_dependencies = Vec::new();
    for name in &assignment.dependencies {
        let mut key = name.clone();
        key.make_ascii_lowercase();
        if unique_assignment_indices
            .get(&key)
            .is_some_and(|dependency| cyclic_edges.contains(&(assignment_ordinal, *dependency)))
        {
            cyclic_dependencies.push(name.as_str());
        }
    }
    cyclic_dependencies.sort_unstable_by(Ord::cmp);
    cyclic_dependencies.dedup();
    if !cyclic_dependencies.is_empty() {
        let value = join_cyclic_dependency_names(&cyclic_dependencies);
        insert_curve_expression_property(&mut properties, "cyclic_dependencies", value);
    }
    cadmpeg_core::text::named_entries_for_decode(ctx, parameter_id.as_str(), properties)
        .map_err(Into::into)
}

fn native_curve_expression_definition(
    entity_id: u32,
    assignment_count: usize,
) -> Result<IrFeatureDefinition, CodecError> {
    let mut parameters = BTreeMap::new();
    let kind = "CurveFromEquation".to_owned().into();
    let entity_value = format!("{entity_id}");
    let assignment_value = format!("{assignment_count}");
    let entity_key = cadmpeg_core::text::NonBlankString::new("entity_id".to_owned())
        .ok_or_else(|| CodecError::malformed("native entity key is blank"))?;
    parameters.insert(entity_key, entity_value);
    let assignment_key = cadmpeg_core::text::NonBlankString::new("assignment_count".to_owned())
        .ok_or_else(|| CodecError::malformed("native assignment key is blank"))?;
    parameters.insert(assignment_key, assignment_value);
    Ok(IrFeatureDefinition::Operation(IrFeatureOperation::Native {
        kind,
        parameters,
    }))
}

fn curve_expression_feature_labels(entity_id: u32) -> (String, String) {
    (
        format!("Curve Equation {entity_id}"),
        "crv_fr_eqn".to_owned(),
    )
}

fn curve_expression_parameter_dependencies(
    record: &crate::curve::CurveExpressionRecord,
    assignment_ordinal: usize,
    assignment_indices_by_name: &BTreeMap<String, Option<usize>>,
    unique_assignment_indices: &BTreeMap<String, usize>,
    cyclic_edges: &HashSet<(usize, usize)>,
    dimension_parameters: &BTreeMap<String, ParameterId>,
) -> Result<Vec<ParameterId>, CodecError> {
    let assignment = &record.assignments[assignment_ordinal];
    let mut seen = BTreeSet::new();
    let mut dependencies = Vec::new();
    let mut dimension_dependencies = Vec::new();
    for name in &assignment.dependencies {
        let mut key = name.clone();
        key.make_ascii_lowercase();
        if let Some(&dependency) = unique_assignment_indices.get(&key) {
            if cyclic_edges.contains(&(assignment_ordinal, dependency))
                || seen.contains(&dependency)
            {
                continue;
            }
            seen.insert(dependency);
            dependencies.push(crate::identity::compose_checked::<ParameterId>(
                &crate::identity::DEPDB_CURVE_EXPRESSION_PARAMETER,
                format_args!("{}-{}-{dependency}", record.entity_id, record.offset),
            )?);
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
        dimension_dependencies.push(parameter);
    }
    for parameter in dimension_dependencies {
        let copied = parameter.as_str().to_owned();
        let copied = ParameterId::try_from(copied).map_err(|_| {
            CodecError::malformed("curve expression dimension parameter id is invalid")
        })?;
        dependencies.push(copied);
    }
    Ok(dependencies)
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
        let ordinal = ordinal_base + cadmpeg_core::decode::u64_from_index(expression_ordinal);
        let feature_id = crate::identity::compose_checked::<IrFeatureId>(
            &crate::identity::DEPDB_CURVE_EXPRESSION_FEATURE,
            format_args!("{}-{}", record.entity_id, record.offset),
        )?;
        let AssignmentIndices {
            by_name: assignment_indices_by_name,
            unique: unique_assignment_indices,
        } = curve_expression_assignment_indices(record);
        let Some(CurveExpressionParameterOrder {
            ordinals: parameter_ordinals,
            cyclic_edges,
        }) = curve_expression_parameter_order(ctx, record, &unique_assignment_indices)?
        else {
            continue;
        };
        let parameter_names = curve_expression_parameter_names(&record.assignments)?;
        let emitted_ordinals = curve_expression_emitted_ordinals(record, &parameter_ordinals)?;
        let mut source_content = Vec::new();
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
            let parameter_id = crate::identity::compose_checked::<ParameterId>(
                &crate::identity::DEPDB_CURVE_EXPRESSION_PARAMETER,
                format_args!(
                    "{}-{}-{assignment_ordinal}",
                    record.entity_id, record.offset
                ),
            )?;
            let dependencies = curve_expression_parameter_dependencies(
                record,
                assignment_ordinal,
                &assignment_indices_by_name,
                &unique_assignment_indices,
                &cyclic_edges,
                dimension_parameters,
            )?;
            annotate(
                ctx,
                annotations,
                parameter_id.as_str(),
                &source_section,
                cadmpeg_core::decode::u64_from_index(assignment.offset),
                "curve_expression_assignment",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model parameters")?;
            let value = match assignment.value.as_ref() {
                Some(crate::curve::CurveExpressionValue::String(value)) => {
                    Some(ParameterValue::String(value.to_owned()))
                }
                other => other.and_then(|value| match value {
                    crate::curve::CurveExpressionValue::Number(value) => {
                        Some(ParameterValue::Real(*value))
                    }
                    crate::curve::CurveExpressionValue::Length(value) => Some(
                        ParameterValue::Length(cadmpeg_ir::scalar::Length::new(value.get())?),
                    ),
                    crate::curve::CurveExpressionValue::Angle(value) => {
                        Some(ParameterValue::Angle(cadmpeg_ir::scalar::Angle::new(
                            value.get().to_radians(),
                        )?))
                    }
                    crate::curve::CurveExpressionValue::Quantity(_)
                    | crate::curve::CurveExpressionValue::String(_) => None,
                }),
            };
            source_carriers.admit_parameter(
                ir,
                DesignParameter {
                    id: crate::identity::copy_checked_id(parameter_id.as_str())?,
                    owner: Some(crate::identity::copy_checked_id(feature_id.as_str())?),
                    ordinal,
                    name: parameter_name.to_owned(),
                    expression: assignment.expression.clone(),
                    display: None,
                    value,
                    dependencies: cadmpeg_ir::features::DistinctMembers::try_from(
                        dependencies,
                        ctx,
                    )
                    .map_err(cadmpeg_core::CodecError::from)?,
                    properties: BTreeMap::new(),
                    pmi: None,
                    native_ref: Some(curve_expression_record_id(record)),
                },
            )?;
            transferred_parameter_count += 1;
            source_content.push(FeatureSourceContent::Parameter(
                crate::identity::copy_checked_id(parameter_id.as_str())?,
            ));
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
                (&parameter.name, &parameter.id),
                (&assignment_indices_by_name, &unique_assignment_indices),
                dimension_parameters,
                &cyclic_edges,
            )?;
            parameter_index += 1;
        }
        let helix = crate::curve::expression_helix(ctx, record)?;
        let placed_helix = helix
            .as_ref()
            .and_then(|helix| curve_expression_helix_definition(record, helix));
        let neutral_helix =
            helix
                .as_ref()
                .zip(placed_helix.as_ref())
                .and_then(|(helix, procedural)| {
                    curve_expression_helix_feature_definition(helix, procedural)
                });
        if let Some(procedural_definition) = placed_helix {
            let curve_id = crate::identity::compose_checked::<CurveId>(
                &crate::identity::DEPDB_CURVE_EXPRESSION_CURVE,
                format_args!("{}-{}", record.entity_id, record.offset),
            )?;
            let procedural_id = crate::identity::compose_checked::<ProceduralCurveId>(
                &crate::identity::DEPDB_CURVE_EXPRESSION_HELIX,
                format_args!("{}-{}", record.entity_id, record.offset),
            )?;
            annotate(
                ctx,
                annotations,
                curve_id.as_str(),
                &source_section,
                cadmpeg_core::decode::u64_from_index(record.offset),
                "curve_expression_carrier",
                Exactness::Unknown,
            )?;
            annotate(
                ctx,
                annotations,
                procedural_id.as_str(),
                &source_section,
                cadmpeg_core::decode::u64_from_index(record.offset),
                "curve_expression_helix",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model curves")?;
            source_carriers.admit_curve(
                ctx,
                ir,
                Curve {
                    parameter_range: None,
                    id: crate::identity::copy_checked_id(curve_id.as_str())?,
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                    source_object: None,
                },
            )?;
            source_carriers.admit_procedural_curve(
                ctx,
                ir,
                &curve_id,
                ProceduralCurve::new(procedural_id, procedural_definition),
            )?;
        }
        let axis_definition = if let Some(definition) = neutral_helix {
            Some(definition)
        } else if let Some(helix) = helix {
            let axis_id = curve_expression_record_id(record);
            (|| {
                Some(IrFeatureDefinition::Operation(
                    IrFeatureOperation::HelixNativeAxis {
                        axis_native_ref: cadmpeg_core::text::NonBlankString::new(axis_id)?,
                        axial_rise: Length::new(helix.height.get())?,
                        pitch: Length::new(helix.height.get() / helix.revolutions.get())?,
                        revolutions: helix.revolutions,
                        start_angle: helix.start_angle,
                        clockwise: helix.clockwise,
                    },
                ))
            })()
        } else {
            None
        };
        let definition = if let Some(definition) = axis_definition {
            definition
        } else {
            native_curve_expression_definition(record.entity_id, record.assignments.len())?
        };
        annotate(
            ctx,
            annotations,
            feature_id.as_str(),
            &source_section,
            cadmpeg_core::decode::u64_from_index(record.expression_offset),
            "curve_expression_feature",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model features")?;
        let (name, source_tag) = curve_expression_feature_labels(record.entity_id);
        source_carriers.admit_feature(
            ctx,
            ir,
            Feature {
                id: feature_id,
                ordinal,
                name: Some(name),
                suppressed: Some(false),
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                source_properties: BTreeMap::new(),
                source_tag: Some(source_tag),
                source_text: Some(curve_expression_source_text(&record.lines)),
                source_content: cadmpeg_ir::features::FeatureContent::new(
                    source_content,
                    ctx,
                    "validate Creo feature source content",
                )
                .map_err(cadmpeg_core::CodecError::from)?,

                evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
                native_ref: Some(curve_expression_record_id(record)),
            },
        )?;
    }
    Ok(transferred_parameter_count)
}

#[cfg(test)]
mod tests;
