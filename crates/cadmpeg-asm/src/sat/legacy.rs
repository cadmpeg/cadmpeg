// SPDX-License-Identifier: Apache-2.0
//! ACIS text layouts with versioned base fields and geometry tails.

use super::{take_slot, Cur, Prim, Slot, Token, TypedRecordFailure};
use cadmpeg_core::decode::DecodeContext;
use Slot::{DLen, DLenSentinel, OptB, Sense, Sides, Sub, UvSense, VLen, VUnit, B, D, L, P, R, S};

/// These subtype layouts start with the solved block, with an optional `full`
/// cache form. The remaining construction context stays lexical when its layout
/// is not represented by the modern native construction grammar.
pub(super) fn cache_tail(
    cur: &mut Cur<'_, '_, '_>,
    out: &mut Vec<Token>,
    surface: bool,
) -> Option<()> {
    if matches!(cur.peek(), Some(Prim::Word(word)) if word == "full") {
        cur.word_is("full")?;
        cur.push_token(out, Token::Enum(0));
    }
    if surface {
        super::bs_surface_block(cur, out)?;
    } else {
        super::bs_curve_block(cur, super::BsKind::Model, out)?;
    }
    let tolerance = cur.num()?;
    let tolerance = cur.length(tolerance)?;
    cur.push_token(out, Token::Double(tolerance));
    while !matches!(cur.peek()?, Prim::Close) {
        if matches!(cur.peek(), Some(Prim::Open)) {
            super::type_subtype(cur, out)?;
        } else {
            let prim = cur.bump()?;
            cur.push_lexical_token(out, prim);
        }
    }
    Some(())
}

pub(super) fn type_record(
    ctx: &DecodeContext<'_>,
    head: &str,
    prims: &[Prim],
    scale: f64,
    save_format: u32,
) -> Result<Option<Vec<Token>>, TypedRecordFailure> {
    let shapes: &[&[Slot]] = match head {
        "body" | "lump" | "loop" => &[&[R, R, R]],
        "wire" => &[&[R, R, R, R, B]],
        "shell" if save_format < 107 => &[&[R, R, R, R]],
        "shell" | "subshell" => &[&[R, R, R, R, R]],
        "face" if save_format < 105 => &[&[R, R, R, R, R, Sense]],
        "face" => &[
            &[R, R, R, R, R, Sense, Sides],
            &[R, R, R, R, R, Sense, Sides, B],
        ],
        "coedge" => &[&[R, R, R, R, Sense, R, R]],
        "tcoedge" => &[&[R, R, R, R, Sense, R, R, D, D]],
        "edge" if save_format < 500 => &[&[R, R, R, R, Sense]],
        "edge" if save_format < 600 => &[&[R, D, R, D, R, R, Sense], &[R, D, R, D, R, R, Sense, S]],
        "edge" => &[&[R, D, R, D, R, R, Sense, S]],
        "tedge" if save_format < 500 => &[&[R, R, R, R, Sense, DLen]],
        "tedge" if save_format < 600 => &[
            &[R, D, R, D, R, R, Sense, DLen],
            &[R, D, R, D, R, R, Sense, S, DLen],
        ],
        "tedge" => &[
            &[R, D, R, D, R, R, Sense, S, DLen],
            &[R, D, R, D, R, R, Sense, S, DLen, L],
            &[R, D, R, D, R, R, Sense, S, DLen, L, L],
        ],
        "vertex" => &[&[R, R]],
        "tvertex" => &[&[R, R, DLenSentinel]],
        "point" => &[&[P]],
        "transform" => &[&[VUnit, VUnit, VUnit, VLen, D, B, B, B]],
        "plane" if save_format < 103 => &[&[P, VUnit]],
        "plane" => &[&[P, VUnit, VLen, UvSense]],
        "straight" => &[&[P, VLen]],
        "ellipse" => &[&[P, VUnit, VLen, D]],
        "sphere" if save_format < 103 => &[&[P, DLen]],
        "sphere" => &[&[P, DLen, VUnit, VUnit, UvSense]],
        "torus" => &[&[P, VUnit, DLen, DLen, VUnit, UvSense]],
        "cone" if save_format < 400 => &[&[P, VUnit, VLen, D, OptB, OptB, D, D, Sense]],
        "cone" => &[&[P, VUnit, VLen, D, OptB, OptB, D, D, DLen, Sense]],
        "spline" => &[&[Sense, Sub]],
        "intcurve" => &[&[Sense, Sub]],
        "pcurve" => &[&[L, R, D, D], &[L, Sense, Sub, D, D]],
        _ => return Ok(None),
    };
    for slots in shapes {
        let mut cur = Cur {
            prims,
            pos: 0,
            scale,
            save_format,
            failure: None,
            resource: None,
            ctx,
        };
        let mut out = Vec::new();
        let matched = (|| {
            take_slot(&mut cur, R, &mut out)?;
            // No entity id or pattern reference exists in these layouts.
            // Null markers keep the downstream field positions uniform.
            cur.push_token(&mut out, Token::Long(-1));
            if head != "transform" {
                cur.push_token(&mut out, Token::Ref(-1));
            }
            for (index, slot) in slots.iter().enumerate() {
                if head == "cone" && index == 4 && save_format < 106 {
                    cur.push_token(&mut out, Token::False);
                    cur.push_token(&mut out, Token::False);
                    continue;
                }
                if head == "cone" && index == 5 && save_format < 106 {
                    continue;
                }
                if head == "cone" && save_format < 103 && index == slots.len() - 1 {
                    cur.push_token(&mut out, Token::False);
                    continue;
                }
                if matches!(head, "coedge" | "tcoedge")
                    && matches!(slot, Sense)
                    && matches!(cur.peek(), Some(Prim::Integer(_)))
                {
                    let flag = cur.long()?;
                    cur.push_token(
                        &mut out,
                        match flag {
                            0 => Token::False,
                            1 => Token::True,
                            _ => return None,
                        },
                    );
                } else if matches!(head, "edge" | "tedge")
                    && matches!(slot, S)
                    && matches!(cur.peek(), Some(Prim::Word(_)))
                {
                    let text = cur.word()?;
                    cur.push_text_token(&mut out, text, true);
                } else {
                    take_slot(&mut cur, *slot, &mut out)?;
                }
                if matches!(head, "edge" | "tedge") && save_format < 500 && index < 2 {
                    // These saves carry no endpoint parameters. An absent
                    // marker is not a numeric parameter estimate.
                    cur.push_token(&mut out, Token::False);
                }
                if head == "shell" && save_format < 107 && index == 2 {
                    cur.push_token(&mut out, Token::Ref(-1));
                }
            }
            if head == "face" && save_format < 105 {
                cur.push_token(&mut out, Token::False);
            }
            if head == "plane" && save_format < 103 {
                // The plane stores no chart axis. Select a perpendicular
                // reference for the neutral carrier from its normal.
                let Some(Token::Vector3(normal)) = out.last() else {
                    return None;
                };
                let [x, y, z] = *normal;
                let reference = if x.abs() <= y.abs() && x.abs() <= z.abs() {
                    [0.0, z, -y]
                } else if y.abs() <= z.abs() {
                    [-z, 0.0, x]
                } else {
                    [y, -x, 0.0]
                };
                cur.push_token(&mut out, Token::Vector3(reference));
                cur.push_token(&mut out, Token::False);
            }
            if head == "sphere" && save_format < 103 {
                cur.push_token(&mut out, Token::Vector3([1.0, 0.0, 0.0]));
                cur.push_token(&mut out, Token::Vector3([0.0, 0.0, 1.0]));
                cur.push_token(&mut out, Token::False);
            }
            let bounds = match head {
                "straight" | "ellipse" | "intcurve" => 2,
                "plane" | "sphere" | "torus" | "cone" | "spline" => 4,
                _ => 0,
            };
            for _ in 0..bounds {
                if save_format < 106 {
                    cur.push_token(&mut out, Token::False);
                } else {
                    cur.opt_bound(&mut out)?;
                }
            }
            cur.done().then_some(())
        })()
        .is_some();
        match (cur.resource, cur.failure) {
            (Some(error), _) => return Err(TypedRecordFailure::Resource(error)),
            (_, Some(error)) => return Err(TypedRecordFailure::Type(error)),
            _ if matched => return Ok(Some(out)),
            _ => {}
        }
    }
    Ok(None)
}

