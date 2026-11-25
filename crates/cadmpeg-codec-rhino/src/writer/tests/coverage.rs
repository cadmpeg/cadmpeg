// SPDX-License-Identifier: Apache-2.0
//! Model arenas the 3DM write does not represent are reported, not dropped.

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::geometry::FiniteVector3;
use cadmpeg_ir::features::parameters::DesignParameter;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::products::{
    ExternalDocument, Occurrence, ProductDefinition, ProductDefinitionKind,
};
use cadmpeg_ir::transform::Transform;

use crate::{RhinoArchiveVersion, RhinoCodec};

/// One point body, one part definition holding it, and a root definition
/// placing that part at x=100 and x=200.
fn placed_point_product() -> CadIr {
    let mut ir = CadIr::empty();
    let body: cadmpeg_ir::ids::BodyId = "cadir:model:body#pin".try_into().expect("identity");
    let region: cadmpeg_ir::ids::RegionId = "cadir:model:region#pin".try_into().expect("identity");
    let shell: cadmpeg_ir::ids::ShellId = "cadir:model:shell#pin".try_into().expect("identity");
    let vertex = cadmpeg_ir::ids::VertexId::mint("cadir:model:vertex#pin").expect("identity");
    let point = cadmpeg_ir::ids::PointId::mint("cadir:model:point#pin").expect("identity");
    ir.geometry.bodies.push(cadmpeg_ir::topology::Body {
        id: body.clone(),
        kind: cadmpeg_ir::topology::BodyKind::General,
        transform: None,
        name: None,
        color: None,
        visible: None,
        native_ref: None,
        unstated: None,
    });
    ir.geometry.regions.push(cadmpeg_ir::topology::Region::new(
        region.clone(),
        body.clone(),
        vec![shell.clone()],
    ));
    ir.geometry
        .shells
        .push(cadmpeg_ir::topology::Shell::with_members(
            shell,
            region,
            Vec::new(),
            vec![vertex.clone()],
        ));
    ir.geometry.vertices.push(cadmpeg_ir::topology::Vertex {
        id: vertex,
        point: point.clone(),
        tolerance: None,
        native_ref: None,
    });
    ir.geometry.points.push(cadmpeg_ir::topology::Point::new(
        point,
        cadmpeg_ir::features::geometry::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
            .expect("finite position"),
        None,
    ));
    let part: cadmpeg_ir::ids::ProductDefinitionId =
        "cadir:model:product#pin".try_into().expect("identity");
    ir.model.product_definitions.push(ProductDefinition {
        sketches: Vec::default(),

        id: part.clone(),
        kind: Some(ProductDefinitionKind::Part),
        source_name: Some("Pin".into()),
        label: None,
        description: None,
        part_number: None,
        bom_properties: std::collections::BTreeMap::new(),
        bodies: vec![body],
        native_ref: None,
        generated_from: None,
        storage: cadmpeg_ir::products::Storage::Embedded,
        occurrences: Vec::new(),
        instance_bodies: Vec::new(),
        representations: Vec::new(),
        units: None,
        general_tolerance: None,
        active: None,
    });
    let mut root = ProductDefinition::root(
        "cadir:model:product#root".try_into().expect("identity"),
        ExternalDocument::default(),
    );
    for (ordinal, x) in [(0, 100.0), (1, 200.0)] {
        root.occurrences.push(
            cadmpeg_ir::ids::OccurrenceId::mint(format!("cadir:model:occurrence#pin.{ordinal}"))
                .expect("identity"),
        );
        ir.model.occurrences.push(Occurrence {
            id: cadmpeg_ir::ids::OccurrenceId::mint(format!(
                "cadir:model:occurrence#pin.{ordinal}"
            ))
            .expect("identity"),
            prototype: part.clone(),
            transform: Some(Transform::identity().with_translation(
                FiniteVector3::new(Vector3::new(x, 0.0, 0.0)).expect("finite offset"),
            )),
            suppressed: None,
            visible: None,
            representation: None,
            overrides: Vec::new(),
            name: None,
            native_ref: None,
        });
    }
    ir.model.product_definitions.push(root);
    ir.finalize();
    ir
}

#[test]
fn unrepresented_product_structure_is_an_export_loss() {
    let ir = placed_point_product();
    let validation =
        cadmpeg_ir::validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
    assert!(validation.is_ok(), "{:?}", validation.findings);

    let plan = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("the point body is writable");
    let omitted = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.namespace() == "export")
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        omitted,
        [
            "the rhino write does not represent model arena `product_definitions`: 2 record(s) \
             omitted: cadir:model:product#pin, cadir:model:product#root",
            "the rhino write does not represent model arena `occurrences`: 2 record(s) omitted: \
             cadir:model:occurrence#pin.0, cadir:model:occurrence#pin.1",
        ]
    );
}

