// SPDX-License-Identifier: Apache-2.0
//! Each loss code is emitted by the decode or export path that owns it.

use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::tessellation::{ChannelAddressing, TessellationChannel, TessellationTriangleGroup};

use crate::loss::UsdLossCode;
use crate::test_support::{decode, encode, prim_layer};

const TRIANGLE: &str = "int[] faceVertexCounts = [3]\nint[] faceVertexIndices = [0, 1, 2]\npoint3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]\nuniform token subdivisionScheme = \"none\"";
const PATCH: &str = "int uVertexCount = 2\nint vVertexCount = 2\nint uOrder = 2\nint vOrder = 2\ndouble[] uKnots = [0, 0, 1, 1]\ndouble[] vKnots = [0, 0, 1, 1]\npoint3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0), (1, 1, 0)]";

fn emitted(losses: &[LossNote], code: UsdLossCode) -> bool {
    losses.iter().any(|loss| loss.code == code.kind())
}

fn assert_decode_emits(text: &str, code: UsdLossCode) {
    let result = decode(text);
    assert!(
        emitted(&result.report().losses, code),
        "{code:?} not emitted by:\n{text}\n{:#?}",
        result.report().losses
    );
}

fn mesh(body: &str) -> String {
    format!("#usda 1.0\ndef Mesh \"M\"\n{{\n{body}\n}}\n")
}

#[test]
fn composition_losses_are_emitted() {
    assert_decode_emits(
        "#usda 1.0\n(\n    subLayers = [@base.usda@]\n)\ndef \"A\" {\n}\n",
        UsdLossCode::CompositionSublayersNotComposed,
    );
    assert_decode_emits(
        "#usda 1.0\ndef \"A\" (\n    references = </Missing>\n)\n{\n}\n",
        UsdLossCode::CompositionTargetMissing,
    );
    assert_decode_emits(
        "#usda 1.0\ndef \"A\"\n{\n    variantSet \"v\" = {\n        \"x\" {\n        }\n    }\n}\n",
        UsdLossCode::CompositionVariantNotSelected,
    );
    assert_decode_emits(
        "#usda 1.0\nover \"A\" {\n}\n",
        UsdLossCode::CompositionOverUndefined,
    );
    assert_decode_emits(
        "#usda 1.0\ndef \"A\" (\n    references = </B>\n)\n{\n}\ndef \"B\" (\n    references = </A>\n)\n{\n}\n",
        UsdLossCode::CompositionCycle,
    );
    assert_decode_emits(
        "#usda 1.0\nclass \"P\" {\n}\ndef \"I\" (\n    instanceable = true\n    references = </P>\n)\n{\n    def \"Local\" {\n    }\n}\n",
        UsdLossCode::CompositionOverrideNotApplied,
    );
}

#[test]
fn prim_attribute_and_transform_losses_are_emitted() {
    assert_decode_emits(
        "#usda 1.0\ndef Capsule_1 \"C\" {\n    double radiusTop = 1\n}\n",
        UsdLossCode::PrimIntrinsicNotTransferred,
    );
    assert_decode_emits(
        &prim_layer("double3 xformOp:translate.timeSamples = {\n    0: (1, 2, 3),\n}\nuniform token[] xformOpOrder = [\"xformOp:translate\"]"),
        UsdLossCode::AttributeAnimationNotTransferred,
    );
    assert_decode_emits(
        "#usda 1.0\n(\n    customLayerData = {\n        int a = 1\n    }\n)\ndef \"A\" {\n}\n",
        UsdLossCode::MetadataNotTransferred,
    );
    assert_decode_emits(
        &prim_layer("uniform token[] xformOpOrder = [\"xformOp:bogus\"]\nfloat xformOp:bogus = 1"),
        UsdLossCode::XformMalformed,
    );
}

