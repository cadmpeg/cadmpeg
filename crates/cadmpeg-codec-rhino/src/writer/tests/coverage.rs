// SPDX-License-Identifier: Apache-2.0
//! Product structure the 3DM write carries as instance definitions.
//!
//! #235 writes neutral products and occurrences: definitions become instance
//! definitions and root occurrences become instance references, so neither
//! arena is an export omission.

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::transform::Transform;

use crate::{RhinoArchiveVersion, RhinoCodec};

#[test]
fn object_without_a_uuid_keeps_its_identity_on_rewrite() {
    let source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/fixtures/point.3dm"
    ));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode point");
    let identity = decoded.ir().model.bodies[0].id.clone();
    let plan = RhinoCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("plan point");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write point");
    let written = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode written point");
    assert!(written
        .ir()
        .model
        .bodies
        .iter()
        .any(|body| body.id == identity));
}

#[test]
fn omitted_face_display_overrides_name_each_face() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    for face in &mut ir.model.faces {
        face.name = None;
        face.color = None;
    }
    ir.model.faces[0].name = Some("first".into());
    ir.model.faces[1].name = Some("second".into());
    let plan = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("write cube");
    let loss = plan
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.code == crate::loss::RhinoLossCode::WriterAttributesOmitted.kind()
                && loss.message.contains("face-level name or color")
        })
        .expect("face override loss");
    assert!(loss.message.contains(ir.model.faces[0].id.as_str()));
    assert!(loss.message.contains(ir.model.faces[1].id.as_str()));
    assert!(!loss.message.contains(ir.model.faces[2].id.as_str()));
}

/// One point body, one part definition holding it, and two root occurrences
/// placing that part at x=100 and x=200.
fn placed_point_product() -> CadIr {
    let mut ir = CadIr::empty();
    let body: cadmpeg_ir::ids::BodyId = "cadir:model:body#pin".try_into().expect("identity");
    let region: cadmpeg_ir::ids::RegionId = "cadir:model:region#pin".try_into().expect("identity");
    let shell: cadmpeg_ir::ids::ShellId = "cadir:model:shell#pin".try_into().expect("identity");
    let vertex = cadmpeg_ir::ids::VertexId::mint("cadir:model:vertex#pin").expect("identity");
    let point = cadmpeg_ir::ids::PointId::mint("cadir:model:point#pin").expect("identity");
    ir.model.bodies.push(cadmpeg_ir::topology::Body {
        native_ref: None,
        id: body.clone(),
        kind: cadmpeg_ir::topology::BodyKind::General,
        regions: vec![region.clone()],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.regions.push(cadmpeg_ir::topology::Region {
        native_ref: None,
        id: region.clone(),
        body: body.clone(),
        shells: vec![shell.clone()],
    });
    ir.model.shells.push(
        cadmpeg_ir::topology::Shell::new(
            shell,
            region,
            Vec::new(),
            Vec::new(),
            vec![vertex.clone()],
        )
        .expect("free-vertex shell"),
    );
    ir.model.vertices.push(cadmpeg_ir::topology::Vertex {
        native_ref: None,
        id: vertex,
        point: point.clone(),
        tolerance: None,
    });
    ir.model.points.push(cadmpeg_ir::topology::Point::new(
        point,
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
            .expect("finite position"),
        None,
    ));
    let part: cadmpeg_ir::ids::ProductDefinitionId =
        "cadir:model:product#pin".try_into().expect("identity");
    ir.model.product_definitions.push(ProductDefinition {
        sketches: Vec::default(),
        id: part.clone(),
        kind: ProductDefinitionKind::Part,
        source_name: Some("Pin".into()),
        label: None,
        description: None,
        part_number: None,
        bom_properties: std::collections::BTreeMap::new(),
        bodies: vec![body],
        native_ref: None,
    });
    for (ordinal, x) in [(0, 100.0), (1, 200.0)] {
        ir.model.occurrences.push(Occurrence {
            id: cadmpeg_ir::ids::OccurrenceId::mint(format!(
                "cadir:model:occurrence#pin.{ordinal}"
            ))
            .expect("identity"),
            prototype: PrototypeReference::Local {
                definition: part.clone(),
            },
            parent: OccurrenceParent::Root {},
            ordinal,
            transform: Some(Transform::identity().with_translation(
                FiniteVector3::new(Vector3::new(x, 0.0, 0.0)).expect("finite offset"),
            )),
            linked_prototype: None,
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        });
    }
    ir.finalize();
    ir
}

#[test]
fn product_structure_is_written_as_instance_definitions_and_references() {
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
    let report = plan.report();
    for (arena, kind) in [
        (
            "product_definitions",
            cadmpeg_ir::schema::EntityKind::ProductDefinition,
        ),
        ("occurrences", cadmpeg_ir::schema::EntityKind::Occurrence),
    ] {
        assert!(
            report.coverage.disposition(kind).carries(),
            "{arena}: coverage must state that the target carries products"
        );
    }
    assert!(
        !report.losses.iter().any(|loss| {
            loss.code.namespace() == "export"
                && loss.code.local_code() == "arena.omitted"
                && loss.message.contains("`product_definitions`")
        }),
        "product definitions are written, not omitted: {:?}",
        report.losses
    );
    assert!(
        !report.losses.iter().any(|loss| {
            loss.code.namespace() == "export"
                && loss.code.local_code() == "arena.omitted"
                && loss.message.contains("`occurrences`")
        }),
        "occurrences are written, not omitted: {:?}",
        report.losses
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)
        .expect("write the product structure");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("read the written products");
    assert_eq!(
        decoded.ir().model.product_definitions.len(),
        1,
        "the part definition reappears"
    );
    assert_eq!(
        decoded.ir().model.occurrences.len(),
        2,
        "both placements reappear"
    );
}
