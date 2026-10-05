// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;

pub fn decode(ctx: &DecodeContext<'_>, source: &str) -> Result<(), cadmpeg_core::decode::ResourceLimit> {
    let _identity = NonBlankString::for_decode(ctx, source, "admitted identity")?;
    let _unadmitted = unadmitted_character_scan(source);
    Ok(())
}

fn unadmitted_character_scan(source: &str) -> bool {
    source.chars().any(|ch| !ch.is_whitespace()) // finding: uncharged_decode_work
}
