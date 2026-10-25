// SPDX-License-Identifier: Apache-2.0
//! Legacy mapped-name records have no independent resynchronization marker.

use cadmpeg_core::decode::{DecodeContext, View};

use super::{emit, retain_push, Tokens};
use crate::framing::FramedPart;

/// Frame inline legacy records, or the counted records of a legacy side entry.
/// `count=None` reads the leading side-member count; `Some` is the XML count.
pub fn frame_legacy_records<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    count: Option<usize>,
    parent: usize,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    let mut tokens = Tokens::new(source);
    let side = count.is_none();
    let count = match count {
        Some(count) => count,
        None => {
            let field = tokens.token()?;
            tokens.count(field)?
        }
    };
    if count > source.window().len() {
        return Err("legacy map count exceeds source extent".into());
    }
    for _ in 0..count {
        tokens.skip();
        let start = tokens.position;
        let mut framing = Vec::new();
        tokens.token()?;
        retain_push(ctx, &mut framing, tokens.skip())?;
        tokens.token()?;
        retain_push(ctx, &mut framing, tokens.skip())?;
        let count = tokens.token()?;
        let strings = tokens.count(count.clone())?;
        retain_push(ctx, &mut framing, count)?;
        for _ in 0..strings {
            retain_push(ctx, &mut framing, tokens.skip())?;
            tokens.token()?;
        }
        retain_push(ctx, &mut framing, tokens.skip())?;
        emit(
            ctx,
            &tokens,
            start..tokens.position,
            if side {
                "freecad.legacy_map_record"
            } else {
                "freecad.legacy_map_inline_record"
            },
            parent,
            framing,
            vec![],
            out,
        )?;
    }
    tokens.skip();
    if tokens.position != tokens.bytes().len() {
        return Err("unassigned legacy map tail".into());
    }
    Ok(())
}
