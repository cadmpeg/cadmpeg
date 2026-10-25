// SPDX-License-Identifier: Apache-2.0
//! Source-free emission of polygon tables alongside current neutral topology.

use super::super::xml::{self, Object};
use crate::brep::records::{BrepRow, BrepValue};
use crate::brep::{ShapePayloadRecord, TextPolygon3d, TextPolygonOnTriangulation};
use crate::native::PropertyRecord;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{NativeDisposition, NativeDispositions};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, HashMap};
pub(in crate::writer) mod gui;
mod merge;

pub(in crate::writer) fn write(
    ir: &CadIr,
    objects: &mut [Object],
    names: &HashMap<String, String>,
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
    entries: &mut BTreeMap<String, Vec<u8>>,
    polygon_homes: &HashMap<String, HashMap<String, Vec<usize>>>,
    native_only: bool,
    dispositions: &mut NativeDispositions,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<std::collections::HashSet<String>, CodecError> {
    let mut emitted = std::collections::HashSet::new();
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(emitted);
    };
    let all_rows = crate::brep::records::polygons::polygon_rows(namespace, payloads)?;
    let mut lists = BTreeMap::<String, Vec<String>>::new();
    for payload in payloads {
        let Some(property) = properties
            .iter()
            .find(|property| property.id == payload.property)
        else {
            continue;
        };
        let Some(name) = names.get(&property.owner) else {
            continue;
        };
        let Some(object) = objects.iter_mut().find(|object| &object.name == name) else {
            continue;
        };
        for id in payload.polygon_references() {
            if !all_rows
                .iter()
                .any(|row| row.id == id && row.owner == payload.id)
            {
                losses.push(
                    crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                        "B-rep payload {} omits missing polygon row {id}",
                        payload.id
                    )),
                );
            }
        }
        let ordinal = lists.get(&property.id).map_or(0, Vec::len);
        let file = if property.type_name == "Part::PropertyTopoShapeList" {
            format!(
                "{name}.{}.{ordinal}.brp",
                cadmpeg_ir::ids::IdentityKey::encode_segment(&property.name)
            )
        } else if property.name == "Shape" {
            format!("{name}.brp")
        } else {
            format!(
                "{name}.{}.brp",
                cadmpeg_ir::ids::IdentityKey::encode_segment(&property.name)
            )
        };
        let mut current_homes = HashMap::new();
        if !native_only {
            if let Some(serialized) = super::serialize_payload(ir, payload)? {
                emitted.extend(serialized.meshes);
                current_homes = serialized.polygon_homes;
                entries.insert(file.clone(), serialized.bytes);
            } else {
                entries.insert(file.clone(), empty_carrier(payload));
                if ir
                    .model
                    .bodies
                    .iter()
                    .any(|body| body.native_ref.as_deref() == Some(payload.id.as_str()))
                {
                    losses.push(crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!("B-rep payload {} has a current body without an OCCT encoding and is omitted",payload.id)));
                }
            }
        }
        let bytes = entries
            .entry(file.clone())
            .or_insert_with(|| empty_carrier(payload));
        let homes = if native_only {
            polygon_homes.get(&file).unwrap_or(&current_homes)
        } else {
            &current_homes
        };
        let mut has_geometry = false;
        emitted.extend(merge::merge(
            ir,
            payload,
            &all_rows,
            bytes,
            homes,
            native_only,
            &mut has_geometry,
            dispositions,
            losses,
        )?);
        {
            if property.type_name == "Part::PropertyTopoShapeList" {
                lists
                    .entry(property.id.clone())
                    .or_default()
                    .push(xml::element("TopoShape", &[("file", file)]));
            } else {
                object.set(
                    &property.name,
                    xml::property(
                        &property.name,
                        "Part::PropertyPartShape",
                        &xml::element("Part", &[("file", file)]),
                    ),
                );
            }
            if has_geometry {
                object.mark_geometry_written();
            }
        }
    }
    for (id, values) in lists {
        let Some(property) = properties.iter().find(|property| property.id == id) else {
            continue;
        };
        let Some(name) = names.get(&property.owner) else {
            continue;
        };
        if let Some(object) = objects.iter_mut().find(|object| &object.name == name) {
            object.set(
                &property.name,
                xml::property(
                    &property.name,
                    "Part::PropertyTopoShapeList",
                    &format!(
                        "<ShapeList count=\"{}\">{}</ShapeList>",
                        values.len(),
                        values.join("")
                    ),
                ),
            );
        }
    }
    Ok(emitted)
}

fn empty_carrier(payload: &ShapePayloadRecord) -> Vec<u8> {
    if matches!(payload.payload, crate::brep::ShapePayload::Empty) {
        Vec::new()
    } else {
        b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*\n".to_vec()
    }
}

fn polygon3d(value: &TextPolygon3d) -> String {
    let mut output = format!(
        "{} {} {}",
        value.nodes.len(),
        u8::from(value.parameters.is_some()),
        value.deflection.get()
    );
    for point in &value.nodes {
        output.push_str(&format!(" {}", super::geometry::point(point.get())));
    }
    if let Some(parameters) = &value.parameters {
        for parameter in parameters {
            output.push_str(&format!(" {}", parameter.get()));
        }
    }
    output
}

fn indexed_polygon(value: &TextPolygonOnTriangulation) -> String {
    let mut output = value.nodes.len().to_string();
    for node in &value.nodes {
        output.push_str(&format!(" {node}"));
    }
    output.push_str(&format!(
        " p {} {}",
        value.deflection.get(),
        u8::from(value.parameters.is_some())
    ));
    if let Some(parameters) = &value.parameters {
        for parameter in parameters {
            output.push_str(&format!(" {}", parameter.get()));
        }
    }
    output
}

