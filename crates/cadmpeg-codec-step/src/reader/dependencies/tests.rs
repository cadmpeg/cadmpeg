// SPDX-License-Identifier: Apache-2.0
//! STEP external-document dependency tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
#![allow(unused_imports)]

use std::fmt::Write as _;
use std::io::Cursor;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use cadmpeg_core::decode::{DecodeMode, InspectOptions, View};
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::eval::{
    model_curve_point_by_id, model_surface_partials_by_id, model_surface_point_by_id, pcurve_uv,
};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::{LengthUnit, Units};
use cadmpeg_ir::CadIr;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::ids::StepIdentity;
use crate::test_support::{decode_inline, export};
use crate::{
    write_step, StepCodec, StepError, StepSchema, StepUnsupportedPolicy, StepWriteOptions,
};

#[test]
pub(crate) fn decode_reports_data_section_external_dependencies() {
    let bytes = include_bytes!("../../../tests/fixtures/ap242_external_documents.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode external document dependencies");

    assert!(result.report().notes.contains(
        &"external document SPEC-42 (Interface control drawing) from supplier vault".into()
    ));
    assert!(result
        .report()
        .notes
        .contains(&"external source https://example.invalid/library item fastener-table".into()));

    let summary = StepCodec::default()
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("inspect external document dependencies");
    let dependencies = summary
        .entries
        .iter()
        .find(|entry| entry.name == "EXTERNAL_DEPENDENCIES")
        .expect("external dependency inventory");
    assert_eq!(dependencies.attributes["dependency_count"], "2");
}

#[test]
fn standalone_relative_uri_is_retained_without_filesystem_resolution() {
    let bytes = include_bytes!("../../parse/tests/data/er01_standalone_relative_uri.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode standalone URI witness without a transport base");

    assert!(result
        .report()
        .notes
        .contains(&"external reference #10 -> parts/child.p21#target".into()));
    assert!(result
        .report()
        .notes
        .contains(&"external document doc-id (doc-name) from parts/document.p21#target".into()));

    let summary = StepCodec::default()
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("inspect standalone URI witness");
    let references = summary
        .entries
        .iter()
        .find(|entry| entry.name == "REFERENCE")
        .expect("reference inventory");
    assert_eq!(
        references.attributes["external_uris"],
        "parts/child.p21#target,#local_target"
    );
}

#[test]
fn resource_schemes_and_uuid_references_require_external_access() {
    let bytes = include_bytes!("../../parse/tests/data/er02_resource_access_witness.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode resource access witness without transport access");

    for note in [
        "external reference #10 -> https://example.invalid/part.p21#shape",
        "external reference #11 -> file:///definitely/not/a/real/part.p21#shape",
        "external reference #12 -> urn:uuid:123e4567-e89b-12d3-a456-426614174000#shape",
        "external reference #13 -> #123e4567-e89b-12d3-a456-426614174000",
    ] {
        assert!(
            result.report().notes.contains(&note.into()),
            "missing {note}"
        );
    }

    let summary = StepCodec::default()
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("inspect resource access witness");
    let references = summary
        .entries
        .iter()
        .find(|entry| entry.name == "REFERENCE")
        .expect("reference inventory");
    assert_eq!(
        references.attributes["external_uris"],
        "https://example.invalid/part.p21#shape,file:///definitely/not/a/real/part.p21#shape,urn:uuid:123e4567-e89b-12d3-a456-426614174000#shape,#123e4567-e89b-12d3-a456-426614174000"
    );
}

#[test]
fn decode_does_not_invoke_the_external_resource_resolver() {
    let bytes = include_bytes!("../../parse/tests/data/er02_resource_access_witness.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode must not access the URI resources");

    assert_eq!(
        result
            .report()
            .notes
            .iter()
            .filter(|note| note.starts_with("external reference "))
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec![
            "external reference #10 -> https://example.invalid/part.p21#shape",
            "external reference #11 -> file:///definitely/not/a/real/part.p21#shape",
            "external reference #12 -> urn:uuid:123e4567-e89b-12d3-a456-426614174000#shape",
            "external reference #13 -> #123e4567-e89b-12d3-a456-426614174000",
        ]
    );
}

#[derive(Debug, PartialEq)]
struct CallerEntityBinding {
    local_occurrence: u64,
    resource_uri: String,
    anchor_name: String,
    target_id: u64,
}