#[test]
fn mesh_losses_are_emitted() {
    assert_decode_emits(
        &mesh("int[] faceVertexCounts = [3, 2]\nint[] faceVertexIndices = [0, 1, 2, 0, 1]\npoint3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]"),
        UsdLossCode::MeshFaceRejected,
    );
    assert_decode_emits(
        &mesh(&format!(
            "{TRIANGLE}\ntexCoord2f[] primvars:st = [(0, 0)] (\n    interpolation = \"vertex\"\n)"
        )),
        UsdLossCode::MeshPrimvarNotTransferred,
    );
    assert_decode_emits(
        &mesh(&format!("{TRIANGLE}\nnormal3f[] normals = [(0, 0, 1)]")),
        UsdLossCode::MeshNormalsNotTransferred,
    );
    assert_decode_emits(
        &mesh(&format!("{TRIANGLE}\ndef GeomSubset \"S\"\n{{\n    uniform token elementType = \"point\"\n    int[] indices = [0]\n}}")),
        UsdLossCode::MeshSubsetNotTransferred,
    );
    assert_decode_emits(
        &mesh(&format!("{TRIANGLE}\ncolor3f[] primvars:displayColor = [(1, 0, 0), (0, 1, 0), (0, 0, 1)] (\n    interpolation = \"vertex\"\n)")),
        UsdLossCode::DisplayColorNotTransferred,
    );
    assert_decode_emits(
        &format!("#usda 1.0\ndef Material \"Mdl\"\n{{\n}}\ndef Mesh \"M\"\n{{\n{TRIANGLE}\nrel material:binding = </Mdl>\n}}\n"),
        UsdLossCode::MaterialNotTransferred,
    );
}

/// A pinned decode of `text`.
fn decode_pinned(
    text: &str,
) -> Result<cadmpeg_ir::codec::DecodeResult, cadmpeg_ir::codec::DecodeFailure> {
    use cadmpeg_ir::codec::Codec as _;

    let mut options = cadmpeg_ir::codec::DecodeOptions::default();
    options.strictness.pinned = true;
    crate::UsdCodec.decode(&mut std::io::Cursor::new(text.as_bytes()), &options)
}

/// Asserts that `record` is given up by exactly one loss of `losses`, for
/// `cause`, with the message `message`.
fn assert_given_up_once(
    losses: &[LossNote],
    record: &str,
    cause: cadmpeg_ir::unknown::part::Cause,
    message: &str,
) {
    let named = losses
        .iter()
        .filter(|loss| loss.message.contains(record))
        .collect::<Vec<_>>();
    let [loss] = named.as_slice() else {
        panic!("one loss names {record}: {losses:#?}");
    };
    assert_eq!(loss.code, cause.kind(), "{loss:#?}");
    assert_eq!(loss.message, message);
}

/// A mesh prim that states no points is undecoded: one `record.undecoded`
/// names its tessellation, and a pinned decode accepts it.
#[test]
fn a_mesh_with_no_points_is_undecoded() {
    let text = mesh("int[] faceVertexCounts = [3]\nint[] faceVertexIndices = [0, 1, 2]");
    let tessellation = crate::ids::tessellation("/M");
    let message = format!(
        "record {tessellation} given up: the mesh states no points, or no face of three \
         points; dropped=[]"
    );

    let result = decode(&text);
    assert!(result.ir().model.tessellations.is_empty());
    assert_given_up_once(
        &result.report().losses,
        tessellation.as_str(),
        cadmpeg_ir::unknown::part::Cause::Undecoded,
        &message,
    );
    let pinned = decode_pinned(&text).expect("a pinned decode accepts an undecoded mesh");
    assert!(pinned.ir().model.tessellations.is_empty());
}

/// A mesh prim whose one face indexes a point that it does not hold is
/// unreadable: one `record.unreadable` names its tessellation, and a pinned
/// decode refuses it.
#[test]
fn a_mesh_whose_faces_index_no_point_is_unreadable() {
    let text = mesh("int[] faceVertexCounts = [3]\nint[] faceVertexIndices = [0, 1, 5]\npoint3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]");
    let tessellation = crate::ids::tessellation("/M");
    let message = format!(
        "record {tessellation} given up: the mesh states no points, or no face of three \
         points; dropped=[]"
    );

    let result = decode(&text);
    assert!(result.ir().model.tessellations.is_empty());
    assert_given_up_once(
        &result.report().losses,
        tessellation.as_str(),
        cadmpeg_ir::unknown::part::Cause::Unreadable,
        &message,
    );
    match decode_pinned(&text) {
        Err(cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection }) => {
            assert_eq!(
                rejection.loss().code,
                cadmpeg_ir::unknown::part::record_unreadable_kind()
            );
            assert_eq!(rejection.loss().message, message);
        }
        Ok(_) => panic!("expected a strict refusal"),
        Err(other) => panic!("expected a strict refusal, got {other:?}"),
    }
}

