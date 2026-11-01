// SPDX-License-Identifier: Apache-2.0

use super::{integers, token, tokens, vectors3, Issues};
use crate::layers::Layers;
use crate::stage::{compose, Prim};
use crate::test_support::{prim_layer, with_context};
use crate::text::grammar::parse_layer;

/// Runs `run` over the first prim of a layer whose root body is `body`.
pub(crate) fn with_prim<T>(body: &str, run: impl FnOnce(&Prim<'_>, &mut Issues) -> T) -> T {
    with_layer(&prim_layer(body), |stage, issues| {
        run(&stage.prims[0], issues)
    })
}

/// Runs `run` over the stage of `text`.
pub(crate) fn with_layer<T>(
    text: &str,
    run: impl FnOnce(&crate::stage::Stage<'_>, &mut Issues) -> T,
) -> T {
    with_context(text.as_bytes(), |ctx| {
        let layers = Layers::single(parse_layer(ctx, text.as_bytes()).unwrap());
        let stage = compose(ctx, &layers).unwrap();
        let mut issues = Issues::default();
        run(&stage, &mut issues)
    })
}

#[test]
fn readers_return_typed_values() {
    with_prim(
        "uniform token t = \"x\"\nuniform token[] ts = [\"a\", \"b\"]\nint[] i = [1, 2]\npoint3f[] p = [(0.1, 2, 3)]",
        |prim, issues| {
            assert_eq!(token(prim, "t", issues).as_deref(), Some("x"));
            assert_eq!(tokens(prim, "ts", issues), Some(vec!["a".into(), "b".into()]));
            assert_eq!(integers(prim, "i", issues), Some(vec![1, 2]));
            assert_eq!(
                vectors3(prim, "p", issues),
                Some(vec![[f64::from(0.1_f32), 2.0, 3.0]])
            );
            assert!(issues.malformed.is_empty());
        },
    );
}

#[test]
fn a_malformed_value_is_recorded() {
    with_prim(
        "int[] i = [1.5, \"x\"]\nfloat3 v = (1, 2)",
        |prim, issues| {
            assert_eq!(integers(prim, "i", issues), None);
            assert_eq!(vectors3(prim, "v", issues), None);
            let paths: Vec<_> = issues
                .malformed
                .iter()
                .map(|(path, _)| path.as_str())
                .collect();
            assert_eq!(paths, ["/A.i", "/A.v"]);
        },
    );
}

#[test]
fn an_animated_value_is_recorded() {
    with_prim(
        "float3[] p.timeSamples = {\n 0: [(1, 2, 3)],\n}",
        |prim, issues| {
            assert_eq!(vectors3(prim, "p", issues), Some(vec![[1.0, 2.0, 3.0]]));
            assert!(issues.malformed.is_empty());
            assert!(issues.animated.contains("/A.p"));
        },
    );
}
