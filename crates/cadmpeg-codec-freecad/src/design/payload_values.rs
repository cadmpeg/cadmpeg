// SPDX-License-Identifier: Apache-2.0
//! Binary design operands use original suppliers for source-backed projections.

use crate::native::PropertyRecord;
use crate::property_reader::PropertyReader;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::scalar::FiniteReal;

fn file(
    reader: &PropertyReader<'_, '_>,
    property: &PropertyRecord,
    kind: &str,
    tag: &str,
) -> Result<Option<bool>, CodecError> {
    if property.type_name != kind {
        return Ok(None);
    }
    let mut roots = property
        .values()
        .iter()
        .filter(|value| value.parent.is_none());
    let Some(root) = roots.next() else {
        return Ok(None);
    };
    if roots.next().is_some() || root.tag != tag {
        return Ok(None);
    };
    let source = reader.feature_field(property, tag, "file")?;
    let file = match &source {
        Some(file) => file.as_ref(),
        None => root.attributes.get("file"),
    };
    let Some(file) = file else { return Ok(None) };
    if file.is_empty() {
        return Ok(property.side_entries().is_empty().then_some(true));
    }
    Ok(Some(false))
}

pub(super) fn vectors(
    reader: &PropertyReader<'_, '_>,
    property: &PropertyRecord,
) -> Result<Option<Vec<FinitePoint3>>, CodecError> {
    let Some(empty) = file(reader, property, "App::PropertyVectorList", "VectorList")? else {
        return Ok(None);
    };
    if empty {
        return Ok(Some(Vec::new()));
    }
    let Some(rows) = reader
        .vector_slots(property)
        .filter(|rows| rows.len() <= super::MAX_SKETCH_RECORDS && rows.iter().all(Option::is_some))
    else {
        return Ok(None);
    };
    let header = if reader.has_payload_sources() {
        reader.feature_payload_header(property)?
    } else {
        None
    };
    let count = header.as_ref().map_or(rows.len(), |header| header.count());
    if count != rows.len() || (reader.has_payload_sources() && header.is_none()) {
        return Err(CodecError::malformed(
            "feature vector has no matching original header",
        ));
    }
    let mut values =
        reader.alloc_filled(count, FinitePoint3::ZERO, "FreeCAD feature vector operands")?;
    for (ordinal, (slot, row)) in values.iter_mut().zip(rows).enumerate() {
        let vector = if let Some(header) = &header {
            reader.feature_vector(property, ordinal, header)?
        } else {
            cadmpeg_ir::units::FiniteVector::new(
                row.ok_or_else(|| CodecError::malformed("feature vector slot is absent"))?
                    .map(FiniteReal::get),
            )
            .ok_or_else(|| CodecError::malformed("held feature vector is not finite"))?
        };
        *slot = FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            vector.get()[0],
            vector.get()[1],
            vector.get()[2],
        ))
        .ok_or_else(|| CodecError::malformed("feature point is not finite"))?;
    }
    Ok(Some(values))
}

pub(super) fn numbers(
    reader: &PropertyReader<'_, '_>,
    property: &PropertyRecord,
) -> Result<Option<Vec<FiniteReal>>, CodecError> {
    let Some(empty) = file(reader, property, "App::PropertyFloatList", "FloatList")? else {
        return Ok(None);
    };
    if empty {
        return Ok(Some(Vec::new()));
    }
    let Some(rows) = reader
        .float_slots(property)
        .filter(|rows| rows.len() <= super::MAX_SKETCH_RECORDS && rows.iter().all(Option::is_some))
    else {
        return Ok(None);
    };
    let header = if reader.has_payload_sources() {
        reader.feature_payload_header(property)?
    } else {
        None
    };
    let count = header.as_ref().map_or(rows.len(), |header| header.count());
    if count != rows.len() || (reader.has_payload_sources() && header.is_none()) {
        return Err(CodecError::malformed(
            "feature scalar has no matching original header",
        ));
    }
    let mut values =
        reader.alloc_filled(count, FiniteReal::ZERO, "FreeCAD feature scalar operands")?;
    for (ordinal, (slot, row)) in values.iter_mut().zip(rows).enumerate() {
        *slot = if let Some(header) = &header {
            reader.feature_float(property, ordinal, header)?
        } else {
            row.ok_or_else(|| CodecError::malformed("feature scalar slot is absent"))?
        };
    }
    Ok(Some(values))
}

pub(super) fn fillets(
    reader: &PropertyReader<'_, '_>,
    property: &PropertyRecord,
) -> Result<Option<Vec<[f64; 2]>>, CodecError> {
    let Some(empty) = file(reader, property, "Part::PropertyFilletEdges", "FilletEdges")? else {
        return Ok(None);
    };
    if empty {
        return Ok(Some(Vec::new()));
    }
    let Some(rows) = reader
        .fillet_slots(property)
        .filter(|rows| rows.len() <= super::MAX_SKETCH_RECORDS && rows.iter().all(Option::is_some))
    else {
        return Ok(None);
    };
    let header = if reader.has_payload_sources() {
        reader.feature_payload_header(property)?
    } else {
        None
    };
    let count = header.as_ref().map_or(rows.len(), |header| header.count());
    if count != rows.len() || (reader.has_payload_sources() && header.is_none()) {
        return Err(CodecError::malformed(
            "feature fillet has no matching original header",
        ));
    }
    let mut values = reader.alloc_filled(count, [0.0; 2], "FreeCAD feature fillet operands")?;
    for (ordinal, (slot, row)) in values.iter_mut().zip(rows).enumerate() {
        *slot = if let Some(header) = &header {
            reader.feature_fillets(property, ordinal, header)?
        } else {
            let row = row
                .as_ref()
                .ok_or_else(|| CodecError::malformed("feature fillet slot is absent"))?;
            [row.radius1.get(), row.radius2.get()]
        };
    }
    Ok(Some(values))
}