/// Read the ACIS base extension separately from the shared entity fields.
/// Later ACIS records also carry class-specific tails after those fields.
pub(super) fn type_extended_record(
    ctx: &DecodeContext<'_>,
    head: &str,
    prims: &[Prim],
    scale: f64,
    save_format: u32,
) -> Result<Option<Vec<Token>>, TypedRecordFailure> {
    if !matches!(prims.get(2), Some(Prim::Integer(_)))
        || !matches!(prims.get(3), Some(Prim::Ref(_)))
    {
        return Ok(None);
    }
    for slots in super::head_shapes(head) {
        let mut cur = Cur {
            prims,
            pos: 0,
            scale,
            save_format,
            failure: None,
            resource: None,
            ctx,
        };
        let mut out = Vec::new();
        let matched = (|| {
            take_slot(&mut cur, R, &mut out)?;
            take_slot(&mut cur, L, &mut out)?;
            cur.long()?;
            take_slot(&mut cur, R, &mut out)?;
            for slot in slots.get(3..)? {
                if matches!(head, "edge" | "tedge")
                    && matches!(slot, S)
                    && matches!(cur.peek(), Some(Prim::Word(_)))
                {
                    let text = cur.word()?;
                    cur.push_text_token(&mut out, text, true);
                } else {
                    take_slot(&mut cur, *slot, &mut out)?;
                }
            }
            while let Some(prim) = cur.bump() {
                cur.push_lexical_token(&mut out, prim);
            }
            Some(())
        })()
        .is_some();
        match (cur.resource, cur.failure) {
            (Some(error), _) => return Err(TypedRecordFailure::Resource(error)),
            (_, Some(error)) => return Err(TypedRecordFailure::Type(error)),
            _ if matched => return Ok(Some(out)),
            _ => {}
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
