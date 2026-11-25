// SPDX-License-Identifier: Apache-2.0
//! Values the STEP writer cannot state: a non-finite real, and a replica
//! transform that is not a similarity, are left out with the rest written.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::transform::Transform;

use crate::export::write_step;
use crate::{StepCodec, StepSchema, StepWriteOptions};

/// The sweep vector's components are finite, and its length is past the
/// largest finite `f64`: the sweep is not written, and the file is.
#[test]
fn a_linear_sweep_whose_magnitude_overflows_is_left_out() {
    use cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
    use cadmpeg_ir::geometry::surface_payloads::LinearSweepSurfaceConstruction;

    let source = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../../../tests/fixtures/ap242_geometry.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode procedural geometry");
    let mut ir = source.ir().clone();
    let sweep = ir
        .geometry
        .procedural_surfaces
        .iter_mut()
        .find(|surface| {
            matches!(
                surface.definition(),
                ProceduralSurfaceDefinition::LinearSweep(_)
            )
        })
        .expect("the fixture states a linear sweep");
    let sweep_id = sweep.id.as_str().to_owned();
    sweep.edit_definition(|definition| {
        let ProceduralSurfaceDefinition::LinearSweep(construction) = definition else {
            panic!("the selected surface is a linear sweep");
        };
        *construction = LinearSweepSurfaceConstruction::try_new(
            construction.directrix().clone(),
            Vector3::new(1.3e308, 1.3e308, 0.0),
        )
        .expect("finite components admit the sweep");
    });

    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("an overflowing sweep magnitude is left out");
    let text = String::from_utf8(bytes.clone()).expect("STEP output is UTF-8");
    assert!(!text.contains('\0'), "{text}");
    assert!(!text.contains("SURFACE_OF_LINEAR_EXTRUSION"), "{text}");
    assert!(
        report.losses.iter().any(|loss| {
            loss.code.to_string() == "export/value.unwritable" && loss.message.contains(&sweep_id)
        }),
        "{:?}",
        report.losses
    );
    StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the written file");
}

/// The section leaves out an instance that holds a non-finite number and
/// every instance that references it, where it wrote `0.` in its place, and
/// keeps the others under their instance names.
#[test]
fn the_section_leaves_out_an_instance_with_a_non_finite_value() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut emitter = crate::writer::Emitter::new();
        assert_eq!(crate::writer::Emitter::real(1.5), "1.5");
        let kept = emitter.emit(
            "CARTESIAN_POINT",
            &format!("'',({},0.,0.)", crate::writer::Emitter::real(1.5)),
        );
        let left_out = emitter.emit(
            "CARTESIAN_POINT",
            &format!("'',({},0.,0.)", crate::writer::Emitter::real(value)),
        );
        let user = emitter.emit("VERTEX_POINT", &format!("'',{left_out}"));
        let other = emitter.emit("VERTEX_POINT", &format!("'',{kept}"));
        let dropped = emitter.drop_non_finite();
        assert_eq!(
            dropped
                .iter()
                .map(|instance| instance.name)
                .collect::<Vec<_>>(),
            [left_out, user],
            "{value}"
        );
        assert_eq!(
            emitter.into_lines(),
            [
                format!("{kept} = CARTESIAN_POINT('',(1.5,0.,0.));"),
                format!("{other} = VERTEX_POINT('',{kept});"),
            ],
            "{value}"
        );
    }
}

/// A model written by [`replica_written_with`].
struct WrittenReplica {
    /// The STEP text.
    text: String,
    /// The losses of the write.
    losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    /// The identity of the model's one procedural replica.
    replica: String,
}

/// Decodes `records`, gives each procedural replica `transform`, and writes
/// the model.
fn replica_written_with(records: &str, transform: Transform) -> WrittenReplica {
    use cadmpeg_ir::geometry::procedural_curves::ProceduralCurveDefinition;
    use cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition;

    let source = crate::test_support::exchange::decode_inline(records);
    let mut ir = source.ir().clone();
    let mut replicas = Vec::new();
    for surface in &mut ir.geometry.procedural_surfaces {
        let id = surface.id.as_str().to_owned();
        surface.edit_definition(|definition| {
            if let ProceduralSurfaceDefinition::Replica {
                transform: held, ..
            } = definition
            {
                *held = transform;
                replicas.push(id);
            }
        });
    }
    for curve in &mut ir.geometry.procedural_curves {
        let id = curve.id.as_str().to_owned();
        curve.edit_definition(|definition| {
            if let ProceduralCurveDefinition::Replica {
                transform: held, ..
            } = definition
            {
                *held = transform;
                replicas.push(id);
            }
        });
    }
    assert_eq!(replicas.len(), 1, "the exchange states one replica");
    let mut bytes = Vec::new();
    let outcome = crate::export::write_step_outcome(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
        std::collections::HashSet::new(),
    )
    .expect("a replica is written or left out");
    WrittenReplica {
        text: String::from_utf8(bytes).expect("STEP output is UTF-8"),
        losses: outcome.losses,
        replica: replicas.remove(0),
    }
}