fn bind_entity_reference(
    root: &crate::parse::Exchange,
    target: &crate::parse::Exchange,
) -> Result<CallerEntityBinding, &'static str> {
    let reference = root
        .references
        .iter()
        .find(|reference| reference.name == "#10")
        .ok_or("missing root reference")?;
    let (resource_uri, anchor_name) = reference
        .uri
        .split_once('#')
        .ok_or("reference has no anchor fragment")?;
    if resource_uri.is_empty() || anchor_name.is_empty() {
        return Err("reference has an incomplete resource identity");
    }
    if crate::reader::schema_identifiers(root) != crate::reader::schema_identifiers(target) {
        return Err("resource schemas differ");
    }
    let root_units = unit_signatures(root);
    let target_units = unit_signatures(target);
    if root_units != target_units {
        return Err("resource units differ");
    }
    let root_context = context_signature(root).ok_or("root has no coordinate context")?;
    let target_context = context_signature(target).ok_or("target has no coordinate context")?;
    if root_context != target_context {
        return Err("resource coordinate contexts differ");
    }
    let anchor = target
        .anchors
        .iter()
        .find(|anchor| anchor.name == anchor_name)
        .ok_or("target anchor is missing")?;
    let crate::parse::Value::Reference(target_id) = anchor.value else {
        return Err("target anchor is not an entity instance");
    };
    let target_record = target
        .records
        .get(&target_id)
        .ok_or("target entity instance is missing")?;
    if !target_record
        .partials
        .iter()
        .any(|partial| partial.name == "CARTESIAN_POINT")
    {
        return Err("target entity is not the admitted AP242 anchor type");
    }
    Ok(CallerEntityBinding {
        local_occurrence: 10,
        resource_uri: resource_uri.to_owned(),
        anchor_name: anchor_name.to_owned(),
        target_id,
    })
}

fn unit_signatures(exchange: &crate::parse::Exchange) -> Vec<Vec<crate::parse::PartialRecord>> {
    exchange
        .records
        .values()
        .filter(|record| {
            record
                .partials
                .iter()
                .any(|partial| partial.name == "LENGTH_UNIT" || partial.name == "PLANE_ANGLE_UNIT")
        })
        .map(|record| record.partials.clone())
        .collect()
}

fn context_signature(
    exchange: &crate::parse::Exchange,
) -> Option<Vec<crate::parse::PartialRecord>> {
    exchange
        .records
        .values()
        .find(|record| {
            record
                .partials
                .iter()
                .any(|partial| partial.name == "GEOMETRIC_REPRESENTATION_CONTEXT")
        })
        .map(|record| record.partials.clone())
}

#[test]
fn caller_composition_binds_annex_j_style_target_after_resource_checks() {
    let root_bytes = include_bytes!("tests/data/er05_distributed_root.p21");
    let target_bytes = include_bytes!("tests/data/er05_distributed_subsidiary.p21");
    let (root, root_diagnostics) = crate::parse::parse(root_bytes).expect("parse distributed root");
    let (target, target_diagnostics) =
        crate::parse::parse(target_bytes).expect("parse distributed subsidiary");
    assert!(root_diagnostics.is_empty());
    assert!(target_diagnostics.is_empty());

    let binding = bind_entity_reference(&root, &target).expect("admit external entity binding");
    assert_eq!(
        binding,
        CallerEntityBinding {
            local_occurrence: 10,
            resource_uri: "https://example.invalid/er05/subsidiary.p21".into(),
            anchor_name: "remote_point".into(),
            target_id: 12,
        }
    );
    assert_eq!(
        root.records[&5].partials[0].parameters,
        vec![crate::parse::Value::Reference(10)]
    );
    assert_eq!(target.anchors[0].value, crate::parse::Value::Reference(12));
    assert_eq!(
        target.records[&11].partials[0].parameters,
        vec![crate::parse::Value::Reference(20)]
    );

    let root_result = StepCodec::default()
        .decode(&mut Cursor::new(root_bytes), &DecodeOptions::default())
        .expect("decode root without importing target");
    let target_result = StepCodec::default()
        .decode(&mut Cursor::new(target_bytes), &DecodeOptions::default())
        .expect("decode target independently");
    assert_eq!(root_result.ir().units, target_result.ir().units);
    assert!(root_result
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.id.as_str() == "step:data:point#4"));
    assert!(target_result
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.id.as_str() == "step:data:point#12"));
    assert!(!root_result
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.id.as_str() == "step:data:point#12"));
    assert_eq!(
        root_result.ir().source.as_ref().unwrap().attributes["entity_instances"],
        "6"
    );
    assert_eq!(
        target_result.ir().source.as_ref().unwrap().attributes["entity_instances"],
        "6"
    );

    let mismatched_units = String::from_utf8(target_bytes.to_vec())
        .unwrap()
        .replace("SI_UNIT(.MILLI.,.METRE.)", "SI_UNIT(.CENTI.,.METRE.)");
    let (mismatched_units, _) =
        crate::parse::parse(mismatched_units.as_bytes()).expect("parse mismatched-unit target");
    assert_eq!(
        bind_entity_reference(&root, &mismatched_units),
        Err("resource units differ")
    );

    let mismatched_context = String::from_utf8(target_bytes.to_vec()).unwrap().replace(
        "REPRESENTATION_CONTEXT('model','3D')",
        "REPRESENTATION_CONTEXT('other','3D')",
    );
    let (mismatched_context, _) = crate::parse::parse(mismatched_context.as_bytes())
        .expect("parse mismatched-context target");
    assert_eq!(
        bind_entity_reference(&root, &mismatched_context),
        Err("resource coordinate contexts differ")
    );
}

