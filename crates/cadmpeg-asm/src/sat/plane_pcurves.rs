// SPDX-License-Identifier: Apache-2.0
//! Length conversion of face-bound pcurves on plane faces ([`asm.md` §7.1]).
//!
//! A pcurve on a plane stores lengths along the plane's parameter
//! directions in the stream's length unit. The record grammar types a
//! face-bound `exp_par_cur` block before the face that names its support is
//! known, so this pass converts those coordinates once every record is typed.

use super::{length_cm, Record, Token};
use crate::brep::geometry::coedge_pcurve_ref;
use std::collections::BTreeSet;

/// The stream length unit of each record: `(first record, millimetres per
/// unit)` for every independently headed stream, in record order.
pub(super) type SegmentScales = [(usize, f64)];

/// Convert the BS2 coordinates of every face-bound pcurve on a plane face
/// into the binary centimetre convention. A pcurve construction that is also
/// bound to a face of another surface kind is left unconverted. The error is
/// the index of a record holding a coordinate that is not representable in
/// centimetres.
pub(super) fn convert(records: &mut [Record], scales: &SegmentScales) -> Result<(), usize> {
    let (on_plane, elsewhere) = face_bound_pcurves(records);
    let definitions = definitions(records);
    let blocks = |pcurves: &BTreeSet<usize>| -> BTreeSet<(usize, usize)> {
        pcurves
            .iter()
            .filter_map(|pcurve| exp_par_cur_block(records, &definitions, *pcurve))
            .collect()
    };
    let elsewhere = blocks(&elsewhere);
    for (owner, marker) in blocks(&on_plane).difference(&elsewhere) {
        let scale = scales
            .iter()
            .take_while(|(start, _)| start <= owner)
            .last()
            .map_or(1.0, |(_, scale)| *scale);
        let record = &mut records[*owner];
        let mut tokens = record.tokens.to_vec();
        scale_bs2_block(&mut tokens, *marker, scale).ok_or(*owner)?;
        record.tokens = tokens.into();
    }
    Ok(())
}

fn record_at(records: &[Record], reference: Option<i64>) -> Option<&Record> {
    records.get(usize::try_from(reference?).ok()?)
}

/// Pcurve record indices bound to a coedge of a plane face, and those bound
/// to a coedge of any other face.
fn face_bound_pcurves(records: &[Record]) -> (BTreeSet<usize>, BTreeSet<usize>) {
    let mut on_plane = BTreeSet::new();
    let mut elsewhere = BTreeSet::new();
    for face in records.iter().filter(|record| record.head() == "face") {
        let plane =
            record_at(records, face.ref_at(7)).is_some_and(|surface| surface.head() == "plane");
        let bound = if plane { &mut on_plane } else { &mut elsewhere };
        let mut loops = BTreeSet::new();
        let mut loop_ref = face.ref_at(4);
        while let Some(index) = loop_ref {
            let Some(lp) = record_at(records, Some(index)).filter(|lp| lp.head() == "loop") else {
                break;
            };
            if !loops.insert(index) {
                break;
            }
            let mut coedges = BTreeSet::new();
            let mut coedge_ref = lp.ref_at(4);
            while let Some(index) = coedge_ref {
                let Some(coedge) = record_at(records, Some(index))
                    .filter(|coedge| matches!(coedge.head(), "coedge" | "tcoedge"))
                else {
                    break;
                };
                if !coedges.insert(index) {
                    break;
                }
                if let Some(pcurve) =
                    coedge_pcurve_ref(coedge).and_then(|pcurve| usize::try_from(pcurve).ok())
                {
                    bound.insert(pcurve);
                }
                coedge_ref = coedge.ref_at(3);
            }
            loop_ref = lp.ref_at(3);
        }
    }
    (on_plane, elsewhere)
}