/// A rejected mesh drops the appearance binding that would target its
/// tessellation with the tessellation's give-up.
#[test]
fn a_rejected_mesh_drops_its_appearance_binding_with_it() {
    let text = "#usda 1.0\ndef Mesh \"M\"\n{\nint[] faceVertexCounts = [3]\nint[] faceVertexIndices = [0, 1, 2]\nrel material:binding = </Grey>\n}\ndef Material \"Grey\"\n{\ntoken outputs:surface.connect = </Grey/S.outputs:surface>\ndef Shader \"S\"\n{\nuniform token info:id = \"UsdPreviewSurface\"\ncolor3f inputs:diffuseColor = (0.5, 0.5, 0.5)\ntoken outputs:surface\n}\n}\n";
    let result = decode(text);
    assert!(result.ir().model.appearance_bindings.is_empty());
    let tessellation = crate::ids::tessellation("/M");
    let message = format!(
        "record {tessellation} given up: the mesh states no points, or no face of three \
         points; dropped=[{}]",
        crate::ids::appearance_binding("/M")
    );
    assert_given_up_once(
        &result.report().losses,
        tessellation.as_str(),
        cadmpeg_ir::unknown::part::Cause::Undecoded,
        &message,
    );
}

/// A `BasisCurves` prim whose cubic basis the reader does not implement
/// states no curve: one `record.undecoded` names the prim's body, and a
/// pinned decode accepts it.
#[test]
fn a_curve_batch_with_an_unsupported_basis_is_undecoded() {
    let text = "#usda 1.0\ndef BasisCurves \"C\"\n{\n    uniform token basis = \"hermite\"\n    int[] curveVertexCounts = [4]\n    point3f[] points = [(0, 0, 0), (1, 0, 0), (2, 0, 0), (3, 0, 0)]\n}\n";
    let body = crate::ids::body("/C");
    let message = format!("record {body} given up: no curve of the batch transfers; dropped=[]");

    let result = decode(text);
    assert!(result.ir().geometry.edges.is_empty());
    let losses = &result.report().losses;
    assert_given_up_once(
        losses,
        body.as_str(),
        cadmpeg_ir::unknown::part::Cause::Undecoded,
        &message,
    );
    assert!(!emitted(losses, UsdLossCode::CurvesRejected), "{losses:#?}");
    let pinned = decode_pinned(text).expect("a pinned decode accepts an undecoded batch");
    assert!(pinned.ir().geometry.edges.is_empty());
}

/// A `NurbsPatch` prim whose vertex counts do not match its points is
/// unreadable: one `record.unreadable` names the prim's body, and a pinned
/// decode refuses it.
#[test]
fn a_patch_whose_counts_disagree_is_unreadable() {
    let text = format!(
        "#usda 1.0\ndef NurbsPatch \"P\"\n{{\n{}\n}}\n",
        PATCH.replace("int uVertexCount = 2", "int uVertexCount = 3")
    );
    let body = crate::ids::body("/P");
    let message = format!(
        "record {body} given up: the NURBS patch states no counts, orders, knots, and points \
         that agree; dropped=[]"
    );

    let result = decode(&text);
    assert!(result.ir().geometry.faces.is_empty());
    assert_given_up_once(
        &result.report().losses,
        body.as_str(),
        cadmpeg_ir::unknown::part::Cause::Unreadable,
        &message,
    );
    match decode_pinned(&text) {
        Err(cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection }) => {
            assert_eq!(rejection.loss().message, message);
        }
        Ok(_) => panic!("expected a strict refusal"),
        Err(other) => panic!("expected a strict refusal, got {other:?}"),
    }
}