#[derive(Debug, Clone, PartialEq)]
struct Part26CompositionSource {
    resource_uri: String,
    schema_id: String,
    mapping_edition: &'static str,
    population: String,
    dataset_name: String,
    entity_name: String,
    row_index: usize,
    entity_instance_identifier: i32,
    coordinates: [f64; 3],
    unit_signature: String,
    context_signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Part26Part21Relation {
    part26_resource_uri: String,
    part21_resource_uri: String,
    population: String,
    entity_name: String,
    row_index: usize,
    entity_instance_identifier: i32,
    part21_anchor: String,
}

#[derive(Debug, Clone, PartialEq)]
struct Part26PointBinding {
    part26_resource_uri: String,
    part26_population: String,
    part26_entity_name: String,
    part26_row_index: usize,
    part26_entity_instance_identifier: i32,
    part21_resource_uri: String,
    part21_anchor: String,
    part21_target_id: u64,
    neutral_coordinates: Option<[f64; 3]>,
}

#[derive(Debug, PartialEq)]
enum Part26Composition {
    Bound(Part26PointBinding),
    Unbound(&'static str),
    Conflict {
        binding: Part26PointBinding,
        part26_coordinates: [f64; 3],
        part21_coordinates: [f64; 3],
    },
}

fn decode_part26_composition_source() -> Part26CompositionSource {
    let encoded = include_bytes!("tests/data/ce06_part26_ap242_population.h5.b64")
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();
    let bytes = STANDARD
        .decode(encoded)
        .expect("CE-06 Part 26 HDF5 witness");
    let file = hdf5_reader::Hdf5File::from_vec(bytes).expect("valid CE-06 HDF5 witness");
    let schema_id = "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF";
    let schema = file
        .group(&format!("/{schema_id}_encoding"))
        .expect("Part 26 AP242 schema group");
    assert_eq!(
        schema
            .attribute("iso_10303_26_schema")
            .expect("Part 26 schema attribute")
            .read_string()
            .expect("Part 26 schema identifier"),
        schema_id
    );
    let population_name = "AP242_population";
    let population = file
        .group(&format!("/{population_name}"))
        .expect("Part 26 population group");
    assert_eq!(
        population
            .attribute("iso_10303-26_data")
            .expect("Part 26 population schema attribute")
            .read_string()
            .expect("Part 26 population schema identifier"),
        schema_id
    );
    let dataset_name = population
        .attribute("iso_10303_26_data_set_names")
        .expect("Part 26 dataset-name table")
        .read_strings()
        .expect("Part 26 dataset names");
    assert_eq!(dataset_name, ["CARTESIAN_POINT"]);
    let context_signature = population
        .attribute("iso_10303-26_context")
        .expect("Part 26 context attribute")
        .read_string()
        .expect("Part 26 context");

    let entity_name = "CARTESIAN_POINT";
    let dataset = file
        .dataset(&format!(
            "/{population_name}/{entity_name}_objects/{entity_name}_instances"
        ))
        .expect("Part 26 entity dataset");
    assert_eq!(dataset.shape(), [1]);
    let row_bytes = dataset.read_raw_bytes().expect("Part 26 entity row");
    assert_eq!(row_bytes.len(), 64);
    let coordinates = [
        View::f64_le_at(&row_bytes, 40).expect("Part 26 x coordinate"),
        View::f64_le_at(&row_bytes, 48).expect("Part 26 y coordinate"),
        View::f64_le_at(&row_bytes, 56).expect("Part 26 z coordinate"),
    ];
    Part26CompositionSource {
        resource_uri: "https://example.invalid/er05/subsidiary.h5".into(),
        schema_id: schema_id.into(),
        mapping_edition: "ISO/TS 10303-26:2011",
        population: population_name.into(),
        dataset_name: dataset_name[0].clone(),
        entity_name: entity_name.into(),
        row_index: 0,
        entity_instance_identifier: View::i32_le_at(&row_bytes, 4)
            .expect("Part 26 entity instance identifier"),
        coordinates,
        unit_signature: "MILLI:METRE,RADIAN".into(),
        context_signature,
    }
}

fn part21_unit_signature(exchange: &crate::parse::Exchange) -> String {
    let mut units = exchange
        .records
        .values()
        .flat_map(|record| record.partials.iter())
        .filter(|partial| partial.name == "SI_UNIT")
        .filter_map(|partial| {
            let prefix = match partial.parameters.first()? {
                crate::parse::Value::Enumeration(value) => Some(value.as_str()),
                crate::parse::Value::Omitted => None,
                _ => return None,
            };
            let unit = match partial.parameters.get(1)? {
                crate::parse::Value::Enumeration(value) => value.as_str(),
                _ => return None,
            };
            Some(prefix.map_or_else(|| unit.to_owned(), |prefix| format!("{prefix}:{unit}")))
        })
        .collect::<Vec<_>>();
    units.sort_unstable();
    units.join(",")
}

fn part21_context_signature(exchange: &crate::parse::Exchange) -> Option<String> {
    let context = exchange.records.values().find(|record| {
        record
            .partials
            .iter()
            .any(|partial| partial.name == "GEOMETRIC_REPRESENTATION_CONTEXT")
    })?;
    let representation_context = context
        .partials
        .iter()
        .find(|partial| partial.name == "REPRESENTATION_CONTEXT")?;
    let label = match representation_context.parameters.first()? {
        crate::parse::Value::String(value) => String::from_utf8(value.clone()).ok()?,
        _ => return None,
    };
    let dimension = match representation_context.parameters.get(1)? {
        crate::parse::Value::String(value) => String::from_utf8(value.clone()).ok()?,
        _ => return None,
    };
    Some(format!("{label},{dimension}"))
}

fn part21_point_coordinates(exchange: &crate::parse::Exchange, id: u64) -> Option<[f64; 3]> {
    let record = exchange.records.get(&id)?;
    let point = record
        .partials
        .iter()
        .find(|partial| partial.name == "CARTESIAN_POINT")?;
    let crate::parse::Value::List(values) = point.parameters.get(1)? else {
        return None;
    };
    if values.len() != 3 {
        return None;
    }
    let coordinates = values
        .iter()
        .map(|value| match value {
            crate::parse::Value::Integer(value) => Some(*value as f64),
            crate::parse::Value::Real(value) => Some(*value),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    coordinates.try_into().ok()
}

fn compose_part26_point(
    source: &Part26CompositionSource,
    target_resource_uri: &str,
    target: &crate::parse::Exchange,
    relation: Option<&Part26Part21Relation>,
) -> Part26Composition {
    let Some(relation) = relation else {
        return Part26Composition::Unbound("missing explicit resource binding");
    };
    if relation.part26_resource_uri != source.resource_uri
        || relation.part21_resource_uri != target_resource_uri
    {
        return Part26Composition::Unbound("resource identities differ");
    }
    if relation.population != source.population
        || relation.entity_name != source.entity_name
        || relation.row_index != source.row_index
        || relation.entity_instance_identifier != source.entity_instance_identifier
        || source.dataset_name != source.entity_name
    {
        return Part26Composition::Unbound("Part 26 identity map does not resolve");
    }
    if source.mapping_edition != "ISO/TS 10303-26:2011" {
        return Part26Composition::Unbound("Part 26 mapping edition is not selected");
    }
    if source.schema_id != crate::reader::schema_identifiers(target).join(",") {
        return Part26Composition::Unbound("resource schemas differ");
    }
    if source.unit_signature != part21_unit_signature(target) {
        return Part26Composition::Unbound("resource units differ");
    }
    if source.context_signature != part21_context_signature(target).unwrap_or_default() {
        return Part26Composition::Unbound("resource coordinate contexts differ");
    }
    let anchor = target
        .anchors
        .iter()
        .find(|anchor| anchor.name == relation.part21_anchor);
    let Some(anchor) = anchor else {
        return Part26Composition::Unbound("target anchor is missing");
    };
    let crate::parse::Value::Reference(target_id) = anchor.value else {
        return Part26Composition::Unbound("target anchor is not an entity instance");
    };
    let Some(target_record) = target.records.get(&target_id) else {
        return Part26Composition::Unbound("target entity instance is missing");
    };
    if !target_record
        .partials
        .iter()
        .any(|partial| partial.name == source.entity_name)
    {
        return Part26Composition::Unbound("target entity type differs");
    }
    let Some(part21_coordinates) = part21_point_coordinates(target, target_id) else {
        return Part26Composition::Unbound("target entity value is not mapped");
    };
    let binding = Part26PointBinding {
        part26_resource_uri: source.resource_uri.clone(),
        part26_population: source.population.clone(),
        part26_entity_name: source.entity_name.clone(),
        part26_row_index: source.row_index,
        part26_entity_instance_identifier: source.entity_instance_identifier,
        part21_resource_uri: target_resource_uri.to_owned(),
        part21_anchor: relation.part21_anchor.clone(),
        part21_target_id: target_id,
        neutral_coordinates: None,
    };
    if source.coordinates != part21_coordinates {
        return Part26Composition::Conflict {
            binding,
            part26_coordinates: source.coordinates,
            part21_coordinates,
        };
    }
    Part26Composition::Bound(Part26PointBinding {
        neutral_coordinates: Some(source.coordinates),
        ..binding
    })
}

#[test]
fn caller_composition_binds_part26_row_to_part21_anchor_only_with_explicit_policy() {
    let part26 = decode_part26_composition_source();
    let target_bytes = include_bytes!("tests/data/er05_distributed_subsidiary.p21");
    let target_resource_uri = "https://example.invalid/er05/subsidiary.p21";
    let (target, diagnostics) = crate::parse::parse(target_bytes).expect("parse Part 21 target");
    assert!(diagnostics.is_empty());
    let relation = Part26Part21Relation {
        part26_resource_uri: part26.resource_uri.clone(),
        part21_resource_uri: target_resource_uri.into(),
        population: part26.population.clone(),
        entity_name: part26.entity_name.clone(),
        row_index: part26.row_index,
        entity_instance_identifier: part26.entity_instance_identifier,
        part21_anchor: "remote_point".into(),
    };

    let Part26Composition::Bound(binding) =
        compose_part26_point(&part26, target_resource_uri, &target, Some(&relation))
    else {
        panic!("compatible Part 26 and Part 21 resources must bind");
    };
    assert_eq!(binding.part26_resource_uri, part26.resource_uri);
    assert_eq!(binding.part26_population, part26.population);
    assert_eq!(binding.part26_entity_name, part26.entity_name);
    assert_eq!(binding.part26_row_index, part26.row_index);
    assert_eq!(binding.part21_resource_uri, target_resource_uri);
    assert_eq!(binding.part21_anchor, "remote_point");
    assert_eq!(binding.part26_entity_instance_identifier, 12);
    assert_eq!(binding.part21_target_id, 12);
    assert_eq!(binding.neutral_coordinates, Some([25.4, 0.0, 0.0]));
    assert_eq!(part21_unit_signature(&target), "MILLI:METRE,RADIAN");
    assert_eq!(
        part21_context_signature(&target).as_deref(),
        Some("model,3D")
    );

    assert_eq!(
        compose_part26_point(&part26, target_resource_uri, &target, None),
        Part26Composition::Unbound("missing explicit resource binding")
    );
    let mut wrong_edition = part26.clone();
    wrong_edition.mapping_edition = "ISO/TS 10303-26:2015";
    assert_eq!(
        compose_part26_point(
            &wrong_edition,
            target_resource_uri,
            &target,
            Some(&relation)
        ),
        Part26Composition::Unbound("Part 26 mapping edition is not selected")
    );
    let mut wrong_resource = relation.clone();
    wrong_resource.part26_resource_uri = target_resource_uri.into();
    assert_eq!(
        compose_part26_point(&part26, target_resource_uri, &target, Some(&wrong_resource)),
        Part26Composition::Unbound("resource identities differ")
    );
    let mut wrong_row = relation.clone();
    wrong_row.row_index = 1;
    assert_eq!(
        compose_part26_point(&part26, target_resource_uri, &target, Some(&wrong_row)),
        Part26Composition::Unbound("Part 26 identity map does not resolve")
    );
    let mut wrong_anchor = relation.clone();
    wrong_anchor.part21_anchor = "local_point_with_same_numeric_id".into();
    assert_eq!(
        compose_part26_point(&part26, target_resource_uri, &target, Some(&wrong_anchor)),
        Part26Composition::Unbound("target anchor is missing")
    );

    let mismatched_units = String::from_utf8(target_bytes.to_vec())
        .expect("Part 21 target text")
        .replace("SI_UNIT(.MILLI.,.METRE.)", "SI_UNIT(.CENTI.,.METRE.)");
    let (mismatched_units, _) =
        crate::parse::parse(mismatched_units.as_bytes()).expect("parse mismatched units");
    assert_eq!(
        compose_part26_point(
            &part26,
            target_resource_uri,
            &mismatched_units,
            Some(&relation)
        ),
        Part26Composition::Unbound("resource units differ")
    );

    let mismatched_context = String::from_utf8(target_bytes.to_vec())
        .expect("Part 21 target text")
        .replace(
            "REPRESENTATION_CONTEXT('model','3D')",
            "REPRESENTATION_CONTEXT('other','3D')",
        );
    let (mismatched_context, _) =
        crate::parse::parse(mismatched_context.as_bytes()).expect("parse mismatched context");
    assert_eq!(
        compose_part26_point(
            &part26,
            target_resource_uri,
            &mismatched_context,
            Some(&relation)
        ),
        Part26Composition::Unbound("resource coordinate contexts differ")
    );

    let mismatched_schema = String::from_utf8(target_bytes.to_vec())
        .expect("Part 21 target text")
        .replace("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF", "AP214");
    let (mismatched_schema, _) =
        crate::parse::parse(mismatched_schema.as_bytes()).expect("parse mismatched schema");
    assert_eq!(
        compose_part26_point(
            &part26,
            target_resource_uri,
            &mismatched_schema,
            Some(&relation)
        ),
        Part26Composition::Unbound("resource schemas differ")
    );

    let conflicting_coordinates = String::from_utf8(target_bytes.to_vec())
        .expect("Part 21 target text")
        .replace("(25.4,0.,0.)", "(25.5,0.,0.)");
    let (conflicting_coordinates, _) = crate::parse::parse(conflicting_coordinates.as_bytes())
        .expect("parse conflicting coordinates");
    let Part26Composition::Conflict {
        binding: conflict_binding,
        part26_coordinates,
        part21_coordinates,
    } = compose_part26_point(
        &part26,
        target_resource_uri,
        &conflicting_coordinates,
        Some(&relation),
    )
    else {
        panic!("different mapped values must be a retained composition conflict");
    };
    assert_eq!(part26_coordinates, [25.4, 0.0, 0.0]);
    assert_eq!(part21_coordinates, [25.5, 0.0, 0.0]);
    assert_eq!(conflict_binding.neutral_coordinates, None);
    assert_eq!(conflict_binding.part26_entity_instance_identifier, 12);
    assert_eq!(conflict_binding.part21_target_id, 12);
}

#[test]
fn resource_metadata_and_uri_spellings_do_not_create_cache_identity() {
    let bytes = include_bytes!("tests/data/er04_cache_identity.p21");
    let (exchange, diagnostics) = crate::parse::parse(bytes).expect("parse cache witness");
    assert!(diagnostics.is_empty());
    let population = exchange
        .header
        .iter()
        .find(|record| record.name == "SCHEMA_POPULATION")
        .expect("schema population header");
    let crate::parse::Value::List(entries) = &population.parameters[0] else {
        panic!("schema population entries");
    };
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[0],
        crate::parse::Value::List(vec![
            crate::parse::Value::String(b"https://example.invalid/model.p21".to_vec()),
            crate::parse::Value::String(b"2026-08-16T00:00:00".to_vec()),
            crate::parse::Value::Omitted,
        ])
    );
    assert_eq!(
        entries[1],
        crate::parse::Value::List(vec![
            crate::parse::Value::String(b"https://example.invalid/model.p21".to_vec()),
            crate::parse::Value::String(b"2026-08-17T00:00:00".to_vec()),
            crate::parse::Value::Omitted,
        ])
    );

    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode cache witness without resource access");
    assert!(result
        .report()
        .notes
        .contains(&"external reference #10 -> https://example.invalid/model.p21#shape".into()));
    assert!(result
        .report()
        .notes
        .contains(&"external reference #11 -> https://example.invalid/./model.p21#shape".into()));
    let source = result.ir().source.as_ref().expect("STEP source metadata");
    assert!(!source.attributes.keys().any(|key| key.contains("cache")));

    let summary = StepCodec::default()
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("inspect cache witness");
    let references = summary
        .entries
        .iter()
        .find(|entry| entry.name == "REFERENCE")
        .expect("reference inventory");
    assert_eq!(
        references.attributes["external_uris"],
        "https://example.invalid/model.p21#shape,https://example.invalid/./model.p21#shape"
    );
}

#[test]
fn signed_resource_digest_and_timestamp_are_retained_without_cache_identity() {
    let bytes = include_bytes!("tests/data/er04_cache_identity_signed.p21");
    let signed_resource = include_bytes!("../../signature/tests/data/sg04_openssl_detached.p21");
    let (exchange, diagnostics) = crate::parse::parse(bytes).expect("parse signed cache witness");
    assert!(diagnostics.is_empty());
    let signed_exchange = crate::parse::parse(signed_resource)
        .expect("parse signed resource")
        .0;
    assert_eq!(signed_exchange.signature_sections.len(), 1);

    let population = exchange
        .header
        .iter()
        .find(|record| record.name == "SCHEMA_POPULATION")
        .expect("signed schema population header");
    let crate::parse::Value::List(entries) = &population.parameters[0] else {
        panic!("signed schema population entries");
    };
    assert_eq!(entries.len(), 2);
    for (entry, (address, timestamp)) in entries.iter().zip([
        ("signature/sg04_openssl_detached.p21", "2026-08-16T00:00:00"),
        (
            "signature/./sg04_openssl_detached.p21",
            "2026-08-17T00:00:00",
        ),
    ]) {
        assert_eq!(
            entry,
            &crate::parse::Value::List(vec![
                crate::parse::Value::String(address.as_bytes().to_vec()),
                crate::parse::Value::String(timestamp.as_bytes().to_vec()),
                crate::parse::Value::String(
                    b"PVXS8diN1zTOTu9AEL6T+aJH7u5ckF7wVCROqLXlIDA=".to_vec(),
                ),
            ])
        );
    }

    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode signed cache witness without resource access");
    let source = result.ir().source.as_ref().expect("STEP source metadata");
    assert!(!source.attributes.keys().any(|key| key.contains("cache")));
    assert!(result
        .report()
        .notes
        .iter()
        .all(|note| !note.starts_with("external resource")));
}

#[test]
fn complex_document_dependency_records_use_inherited_fields() {
    let result = decode_inline(
        "#1=DOCUMENT_TYPE('digital');
#2=(DOCUMENT('SPEC-42','Interface control drawing','',#1) DOCUMENT_FILE());
#3=(APPLIED_DOCUMENT_REFERENCE() DOCUMENT_REFERENCE(#2,'supplier vault'));",
    );

    assert!(result.report().notes.contains(
        &"external document SPEC-42 (Interface control drawing) from supplier vault".into()
    ));
    assert!(!result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| {
            record.id.0 == "step:data:document#2"
                || record.id.0 == "step:data:document_file#2"
                || record.id.0 == "step:data:applied_document_reference#3"
                || record.id.0 == "step:data:document_reference#3"
        }));
}
