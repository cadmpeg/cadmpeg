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
        &mesh("int[] faceVertexCounts = [3]\nint[] faceVertexIndices = [0, 1, 2]"),
        UsdLossCode::MeshRejected,
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

/// A mesh prim with no points is given up: the default decode charges
/// `mesh.rejected` and the prim's `record.unreadable`, and a pinned decode
/// refuses on the `record.unreadable`.
#[test]
fn a_rejected_mesh_is_given_up_and_refuses_a_pinned_decode() {
    use cadmpeg_ir::codec::Codec as _;

    let text = mesh("int[] faceVertexCounts = [3]\nint[] faceVertexIndices = [0, 1, 2]");
    let message = format!(
        "record {} given up: usd/mesh.rejected; dropped=[]",
        crate::ids::tessellation("/M")
    );
    let given_up = |loss: &LossNote| {
        loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind() && loss.message == message
    };

    let result = decode(&text);
    assert!(result.ir().model.tessellations.is_empty());
    let losses = &result.report().losses;
    assert!(emitted(losses, UsdLossCode::MeshRejected), "{losses:#?}");
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind())
            .count(),
        1,
        "{losses:#?}"
    );
    assert!(losses.iter().any(given_up), "{losses:#?}");

    let mut options = cadmpeg_ir::codec::DecodeOptions::default();
    options.strictness.pinned = true;
    match crate::UsdCodec.decode(&mut std::io::Cursor::new(text.as_bytes()), &options) {
        Err(cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection }) => {
            assert!(given_up(rejection.loss()), "{:#?}", rejection.loss());
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
    let message = format!(
        "record {} given up: usd/mesh.rejected; dropped=[{}]",
        crate::ids::tessellation("/M"),
        crate::ids::appearance_binding("/M")
    );
    let losses = &result.report().losses;
    assert!(
        losses.iter().any(|loss| {
            loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind()
                && loss.message == message
        }),
        "{losses:#?}"
    );
}

/// A `BasisCurves` prim whose cubic basis the reader does not implement
/// states no curve: the prim is given up, and a pinned decode refuses it on
/// its `record.unreadable`.
#[test]
fn a_curve_batch_with_an_unsupported_basis_is_given_up() {
    use cadmpeg_ir::codec::Codec as _;

    let text = "#usda 1.0\ndef BasisCurves \"C\"\n{\n    uniform token basis = \"hermite\"\n    int[] curveVertexCounts = [4]\n    point3f[] points = [(0, 0, 0), (1, 0, 0), (2, 0, 0), (3, 0, 0)]\n}\n";
    let message = format!(
        "record {} given up: usd/curves.rejected; dropped=[]",
        crate::ids::body("/C")
    );
    let given_up = |loss: &LossNote| {
        loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind() && loss.message == message
    };

    let result = decode(text);
    assert!(result.ir().geometry.edges.is_empty());
    let losses = &result.report().losses;
    assert!(losses.iter().any(given_up), "{losses:#?}");

    let mut options = cadmpeg_ir::codec::DecodeOptions::default();
    options.strictness.pinned = true;
    match crate::UsdCodec.decode(&mut std::io::Cursor::new(text.as_bytes()), &options) {
        Err(cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection }) => {
            assert!(given_up(rejection.loss()), "{:#?}", rejection.loss());
        }
        Ok(_) => panic!("expected a strict refusal"),
        Err(other) => panic!("expected a strict refusal, got {other:?}"),
    }
}

#[test]
fn curve_and_patch_losses_are_emitted() {
    assert_decode_emits(
        "#usda 1.0\ndef BasisCurves \"C\"\n{\n    uniform token basis = \"catmullRom\"\n    int[] curveVertexCounts = [3]\n    point3f[] points = [(0, 0, 0), (1, 0, 0), (2, 0, 0)]\n}\n",
        UsdLossCode::CurvesRejected,
    );
    assert_decode_emits(
        &format!(
            "#usda 1.0\ndef NurbsPatch \"P\"\n{{\n{}\n}}\n",
            PATCH.replace("int uVertexCount = 2", "int uVertexCount = 3")
        ),
        UsdLossCode::NurbsPatchRejected,
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
        "#usda 1.0\ndef PhysicsFixedJoint \"J\"\n{\n    rel physics:body1 = </Missing>\n}\n",
        UsdLossCode::JointRejected,
    );
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
