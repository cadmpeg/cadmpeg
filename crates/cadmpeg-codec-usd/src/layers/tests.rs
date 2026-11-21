// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy};

use super::load;
use crate::test_support::Files;
use crate::text::grammar::parse_layer;

const ROOT: &str = "#usda 1.0\n(\n    subLayers = [@./base.usda@, @./absent.usda@]\n)\ndef Xform \"A\" (\n    references = @./part.usda@\n)\n{\n}\n";

#[test]
fn named_layers_open_once_through_every_layer_that_names_them() {
    let files = Files(vec![
        (
            "base.usda",
            "#usda 1.0\ndef Xform \"B\" (\n    references = @./part.usda@</B>\n)\n{\n}\n"
                .as_bytes(),
        ),
        ("part.usda", "#usda 1.0\ndef Xform \"B\"\n{\n}\n".as_bytes()),
    ]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(ROOT.as_bytes(), &arena, &policy).unwrap();
    let ctx = ctx.with_companions(&files);
    let root = parse_layer(&ctx, ROOT.as_bytes()).unwrap();
    let layers = load(&ctx, root, None).unwrap();
    assert_eq!(layers.all.len(), 3);
    let base = layers.resolve(0, "./base.usda").unwrap();
    let part = layers.resolve(0, "./part.usda").unwrap();
    assert_eq!(layers.resolve(base, "./part.usda"), Some(part));
    assert_eq!(layers.stack(0), vec![0, base]);
    assert!(layers.unavailable.contains_key("./absent.usda"));
    ctx.finish_session().unwrap();
}

#[test]
fn a_layer_that_several_layers_name_is_charged_to_the_input_budget_once() {
    let root = "#usda 1.0\n(\n    subLayers = [@./a.usda@, @./b.usda@, @./c.usda@]\n)\n";
    let user = "#usda 1.0\ndef Xform \"User\" (\n    references = @./library.usda@\n)\n{\n}\n";
    let library = "#usda 1.0\ndef Xform \"Library\"\n{\n}\n";
    let files = Files(vec![
        ("a.usda", user.as_bytes()),
        ("b.usda", user.as_bytes()),
        ("c.usda", user.as_bytes()),
        ("library.usda", library.as_bytes()),
    ]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_input_bytes = u64_from_index(root.len() + 3 * user.len() + library.len());
    let (ctx, _) = DecodeContext::from_root_bytes(root.as_bytes(), &arena, &policy).unwrap();
    let ctx = ctx.with_companions(&files);
    let root = parse_layer(&ctx, root.as_bytes()).unwrap();
    let layers = load(&ctx, root, None).expect("each file is charged once");
    assert_eq!(layers.all.len(), 5);
    assert!(layers.unavailable.is_empty());
    let referrers: Vec<usize> = ["./a.usda", "./b.usda", "./c.usda"]
        .into_iter()
        .map(|asset| layers.resolve(0, asset).unwrap())
        .collect();
    let library = layers.resolve(referrers[0], "./library.usda").unwrap();
    for referrer in referrers {
        assert_eq!(layers.resolve(referrer, "./library.usda"), Some(library));
    }
    ctx.finish_session().unwrap();
}
