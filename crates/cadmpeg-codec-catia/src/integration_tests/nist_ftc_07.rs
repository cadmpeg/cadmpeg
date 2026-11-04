// SPDX-License-Identifier: Apache-2.0
//! Surface content of the FTC-07 solid, compared with its AP242 model.

use std::io::Cursor;
use std::path::Path;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};

use crate::CatiaCodec;

#[test]
fn nist_ftc_07_transfers_every_solid_face_carrier() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/nist-pmi/catia/nist_ftc_07_asme1_rd_ct5290_ra.CATPart");
    let bytes = std::fs::read(path).expect("read FTC-07 CATPart");
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode FTC-07 CATPart");
    let model = &decoded.ir().model;
    assert_eq!(model.faces.len(), 304);
    let mut nurbs = 0;
    for face in &model.faces {
        let surface = model
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
            .expect("face carrier exists");
        match &surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_)) => nurbs += 1,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. }) => {
                panic!("unresolved face carrier")
            }
            SurfaceGeometry::Solved(_) => {}
            SurfaceGeometry::Procedural { .. } => panic!("unresolved face carrier construction"),
        }
    }
    assert_eq!(nurbs, 20);
}
