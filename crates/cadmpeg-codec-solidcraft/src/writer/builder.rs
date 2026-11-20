// SPDX-License-Identifier: Apache-2.0
//! Source-less generation: a `solidcraft/57` design written from the neutral
//! model alone.
//!
//! The builder writes the design parameters and the primitive features (box,
//! cylinder, cone, sphere, torus) in the order of their ordinals. It never
//! writes a `cache/`: `SolidCraft` evaluates the history on open and rebuilds
//! it. Any other feature, and geometry that no feature builds, has no
//! serializer yet and is refused by name instead of dropped. A feature's
//! dimensions are numbers in millimetres, so they do not follow a parameter.

use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::io::{Cursor, Write};

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    ArenaCoverage, ArenaDisposition, ArenaDispositions, Consumption, EncodeInput, ExportBody,
    WritePath,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::export::{CensusBasis, EntityCensus};
use serde_json::{json, Value};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, System, ZipWriter};

use crate::dialect::CURRENT_FORMAT;
use crate::package::{CONTAINER_FORMAT, DOCUMENT_MEMBER, MARKER_MEMBER};

/// How far a partial primitive's bound may be from the full range and still
/// count as the full one.
const FULL_RANGE_ROUNDING: f64 = 1.0e-9;
/// The name a design takes when the model records none.
const DEFAULT_NAME: &str = "Design";

/// The encoding of a generated design.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Encoding {
    /// A version 1 container.
    Container,
    /// A plain JSON document.
    Json,
}

fn refuse(reason: impl Into<String>) -> CodecError {
    CodecError::NotImplemented(reason.into())
}

/// A length as the expression text `SolidCraft` reads.
fn mm(value: f64) -> Value {
    Value::String(format!("{value} mm"))
}

fn number(value: &Value, key: &str) -> Result<f64, CodecError> {
    value
        .get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| refuse(format!("a primitive has no numeric {key}")))
}

fn near(value: f64, target: f64) -> bool {
    (value - target).abs() <= FULL_RANGE_ROUNDING
}

/// The column `index` of a 3 by 4 placement.
fn column(placement: &Value, index: usize) -> Result<[f64; 3], CodecError> {
    let rows = placement
        .as_array()
        .filter(|rows| rows.len() == 3)
        .ok_or_else(|| refuse("a primitive placement is not a 3 by 4 matrix"))?;
    let mut out = [0.0; 3];
    for (slot, row) in out.iter_mut().zip(rows) {
        *slot = row
            .get(index)
            .and_then(Value::as_f64)
            .ok_or_else(|| refuse("a primitive placement is not a 3 by 4 matrix"))?;
    }
    Ok(out)
}

fn is_identity_rotation(placement: &Value) -> Result<bool, CodecError> {
    let axes = [
        column(placement, 0)?,
        column(placement, 1)?,
        column(placement, 2)?,
    ];
    Ok(
        (0..3)
            .all(|axis| (0..3).all(|row| near(axes[axis][row], f64::from(u8::from(axis == row))))),
    )
}

fn point(values: [f64; 3]) -> Value {
    json!([values[0], values[1], values[2]])
}

