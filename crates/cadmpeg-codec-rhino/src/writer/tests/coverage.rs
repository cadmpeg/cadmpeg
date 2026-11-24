// SPDX-License-Identifier: Apache-2.0
//! Model arenas the 3DM write does not represent are reported, not dropped.

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::geometry::FiniteVector3;
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

        tessellations: Vec::new(),
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
            "the rhino write does not represent model arena `product_definitions`: 2 record(s) omitted",
            "the rhino write does not represent model arena `occurrences`: 2 record(s) omitted",
        ]
    );
}
