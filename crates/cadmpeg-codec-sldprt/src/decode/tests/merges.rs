// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::topology::Point;

use super::super::{merge_brep, Brep};

#[test]
fn brep_merge_moves_arena_records_without_copying_identities() {
    let mut target = Brep::default();
    let mut source = Brep::default();
    source.points.push(Point::new(
        "sldprt:model:point#one".try_into().unwrap(),
        FinitePoint3::ZERO,
        None,
    ));
    let identity_storage = source.points[0].id.as_str().as_ptr();
    merge_brep(&mut target, source).unwrap();
    assert_eq!(target.points[0].id.as_str(), "sldprt:model:point#one");
    assert_eq!(target.points[0].id.as_str().as_ptr(), identity_storage);
}