#[cfg(test)]
fn replace_table(bytes: &mut Vec<u8>, marker: &[u8], next: &[u8], rows: &[String]) -> bool {
    let Some(start) = bytes
        .windows(marker.len())
        .position(|window| window == marker)
    else {
        return false;
    };
    let Some(after) = bytes[start + marker.len()..]
        .windows(next.len())
        .position(|window| window == next)
    else {
        return false;
    };
    let end = start + marker.len() + after;
    let mut replacement = marker.to_vec();
    replacement.extend_from_slice(rows.len().to_string().as_bytes());
    replacement.push(b'\n');
    for row in rows {
        replacement.extend_from_slice(row.as_bytes());
        replacement.push(b'\n');
    }
    bytes.splice(start..end, replacement);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn polygon_tables_write_current_typed_values_without_an_archive_or_model_scope_gate() {
        let bytes = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*\n";
        let mut output = bytes.to_vec();
        let polygon = TextPolygon3d {
            deflection: cadmpeg_ir::scalar::NonNegativeReal::ZERO,
            nodes: vec![cadmpeg_ir::features::FinitePoint3::ZERO],
            parameters: Some(vec![cadmpeg_ir::scalar::FiniteReal::ONE]),
        };
        assert!(replace_table(
            &mut output,
            b"Polygon3D ",
            b"PolygonOnTriangulations ",
            &[polygon3d(&polygon)]
        ));
        assert!(String::from_utf8(output)
            .unwrap()
            .contains("Polygon3D 1\n1 1 0 0 0 0 1\nPolygonOnTriangulations 0"));
    }

    #[test]
    fn explicit_polygon_row_edits_and_deletions_control_source_free_output_at_full_model_scope() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut ir = CadIr::empty();
        let payload_id = "fcstd:native:shape-payload#polygon";
        let property = PropertyRecord {
            id: "fcstd:native:property#Shape".to_owned(),
            owner: "fcstd:native:object#Shape".to_owned(),
            name: "Shape".to_owned(),
            type_name: "Part::PropertyPartShape".to_owned(),
            family: crate::native::PropertyFamily::Geometry,
            dynamic: None,
            status: None,
            body: Some(crate::native::PropertyBody::Persisted {
                values: vec![].into(),
                links: vec![],
                side_entries: vec!["Shape.brp".to_owned()],
            }),
            order: 0,
        };
        let current = TextPolygon3d {
            deflection: cadmpeg_ir::scalar::NonNegativeReal::ZERO,
            nodes: vec![cadmpeg_ir::features::FinitePoint3::from_coordinates(
                cadmpeg_ir::scalar::FiniteReal::ONE,
                cadmpeg_ir::scalar::FiniteReal::ZERO,
                cadmpeg_ir::scalar::FiniteReal::ZERO,
            )],
            parameters: None,
        };
        let row = crate::brep::records::polygons::PolygonRecord {
            id: "fcstd:native:polygons3d#polygon".to_owned(),
            value: current,
        };
        let payload: ShapePayloadRecord = serde_json::from_value(serde_json::json!({
            "id":payload_id,"property":property.id,"entry":"fcstd:native:entry#Shape.brp","form":"binary","text":null,
            "binary":{"topology_version":4,"locations":[],"curve2ds":[],"curves":[],"surfaces":[],"polygons3d":[row.id],"polygons_on_triangulations":[],"triangulations":[],"tshapes":[],"roots":[]}
        })).unwrap();
        ir.native
            .namespace_mut("fcstd")
            .set_arena(&ctx, "polygons3d", &[row.clone()])
            .unwrap();
        ir.native
            .namespace_mut("fcstd")
            .set_arena(&ctx, "shape_payloads", &[payload.clone()])
            .unwrap();
        let names = HashMap::from([(property.owner.clone(), "Shape".to_owned())]);
        let mut objects = vec![Object::new("Shape".to_owned(), "Part::Feature", "Shape")];
        let mut entries = BTreeMap::new();
        let mut dispositions = super::super::super::dispositions::initial();
        write(
            &ir,
            &mut objects,
            &names,
            &[property.clone()],
            &crate::brep::records::payloads_untracked(ir.native.namespace("fcstd").unwrap())
                .unwrap(),
            &mut entries,
            &HashMap::new(),
            false,
            &mut dispositions,
            &mut Vec::new(),
        )
        .unwrap();
        assert!(ir.scope.model);
        assert!(ir.source.is_none());
        let text = std::str::from_utf8(&entries["Shape.brp"]).unwrap();
        assert!(text.contains("Polygon3D 1\n1 0 0 1 0 0\n"));
        assert_eq!(
            dispositions.record_disposition("fcstd", "polygons3d", &row.id),
            NativeDisposition::Written
        );
        ir.native
            .namespace_mut("fcstd")
            .set_arena::<crate::brep::records::polygons::Polygon3dRecord>(&ctx, "polygons3d", &[])
            .unwrap();
        entries.clear();
        dispositions = super::super::super::dispositions::initial();
        write(
            &ir,
            &mut objects,
            &names,
            &[property],
            &crate::brep::records::payloads_untracked(ir.native.namespace("fcstd").unwrap())
                .unwrap(),
            &mut entries,
            &HashMap::new(),
            false,
            &mut dispositions,
            &mut Vec::new(),
        )
        .unwrap();
        assert!(std::str::from_utf8(&entries["Shape.brp"])
            .unwrap()
            .contains("Polygon3D 0\n"));
    }
}
