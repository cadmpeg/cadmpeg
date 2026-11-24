// SPDX-License-Identifier: Apache-2.0
//! Parts that a product's shape states only as triangles.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::StepLossCode;
use crate::StepCodec;

/// A tetrahedron stated only as a triangulated surface set, the product
/// structure of one part, and the shape representation `shape` that the
/// part's `SHAPE_DEFINITION_REPRESENTATION` names, with `extra` records.
fn tetrahedron_part(shape: u32, extra: &str) -> String {
    format!(
        "ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('tessellated part'),'2;1');
FILE_NAME('part','2026-11-16T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');
FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));
ENDSEC;
DATA;
#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));
#11=COORDINATES_LIST('corners',4,((0.,0.,0.),(10.,0.,0.),(0.,10.,0.),(0.,0.,10.)));
#12=TRIANGULATED_SURFACE_SET('tetrahedron',#11,4,(),(),((1,3,2),(1,2,4),(2,3,4),(3,1,4)));
#13=TESSELLATED_SHAPE_REPRESENTATION('mesh',(#12),#2);
#20=APPLICATION_CONTEXT('managed model based 3d engineering');
#21=PRODUCT_CONTEXT('',#20,'mechanical');
#22=PRODUCT('tetrahedron','tetrahedron','',(#21));
#23=PRODUCT_DEFINITION_FORMATION('','',#22);
#24=PRODUCT_DEFINITION_CONTEXT('part definition',#20,'design');
#25=PRODUCT_DEFINITION('design','',#23,#24);
#26=PRODUCT_DEFINITION_SHAPE('','',#25);
#27=SHAPE_DEFINITION_REPRESENTATION(#26,#{shape});
{extra}ENDSEC;
END-ISO-10303-21;
"
    )
}

/// The part's one product definition owns the mesh, the mesh represents no
/// body, and the model validates.
fn assert_owned_mesh(source: &str) {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(source.as_bytes()),
            &DecodeOptions::default(),
        )
        .expect("the tessellated part decodes");
    let ir = decoded.ir();
    assert!(ir.geometry.bodies.is_empty());
    assert_eq!(ir.model.tessellations.len(), 1);
    let mesh = &ir.model.tessellations[0];
    assert_eq!(mesh.of.body().cloned(), None);
    assert_eq!(ir.model.product_definitions.len(), 1);
    let definition = &ir.model.product_definitions[0];
    assert!(definition.bodies.is_empty());
    assert_eq!(
        mesh.of,
        cadmpeg_ir::tessellation::Meshed::definition(definition.id.clone())
    );
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::TessellationItemUndeclared.kind()));
    let validation = cadmpeg_ir::validate_neutral(ir, decoded.report().losses.clone())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn a_part_shape_that_lists_a_mesh_owns_it() {
    assert_owned_mesh(&tetrahedron_part(13, ""));
}

#[test]
fn a_mesh_related_to_a_part_shape_belongs_to_the_part() {
    let extra = "#30=CARTESIAN_POINT('origin',(0.,0.,0.));
#31=DIRECTION('z',(0.,0.,1.));
#32=DIRECTION('x',(1.,0.,0.));
#33=AXIS2_PLACEMENT_3D('',#30,#31,#32);
#34=SHAPE_REPRESENTATION('part',(#33),#2);
#35=SHAPE_REPRESENTATION_RELATIONSHIP('','',#34,#13);
";
    assert_owned_mesh(&tetrahedron_part(34, extra));
}