#[test]
fn curve_and_patch_losses_are_emitted() {
    assert_decode_emits(
        "#usda 1.0\ndef BasisCurves \"C\"\n{\n    uniform token type = \"linear\"\n    int[] curveVertexCounts = [2, 1]\n    point3f[] points = [(0, 0, 0), (1, 0, 0), (2, 0, 0)]\n}\n",
        UsdLossCode::CurvesNotTransferred,
    );
    assert_decode_emits(
        "#usda 1.0\ndef BasisCurves \"C\"\n{\n    int[] curveVertexCounts = [4, 3]\n    point3f[] points = [(0, 0, 0), (1, 0, 0), (2, 0, 0), (3, 0, 0), (0, 1, 0), (1, 1, 0), (2, 1, 0)]\n}\n",
        UsdLossCode::CurvesRejected,
    );
    assert_decode_emits(
        &format!("#usda 1.0\ndef NurbsPatch \"P\"\n{{\n{PATCH}\ndouble2 uRange = (0, 2)\n}}\n"),
        UsdLossCode::NurbsRangeNotTransferred,
    );
}

#[test]
fn export_losses_are_emitted() {
    let curves = decode("#usda 1.0\ndef BasisCurves \"C\"\n{\n    uniform token type = \"linear\"\n    int[] curveVertexCounts = [2]\n    point3f[] points = [(0, 0, 0), (1, 0, 0)]\n}\n");
    let (mut ir, _, _) = curves.into_parts();
    let circle = cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        1.0,
    )
    .unwrap();
    ir.geometry.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle));
    let (_, report) = encode(&ir);
    assert!(emitted(&report.losses, UsdLossCode::ExportCurveNotWritten));

    let free = decode("#usda 1.0\ndef BasisCurves \"C\"\n{\n    uniform token type = \"linear\"\n    int[] curveVertexCounts = [2]\n    point3f[] points = [(0, 0, 0), (1, 0, 0)]\n}\n");
    let (mut ir, _, _) = free.into_parts();
    ir.geometry.edges.clear();
    ir.geometry.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle));
    let (_, report) = encode(&ir);
    assert!(emitted(
        &report.losses,
        UsdLossCode::ExportCarrierNotWritten
    ));

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let cylinder = cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        1.0,
    )
    .unwrap();
    ir.geometry.surfaces[0].geometry =
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder));
    // An edge curve with no shape leaves the face unmeshed.
    let face = ir
        .geometry
        .faces
        .iter()
        .find(|face| face.surface == ir.geometry.surfaces[0].id)
        .unwrap();
    let loop_ = ir
        .geometry
        .loops
        .iter()
        .find(|loop_| face.loops.contains(&loop_.id))
        .unwrap();
    let coedge = ir
        .geometry
        .coedges
        .iter()
        .find(|coedge| loop_.coedges().contains(&coedge.id))
        .unwrap();
    let curve = ir
        .geometry
        .edges
        .iter()
        .find(|edge| edge.id == coedge.edge)
        .unwrap()
        .curve()
        .unwrap()
        .clone();
    let slot = ir
        .geometry
        .curves
        .iter()
        .position(|candidate| candidate.id == curve)
        .unwrap();
    ir.geometry.curves[slot].geometry =
        CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None });
    let (_, report) = encode(&ir);
    assert!(emitted(&report.losses, UsdLossCode::ExportFaceNotWritten));

    let meshed = decode(&mesh(TRIANGLE));
    let (mesh_ir, _, _) = meshed.into_parts();
    let patch = decode(&format!(
        "#usda 1.0\ndef NurbsPatch \"P\"\n{{\n{PATCH}\n}}\n"
    ));
    let (mut ir, _, _) = patch.into_parts();
    let body = ir.geometry.bodies[0].id.clone();
    let tessellation =
        mesh_ir.model.tessellations[0]
            .clone()
            .with_of(cadmpeg_ir::tessellation::Meshed::Body {
                body,
                faces: Vec::new(),
            });
    ir.model.tessellations.push(tessellation);
    ir.geometry.surfaces[0].geometry =
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
    let (_, report) = encode(&ir);
    assert!(emitted(
        &report.losses,
        UsdLossCode::ExportTopologyTessellated
    ));

    let meshed = decode(&mesh(TRIANGLE));
    let (mut ir, _, _) = meshed.into_parts();
    let tessellation = ir.model.tessellations.remove(0);
    let channel =
        TessellationChannel::new(ChannelAddressing::Vertex {}, 4, 1, 0, vec![0; 12]).unwrap();
    let mesh = tessellation.mesh().clone();
    ir.model
        .tessellations
        .push(tessellation.with_mesh(mesh, vec![channel]).unwrap());
    let (_, report) = encode(&ir);
    assert!(emitted(
        &report.losses,
        UsdLossCode::ExportChannelNotWritten
    ));
}

