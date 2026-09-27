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
            let Some(&index) =
                unique_assignment_indices.get(&crate::curve::expression_identifier_key(name))
            else {
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
        for (assignment_ordinal, assignment) in record.assignments.iter().enumerate() {
            let Some((assignment_name, declared_unit)) = assignment.parameter_target() else {
                continue;
            };
            let Some(&ordinal) = emitted_ordinals.get(&assignment_ordinal) else {
                continue;
            };
            let parameter_id = ParameterId::compose(
                &crate::identity::DEPDB_CURVE_EXPRESSION_PARAMETER,
                cadmpeg_ir::ids::IdentityKey::from(record.entity_id)
                    .dash(record.offset)
                    .dash(assignment_ordinal),
            );
            let mut dependencies = assignment
                .dependencies
                .iter()
                .filter_map(|name| {
                    unique_assignment_indices
                        .get(&crate::curve::expression_identifier_key(name))
                        .copied()
                })
                .filter(|dependency| !cyclic_edges.contains(&(assignment_ordinal, *dependency)))
                .scan(BTreeSet::new(), |seen, dependency| {
                    seen.insert(dependency).then_some(dependency)
                })
                .map(|dependency| {
                    ParameterId::compose(
                        &crate::identity::DEPDB_CURVE_EXPRESSION_PARAMETER,
                        cadmpeg_ir::ids::IdentityKey::from(record.entity_id)
                            .dash(record.offset)
                            .dash(dependency),
                    )
                })
                .collect::<Vec<_>>();
            dependencies.extend(assignment.dependencies.iter().filter_map(|name| {
                let key = crate::curve::expression_identifier_key(name);
                if assignment_indices_by_name.contains_key(&key) {
                    None
                } else {
                    dimension_parameters.get(&key).cloned()
                }
            }));
            let external_dependencies = assignment
                .dependencies
                .iter()
                .filter(|name| {
                    let key = crate::curve::expression_identifier_key(name);
                    key != "t"
                        && !assignment_indices_by_name.contains_key(&key)
                        && !dimension_parameters.contains_key(&key)
                })
                .cloned()
                .collect::<Vec<_>>();
            let ambiguous_dependencies = assignment
                .dependencies
                .iter()
                .filter(|name| {
                    matches!(
                        assignment_indices_by_name
                            .get(&crate::curve::expression_identifier_key(name)),
                        Some(None)
                    )
                })
                .cloned()
                .collect::<Vec<_>>();
            let intrinsic_dependencies = assignment
                .dependencies
                .iter()
                .filter(|name| crate::curve::expression_identifier_key(name) == "t")
                .cloned()
                .collect::<Vec<_>>();
            let mut properties = BTreeMap::new();
            if !external_dependencies.is_empty() {
                properties.insert(
                    "external_dependencies".to_string(),
                    external_dependencies.join(","),
                );
            }
            if !ambiguous_dependencies.is_empty() {
                properties.insert(
                    "ambiguous_dependencies".to_string(),
                    ambiguous_dependencies.join(","),
                );
            }
            properties.insert(
                "source_assignment_ordinal".to_string(),
                assignment_ordinal.to_string(),
            );
            properties.insert(
                "activation".to_string(),
                assignment.activation.token().to_string(),
            );
            if let Some(unit) = declared_unit {
                properties.insert("declared_unit".to_string(), unit.to_owned());
            }
            if let Some(crate::curve::CurveExpressionValue::Quantity(quantity)) = &assignment.value
            {
                properties.insert(
                    "evaluated_canonical_value".to_string(),
                    quantity.value.to_string(),
                );
                properties.insert(
                    "evaluated_dimension".to_string(),
                    format!(
                        "length:{},mass:{},time:{},angle:{},temperature:{}",
                        quantity.length_power,
                        quantity.mass_power,
                        quantity.time_power,
                        quantity.angle_power,
                        quantity.temperature_power
                    ),
                );
            }
            let Some(parameter_name) = parameter_names
                .get(assignment_ordinal)
                .and_then(Option::as_ref)
            else {
                return Err(cadmpeg_core::CodecError::malformed(format!(
                    "curve expression record {} assignment {} has no parameter name",
                    record.entity_id, assignment_ordinal
                )));
            };
            if parameter_name != assignment_name {
                properties.insert("source_name".to_string(), assignment_name.to_owned());
            }
            if !intrinsic_dependencies.is_empty() {
                properties.insert(
                    "independent_variables".to_string(),
                    intrinsic_dependencies.join(","),
                );
            }
            let cyclic_dependencies = assignment
                .dependencies
                .iter()
                .filter_map(|name| {
                    let key = crate::curve::expression_identifier_key(name);
                    unique_assignment_indices
                        .get(&key)
                        .filter(|dependency| {
                            cyclic_edges.contains(&(assignment_ordinal, **dependency))
                        })
                        .map(|_| name.clone())
                })
                .collect::<BTreeSet<_>>();
            if !cyclic_dependencies.is_empty() {
                properties.insert(
                    "cyclic_dependencies".to_string(),
                    cyclic_dependencies
                        .into_iter()
                        .collect::<Vec<_>>()
                        .join(","),
                );
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
            source_carriers.admit_parameter(
                ir,
                DesignParameter {
                    id: parameter_id.clone(),
                    owner: Some(feature_id.clone()),
                    ordinal,
                    name: parameter_name.clone(),
                    expression: assignment.expression.clone(),
                    display: None,
                    value: assignment.value.as_ref().and_then(|value| match value {
                        crate::curve::CurveExpressionValue::Number(value) => Some(
                            ParameterValue::Real(cadmpeg_ir::scalar::FiniteReal::new(*value)?),
                        ),
                        crate::curve::CurveExpressionValue::Length(value) => Some(
                            ParameterValue::Length(cadmpeg_ir::scalar::Length::new(*value)?),
                        ),
                        crate::curve::CurveExpressionValue::Angle(value) => {
                            Some(ParameterValue::Angle(cadmpeg_ir::scalar::Angle::new(
                                value.to_radians(),
                            )?))
                        }
                        crate::curve::CurveExpressionValue::Quantity(_) => None,
                        crate::curve::CurveExpressionValue::String(value) => {
                            Some(ParameterValue::String(value.clone()))
                        }
                    }),
                    dependencies: dependencies.into_iter().collect(),
                    properties: cadmpeg_core::text::named_entries(
                        parameter_id.as_str(),
                        properties,
                    )?,
                    pmi: None,
                    native_ref: Some(curve_expression_record_id(record)),
                },
            )?;
            transferred_parameter_count += 1;
            source_content.push(FeatureSourceContent::Parameter(parameter_id.clone()));
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
        let definition = neutral_helix
            .or_else(|| {
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
            })
            .unwrap_or_else(|| {
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
            });
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
                source_text: Some(
                    record
                        .lines
                        .iter()
                        .map(|line| line.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
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