/// The `SolidCraft` feature record of one neutral feature.
fn feature(neutral: &Value, id: u64, index: usize) -> Result<Value, CodecError> {
    let name = neutral.get("name").and_then(Value::as_str);
    let label = name.map_or_else(|| format!("feature {index}"), str::to_owned);
    let primitive = neutral
        .pointer("/definition/value/value")
        .filter(|_| {
            neutral
                .pointer("/definition/value/kind")
                .and_then(Value::as_str)
                == Some("primitive")
        })
        .ok_or_else(|| {
            refuse(format!(
                "{label} is not a primitive: this writer has a serializer for primitives only"
            ))
        })?;
    let solid = &primitive["solid"];
    let placement = &primitive["placement"];
    let operation = match primitive.get("op").and_then(Value::as_str) {
        Some(op @ ("new_body" | "join" | "cut" | "intersect")) => op,
        other => {
            return Err(refuse(format!(
                "{label} has the primitive operation {other:?}, which has no serializer"
            )))
        }
    };
    let origin = column(placement, 3)?;
    let axis = column(placement, 2)?;
    let kind = solid
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut fields = serde_json::Map::from_iter([
        ("id".to_owned(), json!(id)),
        (
            "name".to_owned(),
            json!(name.map_or_else(|| format!("{kind}{id}"), str::to_owned)),
        ),
        (
            "suppressed".to_owned(),
            json!(neutral.get("suppressed").and_then(Value::as_bool) == Some(true)),
        ),
        ("type".to_owned(), json!(kind)),
        ("operation".to_owned(), json!(operation)),
    ]);
    match kind {
        "box" => {
            if !is_identity_rotation(placement)? {
                return Err(refuse(format!(
                    "{label} is a rotated box, which has no serializer"
                )));
            }
            fields.insert("corner".into(), point(origin));
            fields.insert("length".into(), mm(number(solid, "length")?));
            fields.insert("width".into(), mm(number(solid, "width")?));
            fields.insert("height".into(), mm(number(solid, "height")?));
        }
        "cylinder" | "cone" => {
            if !near(number(solid, "angle")?, TAU) {
                return Err(refuse(format!(
                    "{label} is a partial {kind}, which has no serializer"
                )));
            }
            fields.insert("base".into(), point(origin));
            fields.insert("axis".into(), point(axis));
            fields.insert("height".into(), mm(number(solid, "height")?));
            if kind == "cylinder" {
                fields.insert("radius".into(), mm(number(solid, "radius")?));
            } else {
                fields.insert("radius".into(), mm(number(solid, "radius1")?));
                fields.insert("top_radius".into(), mm(number(solid, "radius2")?));
            }
        }
        "sphere" | "torus" => {
            let (low, high) = if kind == "sphere" {
                (-FRAC_PI_2, FRAC_PI_2)
            } else {
                (-PI, PI)
            };
            if !near(number(solid, "latitude1")?, low)
                || !near(number(solid, "latitude2")?, high)
                || !near(number(solid, "longitude")?, TAU)
            {
                return Err(refuse(format!(
                    "{label} is a partial {kind}, which has no serializer"
                )));
            }
            if !near(axis[0], 0.0) || !near(axis[1], 0.0) || !near(axis[2], 1.0) {
                return Err(refuse(format!(
                    "{label} is a tilted {kind}, which has no serializer"
                )));
            }
            fields.insert("center".into(), point(origin));
            if kind == "sphere" {
                fields.insert("radius".into(), mm(number(solid, "radius")?));
            } else {
                fields.insert("major".into(), mm(number(solid, "major_radius")?));
                fields.insert("minor".into(), mm(number(solid, "minor_radius")?));
            }
        }
        other => {
            return Err(refuse(format!(
                "{label} is a {other:?} primitive, which has no serializer"
            )))
        }
    }
    Ok(Value::Object(fields))
}

/// The document of `ir`.
fn document(ir: &CadIr) -> Result<Value, CodecError> {
    let mut params = Vec::new();
    for parameter in &ir.model.parameters {
        let mut record = json!({
            "name": parameter.name,
            "expr": parameter.expression,
            "model": false,
        });
        if let Some(unit) = parameter
            .properties
            .iter()
            .find_map(|(key, value)| (key.as_str() == "unit").then_some(value))
        {
            record["unit"] = Value::String(unit.clone());
        }
        params.push(record);
    }
    let mut features: Vec<_> = ir
        .model
        .features
        .iter()
        .map(|feature| serde_json::to_value(feature).map_err(|error| refuse(error.to_string())))
        .collect::<Result<_, _>>()?;
    features.sort_by_key(|neutral| neutral.get("ordinal").and_then(Value::as_u64));
    if features.is_empty() && ir.census().values().any(|count| *count > 0) {
        return Err(refuse(
            "the model holds geometry or records but no feature history, and this writer \
             cannot generate a SolidCraft feature from a B-rep alone",
        ));
    }
    let records = features
        .iter()
        .enumerate()
        .map(|(index, neutral)| feature(neutral, u64_from_index(index) + 1, index))
        .collect::<Result<Vec<_>, _>>()?;
    let name = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("document_name"))
        .map_or(DEFAULT_NAME, String::as_str);
    Ok(json!({
        "format": format!("{}{CURRENT_FORMAT}", crate::document::FORMAT_PREFIX),
        "name": name,
        "units": "mm",
        "params": params,
        "features": records,
        "appearances": {"bodies": {}},
    }))
}