/// The record and token index of the BS2 marker of the inline or referenced
/// `exp_par_cur` construction of pcurve record `index`.
fn exp_par_cur_block(
    records: &[Record],
    definitions: &[(usize, usize)],
    index: usize,
) -> Option<(usize, usize)> {
    let pcurve = records
        .get(index)
        .filter(|record| record.head() == "pcurve")?;
    let open = pcurve
        .tokens
        .iter()
        .position(|token| matches!(token, Token::SubtypeOpen))?;
    let reference = match pcurve.tokens.get(open + 1)? {
        Token::Ident(name) | Token::SubIdent(name) if name == "ref" => {
            match pcurve.tokens.get(open + 2)? {
                Token::Long(definition) => Some(*definition),
                _ => return None,
            }
        }
        Token::Long(definition) => Some(*definition),
        _ => None,
    };
    let (owner, open) = match reference {
        Some(definition) => *definitions.get(usize::try_from(definition).ok()?)?,
        None => (index, open),
    };
    match records[owner].tokens.get(open + 1)? {
        Token::Ident(name) | Token::SubIdent(name)
            if matches!(name.as_str(), "exp_par_cur" | "exppc") =>
        {
            Some((owner, open + 2))
        }
        _ => None,
    }
}

/// The record and token index of every subtype definition, in stream order.
fn definitions(records: &[Record]) -> Vec<(usize, usize)> {
    records
        .iter()
        .enumerate()
        .flat_map(|(owner, record)| {
            record
                .tokens
                .windows(2)
                .enumerate()
                .filter_map(move |(pos, pair)| {
                    matches!(pair, [Token::SubtypeOpen, Token::Ident(name) | Token::SubIdent(name)] if name != "ref")
                        .then_some((owner, pos))
                })
        })
        .collect()
}

/// Convert the pole coordinates of the typed BS2 block at `marker`; weights
/// stay unscaled. A block the grammar did not type is left unchanged. Absent
/// when a converted coordinate is not representable.
fn scale_bs2_block(tokens: &mut [Token], marker: usize, scale: f64) -> Option<()> {
    for index in pole_coordinates(tokens, marker).unwrap_or_default() {
        if let Some(Token::Double(value)) = tokens.get_mut(index) {
            *value = length_cm(*value, scale)?;
        }
    }
    Some(())
}

/// The token indices of the pole coordinates of the BS2 block at `marker`,
/// absent unless every field has its typed form: marker, degree, closure
/// enum, knot count, `(knot, multiplicity)` pairs, and the poles.
fn pole_coordinates(tokens: &[Token], marker: usize) -> Option<Vec<usize>> {
    let rational = match tokens.get(marker)? {
        Token::Ident(name) if name == "nubs" => false,
        Token::Ident(name) if name == "nurbs" => true,
        _ => return None,
    };
    let (Token::Long(degree), Token::Enum(_), Token::Long(knots)) = (
        tokens.get(marker + 1)?,
        tokens.get(marker + 2)?,
        tokens.get(marker + 3)?,
    ) else {
        return None;
    };
    let knots = usize::try_from(*knots).ok()?;
    let first_knot = marker + 4;
    let mut multiplicity = 0i64;
    for knot in 0..knots {
        let (Token::Double(_), Token::Long(value)) = (
            tokens.get(first_knot + 2 * knot)?,
            tokens.get(first_knot + 2 * knot + 1)?,
        ) else {
            return None;
        };
        multiplicity = multiplicity.checked_add(*value)?;
    }
    let poles = usize::try_from(multiplicity.checked_sub(degree.checked_sub(1)?)?).ok()?;
    let per_pole = 2 + usize::from(rational);
    let first_pole = first_knot.checked_add(knots.checked_mul(2)?)?;
    let end = first_pole.checked_add(poles.checked_mul(per_pole)?)?;
    tokens
        .get(first_pole..end)?
        .iter()
        .all(|token| matches!(token, Token::Double(_)))
        .then(|| {
            (0..poles)
                .flat_map(|pole| [0, 1].map(|coordinate| first_pole + pole * per_pole + coordinate))
                .collect()
        })
}
