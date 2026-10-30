// SPDX-License-Identifier: Apache-2.0

use super::{Companion, CompanionLoad, Companions};
use crate::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};

#[derive(Debug)]
struct Fixed {
    bytes: Vec<u8>,
}

impl Companions for Fixed {
    fn open(&self, anchor: Option<&str>, path: &str, max_bytes: u64) -> Result<Companion, String> {
        if crate::decode::u64_from_index(self.bytes.len()) > max_bytes {
            return Err("too long".to_owned());
        }
        Ok(Companion {
            id: format!("{}|{path}", anchor.unwrap_or("input")),
            bytes: self.bytes.clone(),
        })
    }
}

#[test]
fn a_context_without_a_source_reports_the_file_unavailable() {
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &DecodePolicy::default(), DecodeDepth::Full);
    assert!(matches!(
        ctx.open_companion(None, "other.usda"),
        Ok(CompanionLoad::Unavailable(_))
    ));
}

#[test]
fn an_opened_file_carries_its_identity_and_charges_the_input_budget() {
    let source = Fixed {
        bytes: vec![1, 2, 3],
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_input_bytes = 5;
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full).with_companions(&source);
    let Ok(CompanionLoad::Loaded { id, view }) = ctx.open_companion(Some("a"), "b") else {
        panic!("the first file fits the budget");
    };
    assert_eq!(id, "a|b");
    assert_eq!(view.window(), [1, 2, 3]);
    assert!(matches!(
        ctx.open_companion(None, "b"),
        Ok(CompanionLoad::Unavailable(_))
    ));
}

#[derive(Debug)]
struct Located;

impl Companions for Located {
    fn open(&self, _: Option<&str>, _: &str, _: u64) -> Result<Companion, String> {
        Err("not read".to_owned())
    }

    fn locate(&self, anchor: Option<&str>, path: &str) -> Result<String, String> {
        Ok(format!("{}|{path}", anchor.unwrap_or("input")))
    }
}

#[test]
fn a_located_file_carries_its_identity_and_charges_nothing() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_input_bytes = 0;
    let source = Located;
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full).with_companions(&source);
    assert_eq!(
        ctx.locate_companion(Some("omniverse://host/a.usd"), "./b.png"),
        Ok("omniverse://host/a.usd|./b.png".to_owned())
    );
    let fixed = Fixed { bytes: vec![1] };
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full).with_companions(&fixed);
    assert!(ctx.locate_companion(None, "b.png").is_err());
    let bare = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    assert!(bare.locate_companion(None, "b.png").is_err());
}
