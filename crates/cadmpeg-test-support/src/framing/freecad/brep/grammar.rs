// SPDX-License-Identifier: Apache-2.0
//! Counted geometry layout grammar shared by the independent carrier walks.

pub(super) trait Layout {
    fn kind(&mut self) -> Option<usize>;
    fn degree(&mut self) -> Option<usize>;
    fn count(&mut self) -> Option<usize>;
    fn signed(&mut self) -> Option<()>;
    fn reals(&mut self, n: usize) -> Option<()>;
    fn knots(&mut self, n: usize) -> Option<()>;
    fn flag(&mut self) -> Option<bool> {
        match self.kind()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
}
fn product(a: usize, b: usize) -> Option<usize> {
    a.checked_mul(b).filter(|value| *value <= 10_000_000)
}
pub(super) fn curve(c: &mut impl Layout, dimension: usize, depth: usize) -> Option<()> {
    if depth > 64 {
        return None;
    }
    match c.kind()? {
        1 => c.reals(2 * dimension),
        2 | 4 => c.reals(if dimension == 2 { 7 } else { 13 }),
        3 | 5 => c.reals(if dimension == 2 { 8 } else { 14 }),
        6 => {
            let rational = c.flag()?;
            let n = c.degree()?.checked_add(1)?;
            c.reals(product(n, dimension + usize::from(rational))?)
        }
        7 => {
            let rational = c.flag()?;
            c.flag()?;
            c.degree()?;
            let n = c.count()?;
            let k = c.count()?;
            c.reals(product(n, dimension + usize::from(rational))?)?;
            c.knots(k)
        }
        8 => {
            c.reals(2)?;
            curve(c, dimension, depth + 1)
        }
        9 => {
            c.reals(if dimension == 2 { 1 } else { 4 })?;
            curve(c, dimension, depth + 1)
        }
        _ => None,
    }
}
pub(super) fn surface(c: &mut impl Layout, depth: usize) -> Option<()> {
    if depth > 64 {
        return None;
    }
    match c.kind()? {
        1 => c.reals(12),
        2 | 4 => c.reals(13),
        3 | 5 => c.reals(14),
        6 => {
            c.reals(3)?;
            curve(c, 3, depth + 1)
        }
        7 => {
            c.reals(6)?;
            curve(c, 3, depth + 1)
        }
        8 => {
            let u = c.flag()?;
            let v = c.flag()?;
            let n = product(c.degree()?.checked_add(1)?, c.degree()?.checked_add(1)?)?;
            c.reals(product(n, 3 + usize::from(u || v))?)
        }
        9 => {
            let u = c.flag()?;
            let v = c.flag()?;
            c.flag()?;
            c.flag()?;
            c.degree()?;
            c.degree()?;
            let n = product(c.count()?, c.count()?)?;
            let uk = c.count()?;
            let vk = c.count()?;
            c.reals(product(n, 3 + usize::from(u || v))?)?;
            c.knots(uk)?;
            c.knots(vk)
        }
        10 => {
            c.reals(4)?;
            surface(c, depth + 1)
        }
        11 => {
            c.reals(1)?;
            surface(c, depth + 1)
        }
        _ => None,
    }
}
pub(super) fn location(c: &mut impl Layout) -> Option<()> {
    match c.kind()? {
        1 => c.reals(12),
        2 => {
            while c.count()? != 0 {
                c.signed()?;
            }
            Some(())
        }
        _ => None,
    }
}
