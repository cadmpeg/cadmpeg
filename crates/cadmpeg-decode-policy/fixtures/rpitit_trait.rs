// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

trait ItemSource {
    fn items(&self) -> impl Iterator<Item = u32>;
}

struct FixedItems;

impl ItemSource for FixedItems {
    fn items(&self) -> impl Iterator<Item = u32> {
        [1, 2].into_iter()
    }
}

fn generic_visit<S: ItemSource>(ctx: &DecodeContext<'_>, source: &S) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("fixture iterator")?;
    let _first = source.items().next();
    Ok(())
}

fn generic_return<S: ItemSource>(ctx: &DecodeContext<'_>, source: &S) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("fixture opaque return")?;
    let _items = source.items();
    Ok(())
}

pub fn decode(ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
    generic_visit(ctx, &FixedItems)?;
    generic_return(ctx, &FixedItems)
}
