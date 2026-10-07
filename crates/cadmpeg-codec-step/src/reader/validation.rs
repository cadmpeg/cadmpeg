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

use super::decode_text_scoped;
use super::geometry::GeometryData;
use super::StageOutcome;
use super::{RecordExt, ValueExt};

#[derive(Clone, Copy)]
enum Expected {
    Area(f64),
    Volume(f64),
    Centroid(Point3),
}

pub(super) fn decode<'ctx>(
    exchange: &Exchange,
    geometry: &GeometryData,
    ir: &mut CadIr,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<StageOutcome<(cadmpeg_core::decode::ScopedReservation<'ctx>, cadmpeg_core::decode::ScopedReservation<'ctx>)>, CodecError> {
    let slot_storage = std::cell::RefCell::new(ctx.reserve_scoped(0, "STEP stage report buffers")?);
    let mut claim_storage = ctx.reserve_scoped(0, "STEP stage claim storage")?;
    let mut scratch_storage = ctx.reserve_scoped(0, "STEP decode scratch")?;
    if !exchange.has_entity(ctx, "PROPERTY_DEFINITION")?
        || !exchange.has_entity(ctx, "PROPERTY_DEFINITION_REPRESENTATION")?
    {
        return Ok(StageOutcome {
            value: (claim_storage, slot_storage.into_inner()),
            claims: BTreeSet::new(),
            notes: Vec::new(),
            losses: Vec::new(),
        });
    }
    let mut losses = Vec::new();
    let mut representations = BTreeMap::new();
    for (&id, record) in ctx.admit_iter(exchange.records(), "STEP decode traversal")? {
        let Some(representation_items) = super::representation::items(ctx, record)? else {
            continue;
        };
        let mut items = BTreeSet::new();
        for item in representation_items {
            scratch_storage.with_storage(|| ctx.insert_btree_set(&mut items, item, "step_validation_representation_items"))?;
        }
        if !items.is_empty() {
            scratch_storage.with_storage(|| ctx.insert_btree_map(&mut representations,
                id,
                items,
                "step_validation_representations",
            ))?;
        }
    }
    let mut properties = BTreeMap::new();
    for (id, record) in exchange.entities(ctx, "PROPERTY_DEFINITION")? {
        let Some(property) = record.partial(ctx, "PROPERTY_DEFINITION")? else {
            continue;
        };
        let mut name_storage = ctx.reserve_scoped(0, "STEP validation property name scratch")?;
        let name = property
            .parameters
            .first()
            .map(|value| {
                decode_text_scoped(
                    exchange,
                    value,
                    (&mut losses, &slot_storage),
                    id,
                    ("validation property name", StepLossCode::MetadataStringInvalid),
                    ctx,
                    &mut name_storage,
                )
            })
            .transpose()?
            .flatten();
        let Some(name) = name else {
            continue;
        };
        if name.as_str().eq_ignore_ascii_case("geometric validation property") {
            let description = property
                .parameters
                .get(1)
                .map(|value| {
                    decode_text_scoped(
                        exchange,
                        value,
                        (&mut losses, &slot_storage),
                        id,
                        ("validation property description", StepLossCode::MetadataStringInvalid),
                        ctx,
                        &mut scratch_storage,
                    )
                })
                .transpose()?
                .flatten()
                .unwrap_or_default();
            scratch_storage.with_storage(|| ctx.insert_btree_map(&mut properties,
                id,
                description,
                "step_validation_properties",
            ))?;
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
        let Some(description) = ctx.get_btree_map(&properties, &property_id, "STEP validation properties get")? else {
            continue;
        };
        let Some(representation_id) = relation.parameters.get(1).and_then(ValueExt::reference)
        else {
            continue;
        };
        let Some(item_ids) = ctx.get_btree_map(&representations, &representation_id, "STEP validation representations get")? else {
            continue;
        };
        scratch_storage.with_storage(|| ctx.insert_btree_set(&mut validation_representations,
            representation_id,
            "step_validation_used_representations",
        ))?;
        for &item_id in ctx.admit_iter(item_ids, "STEP validation item traversal")? {
            let Some(item) = ctx.get_btree_map(exchange.records(), &item_id, "STEP validation record get")? else {
                continue;
            };
            let scale = geometry.units.length([item_id, representation_id]).get();
            let expected = expected_value(item_id, item, exchange, scale, (&mut losses, &slot_storage), ctx)?;
            let Some(expected) = expected else {
                push_validation_loss(
                    (&mut losses, &slot_storage),
                    StepLossCode::DecodeWarning,
                    format!(
                        "geometric validation property #{property_id} has unsupported item #{item_id}"
                    ),
                    ctx,
                )?;
                continue;
            };
            if matches!(expected, Expected::Centroid(_)) {
                scratch_storage.with_storage(|| ctx.insert_btree_set(&mut validation_points, item_id, "step_validation_points"))?;
            }
            for id in [property_id, relation_id, representation_id, item_id] {
                claim_storage.with_storage(|| ctx.insert_btree_set(&mut typed, id, "step_validation_claims"))?;
            }
            if let Some(unit) = measure_unit(ctx, item)? {
                claim_storage.with_storage(|| collect_unit_records(unit, exchange, &mut typed, ctx))?;
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
                ctx.push_scoped_vec(&mut slot_storage.borrow_mut(), &mut notes, ctx.format_retained(format_args!(
                        "geometric validation {kind} {description}: expected {expected_text}, tessellation approximation {actual_text}"
                    ), "step_validation_note_text")?, "step_validation_notes")?;
            } else {
                ctx.push_scoped_vec(&mut slot_storage.borrow_mut(), &mut notes, ctx.format_retained(format_args!(
                        "geometric validation {kind} {description}: expected {expected_text}"
                    ), "step_validation_note_text")?, "step_validation_notes")?;
            }
        }
    }
    let mut referenced_validation_points = BTreeSet::new();
    if !validation_points.is_empty() {
        for (&record_id, record) in ctx.admit_iter(exchange.records(), "STEP decode traversal")? {
            if ctx.contains_btree_set(&validation_representations, &record_id, "STEP validation validation_representations contains")? {
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
                    scratch_storage.with_storage(|| collect_validation_references(
                        value,
                        &validation_points,
                        &mut referenced_validation_points,
                        ctx,
                    ))?;
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
            Ok(!ctx.contains_btree_set(&validation_points, &id, "STEP validation validation_points contains")? || ctx.contains_btree_set(&referenced_validation_points, &id, "STEP validation referenced_validation_points contains")?)
        },
        "STEP validation point retention",
    )?;
    Ok(StageOutcome {
        value: (claim_storage, slot_storage.into_inner()),
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
    (losses, slot_storage): (&mut Vec<LossNote>, &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>),
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
    let measure = super::find_record_value(record, ctx, |value| area_or_volume_measure(ctx, value))?;
    let Some((kind, value)) = measure else {
        return Ok(None);
    };
    let scale = measure_scale(id, record, exchange, scale, kind, (losses, slot_storage), ctx)?;
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
    (losses, slot_storage): (&mut Vec<LossNote>, &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>),
    ctx: &DecodeContext<'_>,
) -> Result<f64, CodecError> {
    let resolved = measure_unit(ctx, record)?
        .map(|unit| ctx.get_btree_map(exchange.records(), &unit, "STEP validation record get"))
        .transpose()?
        .flatten()
        .map(|record| derived_unit_elements(ctx, record))
        .transpose()?
        .flatten()
        .and_then(ValueExt::list);
    let resolved = if let Some(elements) = resolved {
        let mut scale = Some(1.0);
        ctx.all_by(elements, |element| {
            let element = element
                .reference()
                .map(|id| ctx.get_btree_map(exchange.records(), &id, "STEP validation record get"))
        .transpose()?
        .flatten();
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
                return Ok(false);
            };
            let (base, _unit_storage) = ctx.with_scoped_storage("STEP validation unit resolver scratch", || super::geometry::unit_scale_mm(base, exchange, &mut BTreeSet::new(), ctx))?;
            let Some(base) = base else {
                scale = None;
                return Ok(false);
            };
            scale = scale.map(|scale| scale * base.get().powf(exponent));
            Ok(true)
        }, "STEP validation elements traversal")?;
        scale
    } else {
        None
    };
    match resolved {
        Some(scale) => Ok(scale),
        None => {
            push_validation_loss(
                (losses, slot_storage),
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
    (losses, slot_storage): (&mut Vec<LossNote>, &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>),
    code: StepLossCode,
    message: String,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    slot_storage.borrow_mut().with_storage(|| ctx.reserve_vec(losses, 1, "step_validation_losses"))?;
    losses.push(code.note(message));
    Ok(())
}

fn area_or_volume_measure(
    ctx: &DecodeContext<'_>,
    value: &Value,
) -> Result<Option<(&'static str, f64)>, CodecError> {
    let _depth = ctx.enter_nested("STEP validation measure nesting")?;
    match value {
        Value::Typed(kind, value) if matches!(kind.as_str(), "AREA_MEASURE" | "VOLUME_MEASURE") => {
            Ok(value.number().map(|value| (if kind == "AREA_MEASURE" { "AREA_MEASURE" } else { "VOLUME_MEASURE" }, value)))
        }
        Value::Typed(_, value) => area_or_volume_measure(ctx, value),
        Value::List(values) => ctx.find_map(values.as_slice(), |value| area_or_volume_measure(ctx, value), "STEP validation measure list traversal"),
        _ => Ok(None),
    }
}

fn measure_unit(ctx: &DecodeContext<'_>, record: &RawRecord) -> Result<Option<u64>, CodecError> {
    if let Some(partial) = record.partial(ctx, "MEASURE_WITH_UNIT")? {
        let unit = ctx.find_map(
            partial.parameters.as_slice().iter().rev(),
            |value| Ok(ValueExt::reference(value)),
            "STEP validation measure unit traversal",
        )?;
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
        if let Some(partial) = record.partial(ctx, name)? {
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
    let Some(record) = ctx.get_btree_map(exchange.records(), &id, "STEP validation record get")? else {
        return Ok(());
    };
    let Some(elements) = derived_unit_elements(ctx, record)?.and_then(ValueExt::list) else {
        return Ok(());
    };
    for element in ctx.admit_iter(elements, "STEP derived unit element traversal")?.filter_map(ValueExt::reference) {
        ctx.insert_btree_set(typed, element, "step_validation_claims")?;
        if let Some(base) = ctx.get_btree_map(exchange.records(), &element, "STEP validation record get")?
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
    let (mut meshes, mut mesh_storage) =
        ctx.temporary_vec(0, "STEP validation selected meshes")?;
    let mut origin = None;
    for mesh in ctx.admit_iter(
        &ir.model.tessellations,
        "STEP validation mesh selection traversal",
    )? {
        if !ctx.equal(
            &mesh.body.as_ref(),
            &Some(body),
            "STEP validation mesh body equality",
        )? {
            continue;
        }
        ctx.push_scoped_vec(
            &mut mesh_storage,
            &mut meshes,
            mesh,
            "STEP validation selected meshes",
        )?;
        if origin.is_none() {
            origin = mesh.triangles().first().and_then(|triangle| {
                mesh.vertices()
                    .get(cadmpeg_core::decode::index_from_u32(triangle[0]))
                    .copied()
            });
        }
    }
    let Some(origin) = origin else {
        return Ok(None);
    };
    let mut extent = 0.0_f64;
    for mesh in ctx.admit_iter(&meshes, "STEP validation mesh extent traversal")? {
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
    for mesh in ctx.admit_iter(&meshes, "STEP validation mesh property traversal")? {
        let mut edge_storage = ctx.reserve_scoped(0, "STEP mesh edge scratch")?;
        let mut edge_uses = BTreeMap::<(u32, u32), usize>::new();
        for triangle in ctx.admit_iter(mesh.triangles(), "step_validation_mesh_triangles")? {
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

                match edge_storage.with_storage(|| ctx.entry_btree_map(&mut edge_uses, edge, "step_validation_mesh_edges"))? {
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
            && ctx.all_by(
                edge_uses.values(),
                |uses| Ok(*uses == 2),
                "STEP mesh properties map traversal",
            )?;
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
    let number = ctx.rsplit_once(id, "#", "STEP validation point identity split")?.map_or(id, |(_, number)| number);
    Ok(ctx.parse_text::<u64>(number, "STEP validation point number parse")?.ok())
}

fn collect_validation_references(
    value: &Value,
    validation_points: &BTreeSet<u64>,
    referenced: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let _nested = ctx.enter_nested("step_validation_reference_walk")?;
    match value {
        Value::Reference(id) if ctx.contains_btree_set(&validation_points, id, "STEP validation validation_points contains")? => {
            ctx.insert_btree_set(referenced, *id, "step_validation_referenced_points")?;
        }
        Value::List(values) => {
            for value in ctx.admit_iter(
                values.as_slice(),
                "STEP collect validation references value traversal",
            )? {
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
