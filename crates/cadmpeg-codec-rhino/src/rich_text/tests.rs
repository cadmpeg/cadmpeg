// SPDX-License-Identifier: Apache-2.0
use super::{plain_text, Unflattened};

/// Each expected string is the `ON_TextContent::PlainTextWithFields` result
/// of `ON_TextContent::Create` with a dimension style whose stack format is
/// none.
#[test]
fn rich_text_flattens_like_the_opennurbs_run_builder() {
    for (rich, plain) in [
        ("{\\rtf1\\deff0{\\fonttbl{\\f0 Calibri;}{\\f1 Arial;}}\\f0 \\fs23\\b{\\f1\\b Revestiment \\u243?nix - Pedra HA0305}}", "Revestiment \u{f3}nix - Pedra HA0305"),
        ("{\\rtf1\\deff0{\\fonttbl{\\f0 Calibri;}}\\f0 \\fs23{\\f0\\b O\\par}{\\par}}", "O"),
        ("{\\rtf1\\deff0{\\fonttbl{\\f0 Arial;}}\\fs40{\\f0 Tolerance Information:}{\\par}{\\f0 Plate Thickness: \\u177?0.10}{\\par}{\\f0 Hole Diameter: +0.00 -0.20}\\par}", "Tolerance Information:\r\nPlate Thickness: \u{b1}0.10\r\nHole Diameter: +0.00 -0.20\r\n"),
        ("{\\rtf1\\deff0{\\fonttbl{\\f0 Arial;}}\\fs40{\\f0 \\u216?22.00 }{\\f0 [[|+0.2000|-0.0000]]}\\par}", "\u{d8}22.00 +0.2000|-0.0000"),
        ("{\\rtf1\\deff0{\\fonttbl{\\f0 Arial;}}\\fs40{\\f0 8x M4}{\\f0  \\u27785?\\u23380?}{\\f0 ,}{\\f0 \\u-28646?\\u23380?}\\par}", "8x M4 \u{6c89}\u{5b54},\u{901a}\u{5b54}"),
        ("{\\rtf1\\deff0{\\fonttbl{\\f0 Arial;}{\\f1 Gill Sans;}{\\f2 Gill Sans Ultrabold;}{\\f3 Herculanum;}}\\f0 \\fs32{\\f1\\b Hello}{\\f2  Cruel }{\\f3\\ul World}{\\f2 !}}", "Hello Cruel World!"),
        ("{\\rtf1 Hello}", "Hello"),
        ("{\\rtf1 a\\par b}", "ab"),
        ("{\\rtf1 a\\line b}", "ab"),
        ("{\\rtf1 {a}{\\line}{b}}", "a\r\nb"),
        ("{\\rtf1 {\\colortbl;\\red255\\green0\\blue0;}x}", "x"),
        ("{\\rtf1\\ansi\\ansicpg1252 caf\\'e9 \\'80\\'93x}", "caf\u{e9} \u{20ac}\u{201c}x"),
        ("{\\rtf1 x\\'4}", "x\u{fffd}"),
        ("{\\rtf1 {\\*\\fldinst HYPERLINK}visible}", "visible"),
        ("{\\rtf1 {\\*\\generator Riched20;}text}", "text"),
        ("{\\rtf1 a\\tab b}", "a        b"),
        ("{\\rtf1 \\lquote q\\rquote  \\ldblquote d\\rdblquote  \\bullet \\endash \\emdash}", "\u{2018}q\u{2019} \u{201c}d\u{201d} \u{2022}\u{2013}\u{2014}"),
        ("{\\rtf1 a\\~b\\-c\\_d}", "a bc-d"),
        ("{\\rtf1 \\u-10179?\\u-8751?}", "\u{1f5d1}"),
        ("{\\rtf1 \\u-10179?x}", "x"),
        ("{\\rtf1 \\u-8751?y}", "\u{fffd}y"),
        ("{\\rtf1 \\u128465 z}", "\u{1f5d1}z"),
        ("{\\rtf1 a\\{b\\}c\\~d}", "a{b}c d"),
        ("{\\rtf1 {a}{b/c}}", "a b/c"),
        ("{\\rtf1 %<Date(\"MMMM d, yyyy\")>% end}", "%<Date(\"MMMM d, yyyy\")>% end"),
        ("{\\rtf1 [[1|2]]}", "1|2"),
        ("{\\rtf1 \\PAR x}", "x"),
        ("{\\rtf1 \\b0 bold\\b0}", "bold"),
        ("{\\rtf1 {\\f0 x}{\\par}{\\f0 y}{\\par}}", "x\r\ny\r\n"),
        ("{\\rtf1\\fbidis{\\fonttbl{\\f0\\fnil\\fcharset0 Arial;}}\\viewkind4\\uc1\\pard\\f0\\fs20 Line one\\par Line two\\par}", "Line oneLine two"),
        ("{\\rtf1 trailing\\b}", "trailing"),
        ("{\\rtf1 \\u65535?\\u65534?}", "\u{fffd}\u{fffd}"),
        ("{\\rtf1 \\u2778?\\u233e}", "\u{ada}\u{e9}"),
        ("{\\rtf1 {\\fonttbl;}x}", "x"),
        ("{\\rtf1 \\'e9\\'ff\\'a0}", "\u{e9}\u{ff}\u{a0}"),
    ] {
        assert_eq!(plain_text(rich), Ok(plain.to_owned()), "{rich}");
    }
}

#[test]
fn style_dependent_and_code_page_constructs_stay_unflattened() {
    assert_eq!(
        plain_text("{\\rtf1 {a 1/2}}"),
        Err(Unflattened::StyleDependentFraction)
    );
    assert_eq!(
        plain_text("{\\rtf1 [[1/2]]}"),
        Err(Unflattened::StyleDependentFraction)
    );
    assert_eq!(
        plain_text("{\\rtf1{\\fonttbl{\\f0\\fcharset128 MS Gothic;}}\\f0 \\'82\\'a0}"),
        Err(Unflattened::CodePage)
    );
    assert_eq!(
        plain_text("{\\rtf1 a\\u0?b}"),
        Err(Unflattened::InvalidCodePoint)
    );
}