/// A body keeps its exact B-rep, and its tessellation has a triangle group
/// that names a face of the body. No `GeomSubset` property states that
/// face, so the export reports it.
#[test]
fn a_triangle_group_face_is_reported_as_not_written() {
    let meshed = decode(&mesh(TRIANGLE));
    let (mesh_ir, _, _) = meshed.into_parts();
    let patch = decode(&format!(
        "#usda 1.0\ndef NurbsPatch \"P\"\n{{\n{PATCH}\n}}\n"
    ));
    let (mut ir, _, _) = patch.into_parts();
    let body = ir.geometry.bodies[0].id.clone();
    let face = ir.geometry.faces[0].id.clone();
    let group = TessellationTriangleGroup::new(Some("S".to_owned()), vec![0]).with_face(face);
    let tessellation = mesh_ir.model.tessellations[0]
        .clone()
        .with_of(cadmpeg_ir::tessellation::Meshed::Body {
            body,
            faces: Vec::new(),
        })
        .with_triangle_groups(vec![group])
        .unwrap();
    ir.model.tessellations.push(tessellation);
    let (text, report) = encode(&ir);
    assert!(text.contains("def GeomSubset \"S\""), "{text}");
    assert!(
        !emitted(&report.losses, UsdLossCode::ExportTopologyTessellated),
        "{:#?}",
        report.losses
    );
    let notes: Vec<_> = report
        .losses
        .iter()
        .filter(|loss| loss.code == UsdLossCode::ExportGroupFaceNotWritten.kind())
        .collect();
    assert_eq!(notes.len(), 1, "{:#?}", report.losses);
    assert!(
        notes[0]
            .message
            .contains("1 triangle group faces were not written"),
        "{}",
        notes[0].message
    );
}

#[test]
fn unknown_schema_losses_are_emitted() {
    assert_decode_emits(
        "#usda 1.0\ndef CadBrep \"B\"\n{\n}\n",
        UsdLossCode::PrimTypeNotTransferred,
    );
    assert_decode_emits(
        &format!("#usda 1.0\ndef Mesh \"M\" (\n    prepend apiSchemas = [\"PhysxVehicleAPI\"]\n)\n{{\n{TRIANGLE}\n}}\n"),
        UsdLossCode::ApiSchemaNotTransferred,
    );
}

#[test]
fn physics_losses_are_emitted() {
    assert_decode_emits(
        "#usda 1.0\ndef Xform \"B\"\n{\n}\ndef PhysicsFixedJoint \"J\"\n{\n    rel physics:body1 = </B>\n    float physics:breakForce = 10\n}\n",
        UsdLossCode::JointPropertyNotTransferred,
    );
    let source = decode("#usda 1.0\ndef Xform \"A\"\n{\n}\ndef Xform \"B\"\n{\n}\ndef PhysicsFixedJoint \"J\"\n{\n    rel physics:body0 = </A>\n    rel physics:body1 = </B>\n}\n");
    let (mut ir, _, _) = source.into_parts();
    let id = crate::ids::joint_feature("/J");
    let feature = ir
        .design
        .features
        .iter_mut()
        .find(|feature| feature.id == id)
        .expect("the joint feature");
    let cadmpeg_ir::design::feature::FeatureOperation::Joint(joint) =
        &mut feature.definition.operation
    else {
        panic!("not a joint feature");
    };
    joint.kind = cadmpeg_ir::design::ops::shared::JointKind::Parallel;
    let (_, report) = encode(&ir);
    assert!(emitted(&report.losses, UsdLossCode::ExportJointNotWritten));
}