/// Adds one feature, one parameter and one attribute on the first body: the
/// design content a part from a history-based system holds beside its shape.
fn with_design_content(ir: &mut CadIr) {
    ir.model.features.push(
        serde_json::from_value(serde_json::json!({
            "id": "cadir:model:feature#boss", "ordinal": 0, "name": "Boss", "suppressed": false,
            "definition": {"kind": "operation", "value": {"kind": "stored_geometry", "value": {}}},
            "outputs": [], "dependencies": [], "source_properties": {},
            "source_content": []
        }))
        .expect("feature"),
    );
    ir.model.parameters.push(DesignParameter {
        product_definition: None,
        id: ParameterId::mint("cadir:model:parameter#height").expect("identity"),
        owner: None,
        ordinal: 0,
        name: "Height".into(),
        expression: "12mm".into(),
        display: None,
        value: None,
        dependencies: DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.attributes.push(SourceAttribute {
        id: "cadir:model:attribute#finish".try_into().expect("identity"),
        target: AttributeTarget::Body(ir.geometry.bodies[0].id.clone()).into(),
        name: "Finish".into(),
        values: vec![Some(AttributeValue::String("anodized".into()))],
        native_ref: None,
    });
}

/// The archive and the messages of its `export` losses.
fn write_v8(ir: &CadIr) -> (Vec<u8>, Vec<String>) {
    let plan = RhinoCodec
        .plan(
            EncodeInput::new(ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("the document is writable");
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).expect("write");
    let messages = report
        .losses
        .iter()
        .filter(|loss| loss.code.namespace() == "export")
        .map(|loss| loss.message.clone())
        .collect();
    (bytes, messages)
}

#[test]
fn a_body_with_design_content_is_written_and_the_content_reported() {
    let mut ir = super::planar_tetrahedron();
    with_design_content(&mut ir);

    let (bytes, omitted) = write_v8(&ir);
    assert_eq!(
        omitted,
        [
            "the rhino write does not represent model arena `features`: 1 record(s) omitted: \
             cadir:model:feature#boss",
            "the rhino write does not represent model arena `parameters`: 1 record(s) omitted: \
             cadir:model:parameter#height",
            "the rhino write does not represent model arena `attributes`: 1 record(s) omitted: \
             cadir:model:attribute#finish",
        ]
    );
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the written archive decodes");
    assert_eq!(decoded.ir().geometry.bodies.len(), 1);
    assert_eq!(decoded.ir().geometry.faces.len(), 4);
}

#[test]
fn design_content_alone_writes_an_empty_archive_and_says_so() {
    let mut ir = super::planar_tetrahedron();
    with_design_content(&mut ir);
    ir.model.attributes.clear();
    ir.geometry = cadmpeg_ir::document::Topology::default();

    let (_, omitted) = write_v8(&ir);
    assert_eq!(
        omitted,
        [
            "the written 3DM archive holds no object: the document states no geometry or mesh \
             this writer encodes",
            "the rhino write does not represent model arena `features`: 1 record(s) omitted: \
             cadir:model:feature#boss",
            "the rhino write does not represent model arena `parameters`: 1 record(s) omitted: \
             cadir:model:parameter#height",
        ]
    );
}

#[test]
fn the_display_mesh_of_a_brep_body_is_withheld_and_named() {
    let mut ir = super::planar_tetrahedron();
    let mut mesh = cadmpeg_ir::tessellation::Tessellation::new(
        cadmpeg_ir::tessellation::TessellationId::mint("cadir:model:tessellation#display")
            .expect("identity"),
        cadmpeg_ir::tessellation::TessellationMesh::List {
            vertices: vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 2]],
        },
        Vec::new(),
    )
    .expect("mesh");
    mesh.of = cadmpeg_ir::tessellation::Meshed::Body {
        body: ir.geometry.bodies[0].id.clone(),
        faces: Vec::new(),
    };
    ir.model.tessellations.push(mesh);

    let plan = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("the body is written without its display mesh");
    let withheld = crate::loss::RhinoLossCode::WriterBodyMeshWithheld.kind();
    assert!(
        plan.report().losses.iter().any(|loss| loss.code == withheld
            && loss.message.ends_with("cadir:model:tessellation#display")),
        "{:?}",
        plan.report().losses
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the written archive decodes");
    assert_eq!(decoded.ir().geometry.faces.len(), 4);
    assert!(decoded.ir().model.tessellations.is_empty());
}

#[test]
fn a_carrier_from_another_format_is_written_without_its_source_identity() {
    let mut ir = super::planar_tetrahedron();
    ir.geometry.surfaces[0].source_object = Some(cadmpeg_ir::SourceObjectAssociation {
        format: cadmpeg_ir::codec_id!("iges"),
        object_id: cadmpeg_core::nonblank_literal!("D7"),
        geometry_role: Some(cadmpeg_ir::SourceGeometryRole::Independent),
        instance_path: Vec::new(),
    });

    let plan = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("the surface is written without its source identity");
    let omitted = crate::loss::RhinoLossCode::WriterSourceAssociationOmitted.kind();
    assert!(
        plan.report().losses.iter().any(|loss| loss.code == omitted
            && loss
                .message
                .starts_with("1 source-object association(s) were not written")),
        "{:?}",
        plan.report().losses
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the written archive decodes");
    assert_eq!(decoded.ir().geometry.faces.len(), 4);
}
