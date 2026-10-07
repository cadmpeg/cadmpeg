// SPDX-License-Identifier: Apache-2.0
//! Geometric validation-property decoding and mesh self-checks.

use std::collections::{btree_map::Entry, BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::decode_text_charged;
use super::geometry::GeometryData;
use super::StageOutcome;
use super::{RecordExt, ValueExt};

#[derive(Clone, Copy)]
enum Expected {
    Area(f64),
    Volume(f64),
    Centroid(Point3),
}

pub(super) fn decode(
    exchange: &Exchange,
    geometry: &GeometryData,
    ir: &mut CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<StageOutcome<()>, CodecError> {
    if !exchange.has_entity(ctx, "PROPERTY_DEFINITION")?
        || !exchange.has_entity(ctx, "PROPERTY_DEFINITION_REPRESENTATION")?
    {
        return Ok(StageOutcome {
            value: (),
            claims: BTreeSet::new(),
            notes: Vec::new(),
            losses: Vec::new(),
        });
    }
    let mut losses = Vec::new();
    let mut representations = BTreeMap::new();
    for (&id, record) in ctx
        .admit_iter(exchange.records(), "STEP decode traversal")
        .map_err(cadmpeg_core::CodecError::from)?
    {
        let Some(representation_items) = super::representation::items(ctx, record)? else {
            continue;
        };
        let mut items = BTreeSet::new();
        for item in representation_items {
            ctx.insert_btree_set(&mut items, item, "step_validation_representation_items")?;
        }
        if !items.is_empty() {
            ctx.insert_btree_map(
                &mut representations,
                id,
                items,
                "step_validation_representations",
            )?;
        }
    }
    let mut properties = BTreeMap::new();
    for (id, record) in exchange.entities(ctx, "PROPERTY_DEFINITION")? {
        let Some(property) = record.partial(ctx, "PROPERTY_DEFINITION")? else {
            continue;
        };
        let name = property
            .parameters
            .first()
            .map(|value| {
                decode_text_charged(
                    exchange,
                    value,
                    &mut losses,
                    id,
                    "validation property name",
                    StepLossCode::MetadataStringInvalid,
                    ctx,
                )
            })
            .transpose()?
            .flatten();
        let Some(name) = name else {
            continue;
        };
        if ctx.eq_ignore_ascii_case(
            name.as_str(),
            "geometric validation property",
            "STEP validation property name case equality",
        )? {
            let description = property
                .parameters
                .get(1)
                .map(|value| {
                    decode_text_charged(
                        exchange,
                        value,
                        &mut losses,
                        id,
                        "validation property description",
                        StepLossCode::MetadataStringInvalid,
                        ctx,
                    )
                })
                .transpose()?
                .flatten()
                .unwrap_or_default();
            ctx.insert_btree_map(
                &mut properties,
                id,
                description,
                "step_validation_properties",
            )?;
        }
    }
    let computed = mesh_properties(ir, ctx)?;
    let mut typed = BTreeSet::new();
    let mut validation_points = BTreeSet::new();
    let mut validation_representations = BTreeSet::new();
    let mut notes = Vec::new();

    for (relation_id, relation) in exchange.entities(ctx, "PROPERTY_DEFINITION_REPRESENTATION")? {
        let Some(relation) = relation.partial(ctx, "PROPERTY_DEFINITION_REPRESENTATION")? else {
            continue;
        };
        let Some(property_id) = relation.parameters.first().and_then(ValueExt::reference) else {
            continue;
        };
        let Some(description) = properties.get(&property_id) else {
            continue;
        };
        let Some(representation_id) = relation.parameters.get(1).and_then(ValueExt::reference)
        else {
            continue;
        };
        let Some(item_ids) = representations.get(&representation_id) else {
            continue;
        };
        ctx.insert_btree_set(
            &mut validation_representations,
            representation_id,
            "step_validation_used_representations",
        )?;
        for &item_id in ctx.admit_iter(item_ids, "STEP validation item traversal")? {
            let Some(item) = exchange.records().get(&item_id) else {
                continue;
            };
            let scale = geometry.units.length([item_id, representation_id]).get();
            let expected = expected_value(item_id, item, exchange, scale, &mut losses, ctx)?;
            let Some(expected) = expected else {
                push_validation_loss(
                    &mut losses,
                    StepLossCode::DecodeWarning,
                    format!(
                        "geometric validation property #{property_id} has unsupported item #{item_id}"
                    ),
                    ctx,
                )?;
                continue;
            };
            if matches!(expected, Expected::Centroid(_)) {
                ctx.insert_btree_set(&mut validation_points, item_id, "step_validation_points")?;
            }
            for id in [property_id, relation_id, representation_id, item_id] {
                ctx.insert_btree_set(&mut typed, id, "step_validation_claims")?;
            }
            if let Some(unit) = measure_unit(ctx, item)? {
                collect_unit_records(unit, exchange, &mut typed, ctx)?;
            }
            let (kind, expected_text, actual) = match expected {
                Expected::Area(value) => {
                    ("surface area", value.to_string(), computed.map(|p| p.area))
                }
                Expected::Volume(value) => {
                    ("volume", value.to_string(), computed.map(|p| p.volume))
                }
                Expected::Centroid(value) => (
                    "centroid",
                    format!("({},{},{})", value.x, value.y, value.z),
                    computed.map(|p| p.centroid_distance(value)),
                ),
            };
            if let Some(actual) = actual {
                let actual_text = match expected {
                    Expected::Centroid(_) => format!("distance {actual}"),
                    _ => actual.to_string(),
                };
                ctx.push_formatted_retained(&mut notes, format_args!(
                        "geometric validation {kind} {description}: expected {expected_text}, tessellation approximation {actual_text}"
                    ), "step_validation_notes", "step_validation_note_text")?;
            } else {
                ctx.push_formatted_retained(
                    &mut notes,
                    format_args!(
                        "geometric validation {kind} {description}: expected {expected_text}"
                    ),
                    "step_validation_notes",
                    "step_validation_note_text",
                )?;
            }
        }
    }
    let mut referenced_validation_points = BTreeSet::new();
    if !validation_points.is_empty() {
        for (&record_id, record) in ctx
            .admit_iter(exchange.records(), "STEP decode traversal")
            .map_err(cadmpeg_core::CodecError::from)?
        {
            if validation_representations.contains(&record_id) {
                continue;
            }
            for partial in ctx.admit_iter(
                &record.partials[..],
                "STEP validation reference partial traversal",
            )? {
                for value in ctx.admit_iter(
                    partial.parameters.as_slice(),
                    "STEP validation reference parameter traversal",
                )? {
                    collect_validation_references(
                        value,
                        &validation_points,
                        &mut referenced_validation_points,
                        ctx,
                    )?;
                }
            }
        }
    }
    ctx.retain_vec(
        &mut ir.model.points,
        |point| {
            // A point whose identity names no entity is not a validation point.
            let Some(id) = step_id(ctx, point.id.as_str())? else {
                return Ok(true);
            };
            Ok(!validation_points.contains(&id) || referenced_validation_points.contains(&id))
        },
        "STEP validation point retention",
    )?;
    Ok(StageOutcome {
        value: (),
        claims: typed,
        notes,
        losses,
    })
}

fn expected_value(
    id: u64,
    record: &RawRecord,
    exchange: &Exchange,
    scale: f64,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Expected>, CodecError> {
    if let Some(point) = record.partial(ctx, "CARTESIAN_POINT")? {
        let Some(values) = point.parameters.get(1).and_then(ValueExt::list) else {
            return Ok(None);
        };
        if values.len() != 3 {
            return Ok(None);
        }
        let [Some(x), Some(y), Some(z)] =
            [values[0].number(), values[1].number(), values[2].number()]
        else {
            return Ok(None);
        };
        return Ok(Some(Expected::Centroid(Point3::new(
            x * scale,
            y * scale,
            z * scale,
        ))));
    }
    if record
        .partial(ctx, "MEASURE_REPRESENTATION_ITEM")?
        .is_none()
    {
        return Ok(None);
    }
    let mut measure = None;
    'measure: for partial in ctx.admit_iter(
        &record.partials[..],
        "STEP validation measure partial traversal",
    )? {
        for value in ctx.admit_iter(
            partial.parameters.as_slice(),
            "STEP validation measure parameter traversal",
        )? {
            if let Some(value) = area_or_volume_measure(ctx, value)? {
                measure = Some(value);
                break 'measure;
            }
        }
    }
    let Some((kind, value)) = measure else {
        return Ok(None);
    };
    let scale = measure_scale(id, record, exchange, scale, kind, losses, ctx)?;
    Ok(Some(match kind {
        "AREA_MEASURE" => Expected::Area(value * scale),
        "VOLUME_MEASURE" => Expected::Volume(value * scale),
        _ => return Ok(None),
    }))
}

fn measure_scale(
    id: u64,
    record: &RawRecord,
    exchange: &Exchange,
    fallback: f64,
    kind: &str,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<f64, CodecError> {
    let resolved = measure_unit(ctx, record)?
        .and_then(|unit| exchange.records().get(&unit))
        .map(|record| derived_unit_elements(ctx, record))
        .transpose()?
        .flatten()
        .and_then(ValueExt::list);
    let resolved = if let Some(elements) = resolved {
        let mut scale = Some(1.0);
        for element in elements {
            let element = element
                .reference()
                .and_then(|id| exchange.records().get(&id));
            let partial = element
                .map(|element| element.partial(ctx, "DERIVED_UNIT_ELEMENT"))
                .transpose()?
                .flatten();
            let fields = partial.and_then(|element| {
                Some((
                    element.parameters.first()?.reference()?,
                    element.parameters.get(1)?.number()?,
                ))
            });
            let Some((base, exponent)) = fields else {
                scale = None;
                break;
            };
            let Some(base) =
                super::geometry::unit_scale_mm(base, exchange, &mut BTreeSet::new(), ctx)?
            else {
                scale = None;
                break;
            };
            scale = scale.map(|scale| scale * base.get().powf(exponent));
        }
        scale
    } else {
        None
    };
    match resolved {
        Some(scale) => Ok(scale),
        None => {
            push_validation_loss(
                losses,
                StepLossCode::ValidationMeasureUnitUnresolved,
                ctx.format_retained(format_args!(
                    "geometric validation {kind} measure #{id} unit scale did not resolve; the document length scale was used",
                ), "STEP measure_scale text")?,
                ctx,
            )?;
            Ok(fallback.powi(if kind == "AREA_MEASURE" { 2 } else { 3 }))
        }
    }
}

fn push_validation_loss(
    losses: &mut Vec<LossNote>,
    code: StepLossCode,
    message: String,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    ctx.reserve_vec(losses, 1, "step_validation_losses")?;
    losses.push(code.note(message));
    Ok(())
}

fn area_or_volume_measure<'a>(
    ctx: &DecodeContext<'_>,
    value: &'a Value,
) -> Result<Option<(&'a str, f64)>, CodecError> {
    let _depth = ctx.enter_nested("STEP validation measure nesting")?;
    match value {
        Value::Typed(kind, value) if matches!(kind.as_str(), "AREA_MEASURE" | "VOLUME_MEASURE") => {
            Ok(value.number().map(|value| (kind.as_str(), value)))
        }
        Value::Typed(_, value) => area_or_volume_measure(ctx, value),
        Value::List(values) => {
            for value in
                ctx.admit_iter(values.as_slice(), "STEP validation measure list traversal")?
            {
                if let Some(measure) = area_or_volume_measure(ctx, value)? {
                    return Ok(Some(measure));
                }
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

fn measure_unit(ctx: &DecodeContext<'_>, record: &RawRecord) -> Result<Option<u64>, CodecError> {
    if let Some(partial) = record.partial(ctx, "MEASURE_WITH_UNIT")? {
        let unit = ctx
            .admit_iter(
                partial.parameters.as_slice(),
                "STEP validation measure unit traversal",
            )?
            .rev()
            .find_map(ValueExt::reference);
        if unit.is_some() {
            return Ok(unit);
        }
    }
    Ok(record
        .partial(ctx, "MEASURE_REPRESENTATION_ITEM")?
        .and_then(|partial| partial.parameters.get(2))
        .and_then(ValueExt::reference))
}

fn derived_unit_elements<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<&'a Value>, CodecError> {
    for name in ["DERIVED_UNIT", "AREA_UNIT", "VOLUME_UNIT"] {
        if let Some(partial) = ctx
            .admit_iter(&record.partials[..], "STEP derived unit partial traversal")?
            .map(|partial| -> Result<Option<_>, CodecError> {
                Ok((ctx.equal(
                    partial.name.as_str(),
                    name,
                    "STEP derived unit elements equality",
                )?)
                .then_some(partial))
            })
            .find_map(Result::transpose)
            .transpose()?
        {
            return Ok(partial.parameters.first());
        }
    }
    Ok(None)
}

fn collect_unit_records(
    id: u64,
    exchange: &Exchange,
    typed: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    ctx.insert_btree_set(typed, id, "step_validation_claims")?;
    let Some(record) = exchange.records().get(&id) else {
        return Ok(());
    };
    let Some(elements) = derived_unit_elements(ctx, record)?.and_then(ValueExt::list) else {
        return Ok(());
    };
    for element in elements.iter().filter_map(ValueExt::reference) {
        ctx.insert_btree_set(typed, element, "step_validation_claims")?;
        if let Some(base) = exchange
            .records()
            .get(&element)
            .map(|record| record.partial(ctx, "DERIVED_UNIT_ELEMENT"))
            .transpose()?
            .flatten()
            .and_then(|record| record.parameters.first())
            .and_then(ValueExt::reference)
        {
            ctx.insert_btree_set(typed, base, "step_validation_claims")?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct MeshProperties {
    area: f64,
    volume: f64,
    centroid: Point3,
}

impl MeshProperties {
    fn centroid_distance(self, expected: Point3) -> f64 {
        (self.centroid.x - expected.x)
            .hypot(self.centroid.y - expected.y)
            .hypot(self.centroid.z - expected.z)
    }
}

fn mesh_properties(
    ir: &CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<Option<MeshProperties>, CodecError> {
    let Some(body) = (ir.model.bodies.len() == 1).then(|| &ir.model.bodies[0].id) else {
        return Ok(None);
    };
    let origin = ctx
        .admit_iter(
            ir.model.tessellations.as_slice(),
            "STEP validation mesh origin traversal",
        )?
        .map(|mesh| -> Result<Option<_>, CodecError> {
            Ok(ctx
                .equal(
                    &mesh.body.as_ref(),
                    &Some(body),
                    "STEP validation mesh body equality",
                )?
                .then_some(mesh))
        })
        .filter_map(Result::transpose)
        .map(|mesh| -> Result<Option<_>, CodecError> {
            let mesh = mesh?;
            Ok(mesh.triangles().first().and_then(|triangle| {
                mesh.vertices()
                    .get(cadmpeg_core::decode::index_from_u32(triangle[0]))
                    .copied()
            }))
        })
        .find_map(Result::transpose)
        .transpose()?;
    let Some(origin) = origin else {
        return Ok(None);
    };
    let mut extent = 0.0_f64;
    for mesh in ctx
        .admit_iter(
            ir.model.tessellations.as_slice(),
            "STEP validation mesh extent traversal",
        )?
        .map(|mesh| -> Result<Option<_>, CodecError> {
            Ok(ctx
                .equal(
                    &mesh.body.as_ref(),
                    &Some(body),
                    "STEP validation mesh body equality",
                )?
                .then_some(mesh))
        })
        .filter_map(Result::transpose)
    {
        let mesh = mesh?;
        extent = ctx
            .admit_iter(&mesh.vertices(), "STEP validation vertex extent traversal")?
            .fold(extent, |scale, point| {
                scale
                    .max((point.x - origin.x).abs())
                    .max((point.y - origin.y).abs())
                    .max((point.z - origin.z).abs())
            });
    }
    let Some(exponent) = cadmpeg_ir::math::power_of_two_bound(extent) else {
        return Ok(None);
    };
    let mut area = 0.0;
    let mut area_centroid = [0.0; 3];
    let mut signed_volume = 0.0;
    let mut volume_centroid = [0.0; 3];
    let mut triangles = 0usize;
    let mut watertight = true;
    let mut coordinate_scale = 0.0_f64;
    for mesh in ctx
        .admit_iter(
            ir.model.tessellations.as_slice(),
            "STEP validation mesh property traversal",
        )?
        .map(|mesh| -> Result<Option<_>, CodecError> {
            Ok(ctx
                .equal(
                    &mesh.body.as_ref(),
                    &Some(body),
                    "STEP validation mesh body equality",
                )?
                .then_some(mesh))
        })
        .filter_map(Result::transpose)
    {
        let mesh = mesh?;
        let mut edge_uses = BTreeMap::<(u32, u32), usize>::new();
        for triangle in mesh.triangles() {
            ctx.charge_work(1, "step_validation_mesh_triangles")?;
            let [a, b, c] = triangle.map(|index| {
                mesh.vertices()
                    .get(cadmpeg_core::decode::index_from_u32(index))
                    .copied()
            });
            let (Some(a), Some(b), Some(c)) = (a, b, c) else {
                return Ok(None);
            };
            for [first, second] in [
                [triangle[0], triangle[1]],
                [triangle[1], triangle[2]],
                [triangle[2], triangle[0]],
            ] {
                let edge = (first.min(second), first.max(second));
                ctx.admit_btree_entry(&edge_uses, &edge, "step_validation_mesh_edges")?;
                match edge_uses.entry(edge) {
                    Entry::Occupied(mut entry) => *entry.get_mut() += 1,
                    Entry::Vacant(entry) => {
                        entry.insert(1);
                    }
                }
            }
            let relative = |point: cadmpeg_ir::features::FinitePoint3| {
                Some(Point3::new(
                    cadmpeg_ir::math::scale_power_of_two(point.x - origin.x, -exponent)?.get(),
                    cadmpeg_ir::math::scale_power_of_two(point.y - origin.y, -exponent)?.get(),
                    cadmpeg_ir::math::scale_power_of_two(point.z - origin.z, -exponent)?.get(),
                ))
            };
            let [Some(a), Some(b), Some(c)] = [a, b, c].map(relative) else {
                return Ok(None);
            };
            coordinate_scale = coordinate_scale
                .max(a.x.abs())
                .max(a.y.abs())
                .max(a.z.abs())
                .max(b.x.abs())
                .max(b.y.abs())
                .max(b.z.abs())
                .max(c.x.abs())
                .max(c.y.abs())
                .max(c.z.abs());
            let ab = [b.x - a.x, b.y - a.y, b.z - a.z];
            let ac = [c.x - a.x, c.y - a.y, c.z - a.z];
            let cross = [
                ab[1] * ac[2] - ab[2] * ac[1],
                ab[2] * ac[0] - ab[0] * ac[2],
                ab[0] * ac[1] - ab[1] * ac[0],
            ];
            let triangle_area = 0.5 * cross[0].hypot(cross[1]).hypot(cross[2]);
            area += triangle_area;
            for axis in 0..3 {
                area_centroid[axis] +=
                    triangle_area * [a.x + b.x + c.x, a.y + b.y + c.y, a.z + b.z + c.z][axis] / 3.0;
            }
            let tetra_volume = (a.x * (b.y * c.z - b.z * c.y)
                + a.y * (b.z * c.x - b.x * c.z)
                + a.z * (b.x * c.y - b.y * c.x))
                / 6.0;
            signed_volume += tetra_volume;
            for axis in 0..3 {
                volume_centroid[axis] +=
                    tetra_volume * [a.x + b.x + c.x, a.y + b.y + c.y, a.z + b.z + c.z][axis] / 4.0;
            }
            triangles += 1;
        }
        watertight &= !edge_uses.is_empty()
            && ctx
                .admit_iter(&(edge_uses), "STEP mesh properties map traversal")
                .map_err(cadmpeg_core::CodecError::from)?
                .map(|(_, value)| value)
                .all(|uses| *uses == 2);
    }
    if triangles == 0 || area == 0.0 {
        return Ok(None);
    }
    let Some(triangle_count) = cadmpeg_core::convert::f64_from_index(triangles) else {
        return Ok(None);
    };
    let volume_epsilon = f64::EPSILON * coordinate_scale.powi(3) * triangle_count.max(1.0);
    let centroid = if watertight && signed_volume.abs() > volume_epsilon {
        Point3::new(
            volume_centroid[0] / signed_volume,
            volume_centroid[1] / signed_volume,
            volume_centroid[2] / signed_volume,
        )
    } else {
        Point3::new(
            area_centroid[0] / area,
            area_centroid[1] / area,
            area_centroid[2] / area,
        )
    };
    let [Some(x), Some(y), Some(z)] = [centroid.x, centroid.y, centroid.z]
        .map(|component| cadmpeg_ir::math::scale_power_of_two(component, exponent))
    else {
        return Ok(None);
    };
    let centroid = Point3::new(origin.x + x.get(), origin.y + y.get(), origin.z + z.get());
    if !area.is_finite() || !signed_volume.is_finite() || !centroid.is_finite() {
        return Ok(None);
    }
    let Some(area) = cadmpeg_ir::math::scale_power_of_two(area, 2 * exponent) else {
        return Ok(None);
    };
    let volume = if signed_volume == 0.0 {
        0.0
    } else {
        let Some(volume) = cadmpeg_ir::math::scale_power_of_two(signed_volume.abs(), 3 * exponent)
        else {
            return Ok(None);
        };
        volume.get()
    };
    Ok(Some(MeshProperties {
        area: area.get(),
        volume,
        centroid,
    }))
}

/// The numeric entity identifier an IR identity ends with, or `None` when it
/// names none.
fn step_id(ctx: &DecodeContext<'_>, id: &str) -> Result<Option<u64>, CodecError> {
    id.rsplit('#')
        .next()
        .map(|number| ctx.parse_text::<u64>(number, "STEP validation point number parse"))
        .transpose()
        .map(|number| number.and_then(Result::ok))
}

fn collect_validation_references(
    value: &Value,
    validation_points: &BTreeSet<u64>,
    referenced: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let _nested = ctx.enter_nested("step_validation_reference_walk")?;
    match value {
        Value::Reference(id) if validation_points.contains(id) => {
            ctx.insert_btree_set(referenced, *id, "step_validation_referenced_points")?;
        }
        Value::List(values) => {
            for value in ctx
                .admit_iter(
                    values.as_slice(),
                    "STEP collect validation references value traversal",
                )
                .map_err(cadmpeg_core::CodecError::from)?
            {
                collect_validation_references(value, validation_points, referenced, ctx)?;
            }
        }
        Value::Typed(_, value) => {
            collect_validation_references(value, validation_points, referenced, ctx)?;
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests;