/// A joint that the decoder reads and leaves out is given up by its id, one
/// `record.undecoded` per joint with its own cause.
/// A joint whose body is no prim of a whole stage breaks §16.4 and is
/// unreadable; a joint whose body is a prim with no occurrence, or whose
/// frame or bodies have no reading, is undecoded.
#[test]
fn a_rejected_joint_is_given_up_with_its_cause() {
    use cadmpeg_ir::unknown::part::Cause;

    let cases = [
        ("", "names no body", Cause::Undecoded),
        (
            "rel physics:body1 = </Missing>",
            "names a body that is no prim of the stage",
            Cause::Unreadable,
        ),
        (
            "rel physics:body1 = </Mat>",
            "names a body that is not a transferred prim",
            Cause::Undecoded,
        ),
        (
            "rel physics:body1 = </B>\n    quatf physics:localRot0 = (0, 0, 0, 0)",
            "has a malformed frame",
            Cause::Undecoded,
        ),
    ];
    for (body, why, cause) in cases {
        let text = format!(
            "#usda 1.0\ndef Xform \"B\"\n{{\n}}\ndef Material \"Mat\"\n{{\n}}\ndef PhysicsFixedJoint \"J\"\n{{\n    {body}\n}}\n"
        );
        let result = decode(&text);
        let joint = crate::ids::joint("/J");
        assert!(
            result
                .ir()
                .model
                .assembly_joints
                .iter()
                .all(|record| record.id != joint),
            "{text}"
        );
        let feature = crate::ids::joint_feature("/J");
        assert!(result
            .ir()
            .design
            .features
            .iter()
            .all(|record| record.id != feature));
        let losses = &result.report().losses;
        let message = format!("record {joint} given up: the joint {why}; dropped=[]");
        assert_given_up_once(losses, joint.as_str(), cause, &message);
        assert_eq!(
            losses
                .iter()
                .filter(|loss| cadmpeg_ir::unknown::part::gives_up(&loss.code))
                .count(),
            1,
            "{text}"
        );
        assert_eq!(
            decode_pinned(&text).is_ok(),
            cause == Cause::Undecoded,
            "{text}"
        );
    }
}

#[test]
fn design_state_beside_the_joints_is_reported_on_write() {
    let source = decode("#usda 1.0\ndef Xform \"A\"\n{\n}\ndef Xform \"B\"\n{\n}\ndef PhysicsFixedJoint \"J\"\n{\n    rel physics:body0 = </A>\n    rel physics:body1 = </B>\n}\n");
    let (mut ir, _, _) = source.into_parts();
    let (_, report) = encode(&ir);
    assert!(!emitted(
        &report.losses,
        UsdLossCode::ExportDesignNotWritten
    ));
    ir.design.scopes[0].active_arrangements.insert(
        cadmpeg_ir::design::ids::ArrangementId::mint("test:design:arrangement#open")
            .expect("identity grammar"),
    );
    let (_, report) = encode(&ir);
    assert!(emitted(&report.losses, UsdLossCode::ExportDesignNotWritten));
}

#[test]
fn an_unread_property_of_a_group_prim_is_reported() {
    assert_decode_emits(
        "#usda 1.0\ndef Xform \"A\"\n{\n    float custom:thing = 3\n    double3 xformOp:translate = (1, 0, 0)\n    uniform token[] xformOpOrder = [\"xformOp:translate\"]\n}\n",
        UsdLossCode::PropertyNotTransferred,
    );
}

/// A mesh point past the binary32 range has no `point3f` value. The mesh
/// prim is left out, the layer reads back, and the loss names the prim.
#[test]
fn a_mesh_point_past_the_float_range_leaves_its_prim_out() {
    let (mut ir, _, _) = decode(&mesh(TRIANGLE)).into_parts();
    let (written, _) = encode(&ir);
    let name = written
        .split("def Mesh \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("a mesh prim")
        .to_owned();
    let mut first = true;
    ir.model.tessellations[0]
        .edit_vertices(|point| {
            if std::mem::take(&mut first) {
                point.x = 1e300;
            }
            Ok(())
        })
        .expect("a finite point");
    let (text, report) = encode(&ir);
    assert!(!text.contains("def Mesh"), "{text}");
    assert!(!text.contains('\0'), "{text:?}");
    let notes: Vec<_> = report
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == "export/value.unwritable")
        .collect();
    assert_eq!(notes.len(), 1, "{:#?}", report.losses);
    assert!(
        !emitted(&report.losses, UsdLossCode::ExportVertexPrecision),
        "{:#?}",
        report.losses
    );
    assert!(
        notes[0].message.contains(&format!("/{name}")),
        "{}",
        notes[0].message
    );
    assert!(decode(&text).ir().model.tessellations.is_empty());
}

