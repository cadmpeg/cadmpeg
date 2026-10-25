// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};
use cadmpeg_ir::{CadIr, CodecFormat};

fn fixture(inspect: impl FnOnce(&DecodeContext<'_>, View<'_>, &[FramedPart<'_>], &PartOutput)) {
    let member = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 2\n1 0 0 0 1 0 0\n1 2 0 0 1 0 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 1\nCo 0101000 *\n+1 0 *";
    let bytes =
        crate::zip_fixture::archive(&[crate::zip_fixture::RawEntry::stored("Shape.brp", member)]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let mut frames = Vec::new();
    let entries = crate::framing::zip::frame(&ctx, root, &mut frames).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let source = archive.open(&ctx, "Shape.brp").unwrap();
    crate::framing::freecad::brep::frame_brep_member(
        &ctx,
        source,
        entries[0].name(),
        entries[0].frame_index(),
        &mut frames,
    )
    .unwrap();
    let mut output = PartOutput::new(CodecFormat::Fcstd);
    fn visit(
        ctx: &DecodeContext<'_>,
        frames: &[FramedPart<'_>],
        index: usize,
        output: &mut PartOutput,
    ) {
        ctx.decode_part(
            output,
            frames[index].view(),
            frames[index].spec(),
            |stage| {
                for child in 0..frames.len() {
                    if frames[child].parent() == FrameParent::Part(index) {
                        visit(ctx, frames, child, stage);
                    }
                }
                Ok(())
            },
        )
        .unwrap();
    }
    visit(&ctx, &frames, 0, &mut output);
    inspect(&ctx, source, &frames, &output);
}

#[test]
fn original_sequential_target_owns_its_parent_suffix_and_never_the_prior_peer() {
    fixture(|ctx, source, frames, output| {
        let index = frames
            .iter()
            .position(|frame| frame.level().as_str() == "freecad.brep_text_curve")
            .unwrap();
        let target = DamageTarget::new(ctx, frames, index).unwrap();
        let ir = CadIr::empty();
        let before = DecodeSnapshot::new(&ir, output.fidelity());
        let mut damaged = source.window().to_vec();
        damaged[usize::try_from(target.capture.offset()).unwrap()] = b'9';
        let resolve = |range: Range<u64>| {
            usize::try_from(range.start)
                .ok()
                .zip(usize::try_from(range.end).ok())
        };
        let witness = target
            .witness(
                before,
                |stream, range| {
                    (stream == target.stream())
                        .then(|| {
                            resolve(range).and_then(|(start, end)| source.window().get(start..end))
                        })
                        .flatten()
                },
                |stream, range| {
                    (stream == target.stream())
                        .then(|| resolve(range).and_then(|(start, end)| damaged.get(start..end)))
                        .flatten()
                },
            )
            .unwrap();
        let FrameParent::Part(parent) = frames[index].parent() else {
            panic!("real curve parent")
        };
        assert_eq!(
            witness,
            vec![(
                target.stream().to_owned(),
                target.capture.offset(),
                frames[parent].capture().offset() + frames[parent].capture().byte_len()
                    - target.capture.offset()
            )]
        );
        assert!(witness[0].1 > frames[parent].capture().offset());
    });
}

#[test]
fn original_damage_authority_rejects_changed_descendant_framing_and_foreign_bytes() {
    fixture(|ctx, source, frames, output| {
        let index = frames
            .iter()
            .position(|frame| {
                frame.level().as_str() == "freecad.brep_text_table"
                    && frame.view().window().starts_with(b"TShapes")
            })
            .unwrap();
        let target = DamageTarget::new(ctx, frames, index).unwrap();
        let ir = CadIr::empty();
        let before = DecodeSnapshot::new(&ir, output.fidelity());
        let mut damaged = source.window().to_vec();
        let star = frames
            .iter()
            .find(|frame| frame.level().as_str() == "freecad.brep_text_tshape")
            .unwrap()
            .framing()[0]
            .start;
        damaged[usize::try_from(star).unwrap()] = 0;
        fn resolve(bytes: &[u8], range: Range<u64>) -> Option<&[u8]> {
            bytes.get(usize::try_from(range.start).ok()?..usize::try_from(range.end).ok()?)
        }
        assert!(target
            .witness(
                before,
                |_, range| resolve(source.window(), range),
                |_, range| resolve(&damaged, range)
            )
            .is_err());
        assert!(target
            .witness(
                before,
                |_, _| Some(b"foreign bytes"),
                |_, range| resolve(&damaged, range)
            )
            .is_err());
    });
}

#[test]
fn an_incomplete_inventory_cannot_authorize_damage_to_an_actual_child_terminator() {
    fixture(|ctx, source, frames, output| {
        let index = frames
            .iter()
            .position(|frame| {
                frame.level().as_str() == "freecad.brep_text_table"
                    && frame.view().window().starts_with(b"TShapes")
            })
            .unwrap();
        // A caller omits the real child and the following root from its census.
        // The genuine BEFORE helper tree still proves that child's framing.
        let incomplete = &frames[..index + 1];
        let target = DamageTarget::new(ctx, incomplete, index).unwrap();
        let star = frames
            .iter()
            .find(|frame| frame.level().as_str() == "freecad.brep_text_tshape")
            .unwrap()
            .framing()[0]
            .start;
        let mut damaged = source.window().to_vec();
        damaged[usize::try_from(star).unwrap()] = 0;
        let ir = CadIr::empty();
        let before = DecodeSnapshot::new(&ir, output.fidelity());
        fn resolve(bytes: &[u8], range: Range<u64>) -> Option<&[u8]> {
            bytes.get(usize::try_from(range.start).ok()?..usize::try_from(range.end).ok()?)
        }
        assert!(target
            .witness(
                before,
                |_, range| resolve(source.window(), range),
                |_, range| resolve(&damaged, range)
            )
            .is_err());
    });
}
