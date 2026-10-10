// SPDX-License-Identifier: Apache-2.0


use crate::surface::{opaque_spans, scalar_frames, SurfaceParameterScalar};

#[test]
fn opaque_scalar_spans_visit_each_token_once_without_a_terminal_probe() {
    for count in [0, 1, 7, 17, 257] {
        let body = vec![0xe4; count];
        let tokens: Vec<_> = (0..count).map(|offset| SurfaceParameterScalar {
            value: Some(1.0), raw: vec![0xe4], offset,
        }).collect();
        let spans = crate::test_support::assert_work_boundaries(
            if count == 0 { &[] } else { &["creo surface opaque span traversal"] },
            |ctx| opaque_spans(ctx, &body, &tokens),
        );
        assert!(spans.is_empty());
    }
}

#[test]
fn opaque_scalar_spans_preserve_prefix_middle_and_terminal_gap_identity() {
    for gap in [1, 7, 17, 257] {
        let mut body = vec![0xed; gap];
        let mut tokens = Vec::new();
        for _ in 0..3 {
            tokens.push(SurfaceParameterScalar { value: Some(1.0), raw: vec![0xe4], offset: body.len() });
            body.push(0xe4);
            body.extend(std::iter::repeat_n(0xed, gap));
        }
        let spans = super::work_output(|ctx| opaque_spans(ctx, &body, &tokens));
        assert_eq!(spans.len(), 4);
        for (index, span) in spans.iter().enumerate() {
            assert_eq!(span.offset, index * (gap + 1));
            assert_eq!(span.raw, vec![0xed; gap]);
        }

    }
}

#[test]
fn scalar_frames_visit_each_start_adjacent_pair_and_copied_token_once() {
    for count in [0_usize, 1, 2, 4] {
        for separated in [false, true] {
            let tokens: Vec<_> = (0..count).map(|index| SurfaceParameterScalar {
                value: Some(1.0), raw: vec![0xe4], offset: index * if separated { 2 } else { 1 },
            }).collect();
            let frame_count = if separated { count } else { usize::from(count != 0) };
            let frames = crate::test_support::assert_work_boundaries(
                if count == 0 { &[] } else { &["creo surface scalar frame traversal", "creo surface scalar frame bytes"] },
                |ctx| scalar_frames(ctx, &tokens),
            );
            assert_eq!(frames.len(), frame_count);
            if separated {
                for (index, frame) in frames.iter().enumerate() {
                    assert_eq!(frame.offset, 2 * index);
                    assert_eq!(frame.slots, tokens[index..=index]);
                }
            } else if count != 0 {
                assert_eq!(frames[0].offset, 0);
                assert_eq!(frames[0].slots, tokens);
            }
        }
    }
}