fn zip_error(error: &zip::result::ZipError) -> CodecError {
    CodecError::malformed(format_args!("SolidCraft container: {error}"))
}

fn options(method: CompressionMethod) -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(method)
        .system(System::Unix)
}

/// What the builder does with each model arena: the parameters and features
/// are written; the rest is omitted and charged by the sealed wrapper.
///
/// The root product and its one occurrence are what every design has: a
/// design with only those is written whole, since `SolidCraft` makes them on
/// open. More than one is a component tree this writer has no serializer for.
fn coverage(ir: &CadIr) -> ArenaDispositions {
    use ArenaDisposition::{Omitted, Written};
    ArenaDispositions {
        bodies: Omitted,
        regions: Omitted,
        shells: Omitted,
        faces: Omitted,
        loops: Omitted,
        coedges: Omitted,
        edges: Omitted,
        vertices: Omitted,
        points: Omitted,
        surfaces: Omitted,
        curves: Omitted,
        subds: Omitted,
        pcurves: Omitted,
        procedural_surfaces: Omitted,
        procedural_curves: Omitted,
        assets: Omitted,
        features: Written,
        feature_input_topologies: Omitted,
        feature_result_topologies: Omitted,
        configurations: Omitted,
        parameters: Written,
        sketches: Omitted,
        sketch_entities: Omitted,
        sketch_constraints: Omitted,
        spatial_sketches: Omitted,
        spatial_sketch_entities: Omitted,
        spatial_sketch_constraints: Omitted,
        spreadsheets: Omitted,
        product_definitions: if ir.model.product_definitions.len() <= 1 {
            Written
        } else {
            Omitted
        },
        occurrences: if ir.model.occurrences.len() <= 1 {
            Written
        } else {
            Omitted
        },
        assembly_joints: Omitted,
        drawings: Omitted,
        semantic_annotations: Omitted,
        presentation_documents: Omitted,
        view_presentations: Omitted,
        tessellations: Omitted,
        point_clouds: Omitted,
        history_bodies: ArenaDisposition::Ignored,
        appearances: Omitted,
        appearance_bindings: Omitted,
        attributes: Omitted,
        pmi: Omitted,
        presentation_layers: Omitted,
        physical_materials: Omitted,
        mass_properties: Omitted,
        rigid_bodies: Omitted,
        colliders: Omitted,
        articulations: Omitted,
        physics_scenes: Omitted,
        solver_settings: Omitted,
        declared_measures: Omitted,
        design: cadmpeg_ir::codec::write::DesignArenaDispositions::uniform(Omitted),
        process: cadmpeg_ir::codec::write::ProcessArenaDispositions::uniform(Omitted),
    }
}

/// Generate a design of `encoding` from the model alone.
pub(super) fn write(
    input: EncodeInput<'_>,
    encoding: Encoding,
    consumption: Consumption,
) -> Result<ExportBody, CodecError> {
    let document = document(input.ir)?;
    let bytes = match encoding {
        Encoding::Json => {
            let mut out =
                serde_json::to_vec_pretty(&document).map_err(|error| refuse(error.to_string()))?;
            out.push(b'\n');
            out
        }
        Encoding::Container => {
            let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
            zip.start_file(MARKER_MEMBER, options(CompressionMethod::Stored))
                .map_err(|error| zip_error(&error))?;
            zip.write_all(format!("{{\"format\":\"{CONTAINER_FORMAT}\"}}").as_bytes())?;
            zip.start_file(DOCUMENT_MEMBER, options(CompressionMethod::Deflated))
                .map_err(|error| zip_error(&error))?;
            zip.write_all(
                &serde_json::to_vec(&document).map_err(|error| refuse(error.to_string()))?,
            )?;
            zip.finish()
                .map_err(|error| zip_error(&error))?
                .into_inner()
        }
    };
    Ok(ExportBody {
        bytes,
        census: EntityCensus {
            basis: CensusBasis::IrArenas,
            counts: input.ir.census(),
        },
        write_path: WritePath::Synthesized { consumption },
        coverage: ArenaCoverage::Declared(coverage(input.ir)),
        losses: Vec::new(),
        notes: vec![format!(
            "a {CURRENT_FORMAT} design was generated from {} parameters and {} features",
            input.ir.model.parameters.len(),
            input.ir.model.features.len()
        )],
    })
}
