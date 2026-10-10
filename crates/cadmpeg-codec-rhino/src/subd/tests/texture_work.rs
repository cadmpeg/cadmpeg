// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use crate::chunks::{ArchiveVersion, BoundedReader};

#[test]
fn subd_texture_remainder_has_no_separate_work_charge() {
    let fixture = super::Fixture { archive: ArchiveVersion::V7, ..super::Fixture::default() };
    let mut bytes = Vec::new();
    super::face(&mut bytes, fixture, 0);
    assert_eq!(bytes.pop(), Some(255));
    bytes.extend([0, 0, 0, 0, 4]);
    bytes.extend(0_u32.to_le_bytes());
    bytes.push(96);
    for _ in 0..12 { bytes.extend(0.0_f64.to_le_bytes()); }
    bytes.push(255);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "Rhino SubD texture remainder", None);
    let mut storage = ctx.reserve_scoped(0, "test face scratch").unwrap();
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    let face = super::super::read_face(&ctx, &mut storage, &mut reader, fixture.archive, 9, 0).unwrap();
    assert_eq!(face.edges.len(), 4);
    assert_eq!(reader.remaining(), 0);
    drop(face);
    drop(storage);
    assert!(ctx.finish_session().is_ok());
}