/// A shared prototype that is one mesh, and a point of that mesh past the
/// binary32 range: the prototype prim is left out, and so is each
/// occurrence prim whose reference reaches it. The losses name them, and
/// the layer reads back.
#[test]
fn an_occurrence_of_a_prototype_left_out_is_left_out() {
    let instance = |name: &str| {
        format!("    def \"{name}\" (\n        instanceable = true\n        references = </Plate>\n    )\n    {{\n    }}\n")
    };
    let source = decode(&format!(
        "#usda 1.0\nclass Mesh \"Plate\"\n{{\n{TRIANGLE}\n}}\ndef Xform \"Asm\"\n{{\n{}{}}}\n",
        instance("A"),
        instance("B"),
    ));
    let mut ir = source.ir().clone();
    let (written, _) = encode(&ir);
    let prototype = written
        .split("prepend references = <")
        .nth(1)
        .and_then(|rest| rest.split('>').next())
        .expect("an occurrence references its prototype")
        .to_owned();
    let name = prototype.rsplit('/').next().unwrap_or_default();
    assert!(
        written.contains(&format!("def Mesh \"{name}\"")),
        "the prototype prim is the mesh: {written}"
    );
    let mut first = true;
    ir.model.tessellations[0]
        .edit_vertices(|point| {
            if std::mem::take(&mut first) {
                point.x = 1e300;
            }
            Ok(())
        })
        .expect("a finite point");
    let (text, report) = encode(&ir);
    assert!(!text.contains("prepend references"), "{text}");
    for path in [prototype.as_str(), "/Asm/A", "/Asm/B"] {
        assert!(
            report.losses.iter().any(|loss| {
                loss.code.to_string() == "export/value.unwritable" && loss.message.contains(path)
            }),
            "{path}: {:#?}",
            report.losses
        );
    }
    let decoded = decode(&text);
    assert!(decoded.ir().model.tessellations.is_empty(), "{text}");
    assert_eq!(
        decoded.ir().model.occurrences.len(),
        source.ir().model.occurrences.len() - 2,
        "{text}"
    );
}

/// Each code that reports recovery from input that breaks the format
/// carries a strict floor (spec §18): a pinned decode refuses the input.
#[test]
fn floored_codes_refuse_a_pinned_decode() {
    let cases = [
        (
            "#usda 1.0\ndef \"A\" (\n    references = </Missing>\n)\n{\n}\n".to_owned(),
            UsdLossCode::CompositionTargetMissing,
        ),
        (
            "#usda 1.0\n(\n    metersPerUnit = -1\n)\ndef \"A\" {\n}\n".to_owned(),
            UsdLossCode::AttributeValueMalformed,
        ),
        (
            prim_layer("uniform token[] xformOpOrder = [\"xformOp:bogus\"]\nfloat xformOp:bogus = 1"),
            UsdLossCode::XformMalformed,
        ),
        (
            mesh("int[] faceVertexCounts = [3, 2]\nint[] faceVertexIndices = [0, 1, 2, 0, 1]\npoint3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]"),
            UsdLossCode::MeshFaceRejected,
        ),
        (
            "#usda 1.0\ndef NurbsCurves \"C\"\n{\n    int[] curveVertexCounts = [2, 2]\n    int[] order = [2, 3]\n    double[] knots = [0, 0, 1, 1, 0, 0, 0, 1, 1]\n    point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0), (1, 1, 0)]\n}\n".to_owned(),
            UsdLossCode::CurvesRejected,
        ),
        (
            format!("#usda 1.0\ndef NurbsPatch \"P\"\n{{\n{PATCH}\ndouble2 uRange = (0, 2)\n}}\n"),
            UsdLossCode::NurbsRangeNotTransferred,
        ),
    ];
    for (text, code) in cases {
        assert_decode_emits(&text, code);
        match decode_pinned(&text) {
            Err(cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection }) => {
                assert_eq!(rejection.loss().code, code.kind(), "{text}");
            }
            Ok(_) => panic!("expected a strict refusal of {code:?}:\n{text}"),
            Err(other) => panic!("expected a strict refusal, got {other:?}"),
        }
    }
}

