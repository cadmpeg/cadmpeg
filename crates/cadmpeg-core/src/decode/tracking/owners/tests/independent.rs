// SPDX-License-Identifier: Apache-2.0
//! Common source facts have independent actual getters under their own owners.
use super::*;

#[test]
fn separate_producers_use_their_actual_values_and_failed_getters_roll_back() {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"\x02\x07\x09", &arena, &DecodePolicy::default())
            .expect("create tracked fixture source context");
    let (payload_kind, payload_owner) = ctx
        .record_owner("payload", source, || {
            let mut field = source
                .child(0, 1)
                .expect("select bounded original source extent");
            Ok(field.u8().expect("read complete fixture byte field"))
        })
        .expect("declare actual primary source owner");
    let (census_family, census_owner) = ctx
        .record_owner("census", source, || {
            let mut field = source
                .child(0, 1)
                .expect("select bounded original source extent");
            Ok(
                match field.u8().expect("read complete fixture byte field") {
                    2 => "circle",
                    _ => "other",
                },
            )
        })
        .expect("declare actual primary source owner");
    assert_eq!((payload_kind, census_family), (2, "circle"));
    ctx.record_fragment(&payload_owner, source, || {
        let reference_kind = ctx.record(
            "row",
            source
                .child(1, 2)
                .expect("select bounded original source extent"),
            || {
                let mut field = source
                    .child(1, 2)
                    .expect("select bounded original source extent");
                Ok(field.u8().expect("read complete fixture byte field"))
            },
        )?;
        assert_eq!(reference_kind, 7);
        let failed: Result<(), CodecError> = ctx.record_fragment(&census_owner, source, || {
            let mut field = source
                .child(2, 3)
                .expect("select bounded original source extent");
            let count = field.u8().expect("read complete fixture byte field");
            assert_eq!(count, 9);
            Err(CodecError::malformed("failed census field"))
        });
        assert!(failed.is_err());
        Ok(())
    })
    .expect("commit independent source continuation");
    let captures = ctx
        .take_record_captures()
        .expect("extract committed source record captures");
    for id in ["payload", "census"] {
        let capture = captures
            .iter()
            .find(|capture| capture.id() == id)
            .expect("locate expected retained producer capture");
        assert_eq!(
            capture.reads()["file"],
            std::iter::once(0..1).collect::<Vec<std::ops::Range<u64>>>()
        );
    }
    assert_eq!(
        captures
            .iter()
            .find(|capture| capture.id() == "row")
            .expect("locate expected retained producer capture")
            .reads()["file"],
        std::iter::once(1..2).collect::<Vec<std::ops::Range<u64>>>()
    );
}
