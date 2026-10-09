// SPDX-License-Identifier: Apache-2.0
//! Caller depth ceilings for actual recursive native geometry parser bodies.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::{
    parse_binary_curve, parse_binary_curve2d, parse_binary_surface, parse_curve, parse_curve2d,
    parse_surface, BinaryCursor, TokenCursor,
};

#[derive(Clone, Copy)]
enum Parser {
    TextCurve2d,
    TextCurve,
    TextSurface,
    BinaryCurve2d,
    BinaryCurve,
    BinarySurface,
    TextDirectrix,
    BinaryDirectrix,
}

fn parse(ctx: &DecodeContext<'_>, parser: Parser, nested: bool) -> Result<(), CodecError> {
    let line2d = ["1", "0", "0", "1", "0"];
    let line3d = ["1", "0", "0", "0", "0", "0", "1"];
    let plane = ["1", "0", "0", "0", "0", "0", "1", "1", "0", "0", "0", "1", "0"];
    match parser {
        Parser::TextCurve2d | Parser::TextCurve | Parser::TextSurface | Parser::TextDirectrix => {
            let mut tokens = Vec::new();
            match parser {
                Parser::TextCurve2d => {
                    if nested { tokens.extend(["9", "1"]); }
                    tokens.extend(line2d);
                }
                Parser::TextCurve => {
                    if nested { tokens.extend(["8", "0", "1"]); }
                    tokens.extend(line3d);
                }
                Parser::TextSurface => {
                    if nested { tokens.extend(["11", "1"]); }
                    tokens.extend(plane);
                }
                Parser::TextDirectrix => {
                    if nested {
                        tokens.extend(["6", "0", "0", "1"]);
                        tokens.extend(line3d);
                    } else { tokens.extend(plane); }
                }
                _ => unreachable!("selected text parser"),
            }
            let mut cursor = TokenCursor::new(ctx, &tokens);
            match parser {
                Parser::TextCurve2d => parse_curve2d(&mut cursor, 0, 1).map(drop),
                Parser::TextCurve => parse_curve(&mut cursor, 0, 1).map(drop),
                _ => parse_surface(&mut cursor, 0, 1).map(drop),
            }?;
            assert!(cursor.is_empty(), "all real text fixture tokens consumed");
            Ok(())
        }
        _ => {
            let mut bytes = Vec::new();
            let coordinates: &[f64] = match parser {
                Parser::BinaryCurve2d => {
                    if nested {
                        bytes.push(9);
                        bytes.extend_from_slice(&1.0_f64.to_le_bytes());
                    }
                    bytes.push(1);
                    &[0.0, 0.0, 1.0, 0.0]
                }
                Parser::BinaryCurve => {
                    if nested {
                        bytes.push(8);
                        bytes.extend_from_slice(&0.0_f64.to_le_bytes());
                        bytes.extend_from_slice(&1.0_f64.to_le_bytes());
                    }
                    bytes.push(1);
                    &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0]
                }
                Parser::BinaryDirectrix if nested => {
                    bytes.push(6);
                    for coordinate in [0.0_f64, 0.0, 1.0] {
                        bytes.extend_from_slice(&coordinate.to_le_bytes());
                    }
                    bytes.push(1);
                    &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0]
                }
                _ => {
                    if nested {
                        bytes.push(11);
                        bytes.extend_from_slice(&1.0_f64.to_le_bytes());
                    }
                    bytes.push(1);
                    &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
                }
            };
            for coordinate in coordinates {
                bytes.extend_from_slice(&coordinate.to_le_bytes());
            }
            let mut cursor = BinaryCursor::new(ctx, &bytes);
            match parser {
                Parser::BinaryCurve2d => parse_binary_curve2d(&mut cursor, 0).map(drop),
                Parser::BinaryCurve => parse_binary_curve(&mut cursor, 0).map(drop),
                _ => parse_binary_surface(&mut cursor, 0).map(drop),
            }?;
            assert_eq!(cursor.remaining(), 0, "all real binary fixture bytes consumed");
            Ok(())
        }
    }
}

fn assert_depth(parser: Parser, root_operation: &'static str, nested_operation: &'static str) {
    for cap in [0, 1, 2] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        if cap != 0 {
            parse(&ctx, parser, false).expect("one actual parser frame");
            let root = ctx.enter_nested("reuse completed parser depth").expect("root depth retired");
            drop(root);
        }
        if cap == 2 {
            parse(&ctx, parser, true).expect("two actual parser frames");
            let root = ctx.enter_nested("reuse completed nested parser root").expect("root retired");
            let nested = ctx.enter_nested("reuse completed nested parser child").expect("child retired");
            drop((nested, root));
            assert_eq!(ctx.resource_refusal(), None);
            continue;
        }
        let CodecError::ResourceLimit(original) = parse(&ctx, parser, cap == 1)
            .expect_err("caller depth ceiling refuses actual parser frame") else {
                panic!("recursion refusal");
            };
        assert_eq!(original.dimension, ResourceDimension::RecursionDepth);
        assert_eq!(original.limit, cap);
        assert_eq!(original.used, cap);
        assert_eq!(original.additional, 1);
        assert_eq!(original.operation, if cap == 0 { root_operation } else { nested_operation });
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(matches!(parse(&ctx, parser, false),
            Err(CodecError::ResourceLimit(repeated)) if repeated == original));
        assert!(matches!(ctx.enter_nested("refused parser depth reentry"),
            Err(CodecError::ResourceLimit(repeated)) if repeated == original));
    }
}

#[test]
fn text_parameter_curve_parser_uses_the_original_caller_depth() {
    assert_depth(Parser::TextCurve2d, "FreeCAD text parameter-curve parse nesting",
        "FreeCAD text parameter-curve parse nesting");
}

#[test]
fn text_curve_parser_uses_the_original_caller_depth() {
    assert_depth(Parser::TextCurve, "FreeCAD text curve parse nesting",
        "FreeCAD text curve parse nesting");
}

#[test]
fn text_surface_parser_uses_the_original_caller_depth() {
    assert_depth(Parser::TextSurface, "FreeCAD text surface parse nesting",
        "FreeCAD text surface parse nesting");
}

#[test]
fn binary_parameter_curve_parser_uses_the_original_caller_depth() {
    assert_depth(Parser::BinaryCurve2d, "FreeCAD binary parameter-curve parse nesting",
        "FreeCAD binary parameter-curve parse nesting");
}

#[test]
fn binary_curve_parser_uses_the_original_caller_depth() {
    assert_depth(Parser::BinaryCurve, "FreeCAD binary curve parse nesting",
        "FreeCAD binary curve parse nesting");
}

#[test]
fn binary_surface_parser_uses_the_original_caller_depth() {
    assert_depth(Parser::BinarySurface, "FreeCAD binary surface parse nesting",
        "FreeCAD binary surface parse nesting");
}

#[test]
fn text_surface_and_directrix_share_the_original_caller_depth() {
    assert_depth(Parser::TextDirectrix, "FreeCAD text surface parse nesting",
        "FreeCAD text curve parse nesting");
}

#[test]
fn binary_surface_and_directrix_share_the_original_caller_depth() {
    assert_depth(Parser::BinaryDirectrix, "FreeCAD binary surface parse nesting",
        "FreeCAD binary curve parse nesting");
}