/// The codes split from the floored codes report valid input that the
/// decoder does not transfer: a pinned decode accepts it.
#[test]
fn unfloored_codes_accept_a_pinned_decode() {
    let cases = [
        (
            "#usda 1.0\ndef \"Lib\" (\n    references = </Other>\n)\n{\n}\ndef \"Other\"\n{\n}\ndef \"A\" (\n    references = </Lib/Part>\n)\n{\n}\n".to_owned(),
            UsdLossCode::CompositionTargetUnresolved,
        ),
        (
            prim_layer("matrix4d xformOp:transform = ((1, 0, 0, 1), (0, 1, 0, 0), (0, 0, 1, 0), (0, 0, 0, 1))\nuniform token[] xformOpOrder = [\"xformOp:transform\"]"),
            UsdLossCode::XformNotTransferred,
        ),
        (
            "#usda 1.0\ndef BasisCurves \"C\"\n{\n    uniform token type = \"linear\"\n    int[] curveVertexCounts = [2, 1]\n    point3f[] points = [(0, 0, 0), (1, 0, 0), (2, 0, 0)]\n}\n".to_owned(),
            UsdLossCode::CurvesNotTransferred,
        ),
    ];
    for (text, code) in cases {
        assert_decode_emits(&text, code);
        assert!(decode_pinned(&text).is_ok(), "{code:?}:\n{text}");
    }
}

/// A NURBS curve whose knots decrease breaks §13.2: where no curve of the
/// batch transfers, one `record.unreadable` names the batch's body.
#[test]
fn a_curve_batch_whose_knots_decrease_is_unreadable() {
    let text = "#usda 1.0\ndef NurbsCurves \"C\"\n{\n    int[] curveVertexCounts = [2]\n    int[] order = [2]\n    double[] knots = [0, 1, 0.5, 1]\n    point3f[] points = [(0, 0, 0), (1, 0, 0)]\n}\n";
    let body = crate::ids::body("/C");
    let message = format!("record {body} given up: no curve of the batch transfers; dropped=[]");
    let result = decode(text);
    assert!(result.ir().geometry.edges.is_empty());
    assert_given_up_once(
        &result.report().losses,
        body.as_str(),
        cadmpeg_ir::unknown::part::Cause::Unreadable,
        &message,
    );
}

/// A NURBS patch whose U knots decrease breaks §14: one `record.unreadable`
/// names the prim's body.
#[test]
fn a_patch_whose_knots_decrease_is_unreadable() {
    let text = format!(
        "#usda 1.0\ndef NurbsPatch \"P\"\n{{\n{}\n}}\n",
        PATCH.replace(
            "double[] uKnots = [0, 0, 1, 1]",
            "double[] uKnots = [0, 1, 0.5, 1]"
        )
    );
    let body = crate::ids::body("/P");
    let message = format!(
        "record {body} given up: the NURBS patch has a point that is not finite, or knots or \
         weights that no NURBS surface admits; dropped=[]"
    );
    let result = decode(&text);
    assert!(result.ir().geometry.faces.is_empty());
    assert_given_up_once(
        &result.report().losses,
        body.as_str(),
        cadmpeg_ir::unknown::part::Cause::Unreadable,
        &message,
    );
}

/// A malformed value that leaves a mesh with no points is one fact: the
/// give-up of the tessellation names it, and no
/// `attribute.value-malformed` note repeats it.
#[test]
fn a_malformed_value_of_a_given_up_mesh_is_one_entry() {
    let text = mesh("int[] faceVertexCounts = [3]\nint[] faceVertexIndices = [0, 1, 2]\npoint3f[] points = [1, 2, 3]");
    let losses = decode(&text).report().losses.clone();
    let naming: Vec<_> = losses
        .iter()
        .filter(|loss| loss.message.contains("/M.points"))
        .collect();
    let [loss] = naming.as_slice() else {
        panic!("one loss names /M.points: {losses:#?}");
    };
    assert_eq!(
        loss.code,
        cadmpeg_ir::unknown::part::Cause::Unreadable.kind(),
        "{loss:#?}"
    );
    assert!(
        !emitted(&losses, UsdLossCode::AttributeValueMalformed),
        "{losses:#?}"
    );
}
