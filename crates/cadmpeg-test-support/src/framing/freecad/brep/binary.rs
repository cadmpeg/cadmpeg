// SPDX-License-Identifier: Apache-2.0
//! Binary little-endian boundary grammar; failed rows stop the schedule.
use super::grammar::{self, Layout};
use super::*;
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let value = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(value)
    }
    fn i32(&mut self) -> Option<i32> {
        View::i32_le_at(self.take(4)?, 0)
    }
    fn line(&mut self) -> Option<&'a [u8]> {
        loop {
            let width = self
                .bytes
                .get(self.at..)?
                .iter()
                .position(|byte| *byte == b'\n')?
                + 1;
            let line = self.take(width)?;
            if !line.iter().all(u8::is_ascii_whitespace) {
                return Some(line);
            }
        }
    }
}
impl Layout for Cursor<'_> {
    fn kind(&mut self) -> Option<usize> {
        Some(usize::from(*self.take(1)?.first()?))
    }
    fn degree(&mut self) -> Option<usize> {
        Some(usize::from(View::u16_le_at(self.take(2)?, 0)?))
    }
    fn count(&mut self) -> Option<usize> {
        usize::try_from(self.i32()?)
            .ok()
            .filter(|value| *value <= 1_000_000)
    }
    fn signed(&mut self) -> Option<()> {
        self.i32().map(|_| ())
    }
    fn reals(&mut self, n: usize) -> Option<()> {
        for _ in 0..n {
            let value = View::f64_le_at(self.take(8)?, 0)?;
            if !value.is_finite() {
                return None;
            }
        }
        Some(())
    }
    fn knots(&mut self, n: usize) -> Option<()> {
        for _ in 0..n {
            self.reals(1)?;
            self.count()?;
        }
        Some(())
    }
}
pub(super) fn frame<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    root: usize,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    let Some((_, end)) = header(source.window()) else {
        return Ok(());
    };
    let prefix = b"Open CASCADE Topology V";
    let (_, header_end) = header(source.window()).ok_or("missing B-rep header")?;
    let mut c = Cursor {
        bytes: source.window(),
        at: 0,
    };
    let line = c.line();
    let version = line
        .and_then(|line| line.strip_prefix(prefix.as_slice()))
        .and_then(|tail| tail.first())
        .and_then(|byte| byte.checked_sub(b'0'));
    let mut framing = Vec::new();
    let mut dispatch = Vec::new();
    if source.window().get(end - 1) == Some(&b'\n') {
        framing.push(range(source.start() + end - 1, source.start() + end));
    }
    if let Some(line) = line {
        if version.is_some() {
            let digit = source.start() + header_end - line.len() + prefix.len();
            framing.push(range(digit, digit + 1));
            dispatch.push(range(digit, digit + 1));
            framing.sort_by_key(|r| r.start);
        }
    }
    add(
        ctx,
        source,
        source.start(),
        source.start() + end,
        "freecad.brep_binary_header",
        root,
        framing,
        dispatch,
        out,
    )?;
    let Some(version @ 1..=4) = version else {
        return Ok(());
    };
    for (table, name) in TABLES.iter().enumerate() {
        for line in c.bytes[c.at..].split_inclusive(|byte| *byte == b'\n') {
            if !line.iter().all(u8::is_ascii_whitespace) {
                break;
            }
            c.at =
                c.at.checked_add(line.len())
                    .ok_or("binary marker offset overflows")?;
        }
        let marker_start = c.at;
        let count = c.line().and_then(|line| {
            let mut words = std::str::from_utf8(line).ok()?.split_ascii_whitespace();
            if words.next().map(str::as_bytes) != Some(*name) {
                return None;
            }
            let count = words
                .next()?
                .parse::<usize>()
                .ok()
                .filter(|n| *n <= 1_000_000)?;
            words.next().is_none().then_some(count)
        });
        let marker_end = c.at;
        let end = if count.is_some() {
            marker_end
        } else {
            source.window().len()
        };
        let dispatch = (marker_start < marker_end)
            .then(|| range(source.start() + marker_start, source.start() + marker_end))
            .into_iter()
            .collect();
        add(
            ctx,
            source,
            source.start() + marker_start,
            source.start() + end,
            "freecad.brep_binary_record",
            root,
            vec![],
            dispatch,
            out,
        )?;
        let Some(count) = count else { return Ok(()) };
        for index in 1..=count {
            let start = c.at;
            let success = row(&mut c, table, version, index, count).is_some();
            let end = if success { c.at } else { source.window().len() };
            let mut dispatch = vec![range(
                source.start() + marker_start,
                source.start() + marker_end,
            )];
            if start < end && matches!(table, 0 | 1 | 2 | 5 | 7) {
                dispatch.push(range(source.start() + start, source.start() + start + 1));
            }
            // The selector line is external to the record's source extent.
            let external = dispatch.remove(0);
            let entry = add(
                ctx,
                source,
                source.start() + start,
                source.start() + end,
                "freecad.brep_binary_record",
                root,
                vec![],
                dispatch,
                out,
            )?;
            let selector = source
                .child(
                    usize::try_from(external.start).map_err(|e| e.to_string())?,
                    usize::try_from(external.end).map_err(|e| e.to_string())?,
                )
                .ok_or("selector escapes source")?;
            let frame = out
                .pop()
                .ok_or("missing record frame")?
                .with_external_dispatch(ctx, vec![selector])?;
            if entry != out.len() {
                return Err("B-rep frame order changed".into());
            }
            out.push(frame);
            if !success {
                return Ok(());
            }
        }
    }
    let remaining = source
        .window()
        .len()
        .checked_sub(c.at)
        .ok_or("binary B-rep cursor exceeds its member")?;
    if (1..=12).contains(&remaining) {
        add(
            ctx,
            source,
            source.start() + c.at,
            source.end(),
            "freecad.brep_binary_root",
            root,
            vec![],
            vec![],
            out,
        )?;
    }
    Ok(())
}
fn row(c: &mut Cursor<'_>, table: usize, version: u8, index: usize, count: usize) -> Option<()> {
    match table {
        0 => grammar::location(c),
        1 => grammar::curve(c, 2, 0),
        2 => grammar::curve(c, 3, 0),
        5 => grammar::surface(c, 0),
        3 => {
            let n = c.count()?;
            let parameters = c.flag()?;
            c.reals(1 + n.checked_mul(3 + usize::from(parameters))?)
        }
        4 => {
            let n = c.count()?;
            for _ in 0..n {
                c.count()?;
            }
            c.reals(1)?;
            if c.flag()? {
                c.reals(n)?;
            }
            Some(())
        }
        6 => {
            let n = c.count()?;
            let t = c.count()?;
            let uv = c.flag()?;
            let normals = version >= 4 && c.flag()?;
            c.reals(1 + n.checked_mul(3 + 2 * usize::from(uv))?)?;
            for _ in 0..t.checked_mul(3)? {
                c.count()?;
            }
            if normals {
                let bytes = c.take(n.checked_mul(12)?)?;
                for value in bytes.chunks_exact(4) {
                    if !View::f32_le_at(value, 0)?.is_finite() {
                        return None;
                    }
                }
            }
            Some(())
        }
        7 => tshape(c, version, index, count),
        _ => None,
    }
}
fn tshape(c: &mut Cursor<'_>, version: u8, index: usize, count: usize) -> Option<()> {
    match c.kind()? {
        0..=3 | 5 => {}
        4 => {
            c.flag()?;
            c.reals(1)?;
            c.count()?;
            c.count()?;
            match c.kind()? {
                0 | 1 => {}
                2 => {
                    c.count()?;
                }
                _ => return None,
            }
        }
        6 => {
            c.reals(1)?;
            c.flag()?;
            c.flag()?;
            c.flag()?;
            loop {
                match c.kind()? {
                    0 => break,
                    1 => {
                        c.count()?;
                        c.count()?;
                        c.reals(2)?;
                    }
                    2 | 3 => {
                        let kind = usize::from(*c.bytes.get(c.at - 1)?);
                        c.count()?;
                        if kind == 3 {
                            c.count()?;
                            c.kind()?;
                        }
                        c.count()?;
                        c.count()?;
                        c.reals(2)?;
                        if matches!(version, 2 | 3) {
                            c.reals(4)?;
                        }
                    }
                    4 => {
                        c.kind()?;
                        for _ in 0..4 {
                            c.count()?;
                        }
                    }
                    5 => {
                        c.count()?;
                        c.count()?;
                    }
                    6 => {
                        for _ in 0..3 {
                            c.count()?;
                        }
                    }
                    7 => {
                        for _ in 0..4 {
                            c.count()?;
                        }
                    }
                    _ => return None,
                }
            }
        }
        7 => {
            c.reals(4)?;
            loop {
                match c.kind()? {
                    0 => break,
                    1 => {
                        c.reals(1)?;
                        c.count()?;
                        c.count()?;
                    }
                    2 => {
                        c.reals(1)?;
                        for _ in 0..3 {
                            c.count()?;
                        }
                    }
                    3 => {
                        c.reals(2)?;
                        c.count()?;
                        c.count()?;
                    }
                    _ => return None,
                }
            }
        }
        _ => return None,
    }
    for _ in 0..7 {
        c.flag()?;
    }
    loop {
        match c.kind()? {
            42 => break,
            0..=3 => {
                // Reverse child indices must name an already admitted shape
                // in this actual N-record table. A failed reference cannot
                // borrow a subsequent record's '*'.
                let reverse = c.count()?;
                if !(1..=count).contains(&reverse) || count - reverse + 1 >= index {
                    return None;
                }
                c.count()?;
            }
            _ => return None,
        }
    }
    Some(())
}