const SURFACE_REPLICA_EXCHANGE: &str = "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=AXIS2_PLACEMENT_3D('',#1,#4,#2);
#6=PLANE('',#5);
#7=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#1,2.,#4);
#8=SURFACE_REPLICA('',#6,#7);
#9=RECTANGULAR_TRIMMED_SURFACE('',#8,0.,1.,0.,1.,.T.,.T.);
#10=GEOMETRIC_SET('',(#9));
#11=SHAPE_REPRESENTATION('',(#10),#12);
#12=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));";

const CURVE_REPLICA_EXCHANGE: &str = "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=VECTOR('',#2,2.);
#6=LINE('',#1,#5);
#7=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#1,3.,#4);
#8=CURVE_REPLICA('',#6,#7);
#9=TRIMMED_CURVE('',#8,(PARAMETER_VALUE(1.)),(PARAMETER_VALUE(2.)),.T.,.PARAMETER.);
#10=GEOMETRIC_CURVE_SET('',(#9));
#11=SHAPE_REPRESENTATION('',(#10),$);";

/// A procedural replica under `rows` is not written: the loss names it, the
/// solved carrier stands in, and no replica of the file read back holds
/// `rows`.
fn assert_replica_is_left_out(exchange: &str, rows: [[f64; 4]; 3]) {
    use cadmpeg_ir::geometry::procedural_curves::ProceduralCurveDefinition;
    use cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition;

    let transform = Transform::affine(rows).expect("affine");
    let written = replica_written_with(exchange, transform);
    assert!(
        written.losses.iter().any(|loss| {
            loss.code.to_string() == "export/value.unwritable"
                && loss.message.contains(&written.replica)
        }),
        "{:?}",
        written.losses
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(&written.text), &DecodeOptions::default())
        .expect("decode the written file")
        .into_parts()
        .0;
    let surfaces = decoded
        .geometry
        .procedural_surfaces
        .iter()
        .filter_map(|surface| match surface.definition() {
            ProceduralSurfaceDefinition::Replica { transform, .. } => Some(*transform),
            _ => None,
        });
    let curves = decoded
        .geometry
        .procedural_curves
        .iter()
        .filter_map(|curve| match curve.definition() {
            ProceduralCurveDefinition::Replica { transform, .. } => Some(*transform),
            _ => None,
        });
    assert!(
        surfaces.chain(curves).all(|held| held != transform),
        "{}",
        written.text
    );
}

const SHEAR: [[f64; 4]; 3] = [
    [1.0, 0.5, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// A procedural replica whose transform is a similarity is written as a
/// replica.
#[test]
fn a_procedural_replica_with_a_similarity_is_written() {
    let similarity = Transform::affine([
        [0.0, -2.0, 0.0, 10.0],
        [2.0, 0.0, 0.0, 20.0],
        [0.0, 0.0, 2.0, 30.0],
    ])
    .expect("affine transform");
    for (exchange, replica) in [
        (SURFACE_REPLICA_EXCHANGE, "SURFACE_REPLICA"),
        (CURVE_REPLICA_EXCHANGE, "CURVE_REPLICA"),
    ] {
        let text = replica_written_with(exchange, similarity).text;
        assert!(text.contains(replica), "{text}");
    }
}

/// A sheared surface replica is left out, where it was written as a
/// different transform.
#[test]
fn a_sheared_surface_replica_is_left_out() {
    assert_replica_is_left_out(SURFACE_REPLICA_EXCHANGE, SHEAR);
}

/// A sheared curve replica is left out, where it was written as a
/// different transform.
#[test]
fn a_sheared_curve_replica_is_left_out() {
    assert_replica_is_left_out(CURVE_REPLICA_EXCHANGE, SHEAR);
}

/// A replica transform with a zero column is left out, where the column was
/// written as the direction `(0,0,1)`.
#[test]
fn a_replica_with_a_zero_column_is_left_out() {
    assert_replica_is_left_out(
        CURVE_REPLICA_EXCHANGE,
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 0.0],
        ],
    );
}
