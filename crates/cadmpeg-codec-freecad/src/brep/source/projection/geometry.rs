// SPDX-License-Identifier: Apache-2.0
//! Bounded original-row getters for actual neutral geometry producers.
use super::*;

#[derive(Clone, Copy)]
enum Root {
    Curve,
    Surface,
}
#[derive(Clone, Copy)]
enum Step {
    Basis,
    Directrix,
}
#[derive(Clone)]
pub(crate) struct GeometrySource<'a> {
    view: View<'a>,
    binary: bool,
    root: Root,
    index: usize,
    steps: [Step; MAX_GEOMETRY_NESTING_DEPTH],
    length: usize,
}
enum Value {
    Curve(TextCurve),
    Surface(TextSurface),
}
impl<'a> GeometrySource<'a> {
    fn new(view: View<'a>, binary: bool, index: usize, root: Root) -> Self {
        Self {
            view,
            binary,
            index,
            root,
            steps: [Step::Basis; MAX_GEOMETRY_NESTING_DEPTH],
            length: 0,
        }
    }
    fn child(&self, step: Step) -> Result<Self, CodecError> {
        let mut child = self.clone();
        let slot = child
            .steps
            .get_mut(child.length)
            .ok_or_else(|| CodecError::malformed("geometry source route exceeds nesting limit"))?;
        *slot = step;
        child.length += 1;
        Ok(child)
    }
    pub(crate) fn basis(&self) -> Result<Self, CodecError> {
        self.child(Step::Basis)
    }
    pub(crate) fn directrix(&self) -> Result<Self, CodecError> {
        self.child(Step::Directrix)
    }
    pub(crate) fn read_curve(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
    ) -> Result<TextCurve, CodecError> {
        ctx.record(id, self.view, || match self.value()? {
            Value::Curve(value) => Ok(value),
            Value::Surface(_) => Err(CodecError::malformed(
                "curve producer requires a curve source branch",
            )),
        })
    }
    pub(crate) fn read_surface(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
    ) -> Result<TextSurface, CodecError> {
        ctx.record(id, self.view, || match self.value()? {
            Value::Surface(value) => Ok(value),
            Value::Curve(_) => Err(CodecError::malformed(
                "surface producer requires a surface source branch",
            )),
        })
    }
    fn value(&self) -> Result<Value, CodecError> {
        let mut value = if self.binary {
            let mut cursor = BinaryCursor {
                view: self.view,
                census_marks: vec![],
                representation_marks: vec![],
            };
            match self.root {
                Root::Curve => Value::Curve(parse_binary_curve(&mut cursor, 0)?),
                Root::Surface => Value::Surface(parse_binary_surface(&mut cursor, 0)?),
            }
        } else {
            let (tokens, ranges) = text_tokens(self.view);
            let mut cursor = cursor(&tokens, &ranges, self.view);
            match self.root {
                Root::Curve => Value::Curve(parse_curve(&mut cursor, 0, self.index)?),
                Root::Surface => Value::Surface(parse_surface(&mut cursor, 0, self.index)?),
            }
        };
        for step in &self.steps[..self.length] {
            value = match (step, value) {
                (
                    Step::Basis,
                    Value::Curve(
                        TextCurve::Trimmed { basis, .. } | TextCurve::Offset { basis, .. },
                    ),
                ) => Value::Curve(*basis.curve),
                (
                    Step::Basis,
                    Value::Surface(
                        TextSurface::Trimmed { basis, .. } | TextSurface::Offset { basis, .. },
                    ),
                ) => Value::Surface(*basis.surface),
                (
                    Step::Directrix,
                    Value::Surface(
                        TextSurface::Extrusion { directrix, .. }
                        | TextSurface::Revolution { directrix, .. },
                    ),
                ) => Value::Curve(*directrix.curve),
                _ => {
                    return Err(CodecError::malformed(
                        "geometry source route differs from its typed branch",
                    ))
                }
            };
        }
        Ok(value)
    }
}
impl ProjectionSources<'_> {
    pub(crate) fn curve(
        &self,
        payload: &ShapePayloadRecord,
        dense: usize,
    ) -> Result<GeometrySource<'_>, CodecError> {
        self.geometry(payload, 2, dense, Root::Curve)
    }
    pub(crate) fn surface(
        &self,
        payload: &ShapePayloadRecord,
        dense: usize,
    ) -> Result<GeometrySource<'_>, CodecError> {
        self.geometry(payload, 5, dense, Root::Surface)
    }
    fn geometry(
        &self,
        payload: &ShapePayloadRecord,
        table: u8,
        dense: usize,
        root: Root,
    ) -> Result<GeometrySource<'_>, CodecError> {
        let source = self.payloads.get(&payload.id).ok_or_else(|| {
            CodecError::malformed("geometry payload has no original source authority")
        })?;
        let index = payload.source_index(table, dense);
        Ok(GeometrySource::new(
            source.row(table, index)?,
            source.binary,
            index,
            root,
        ))
    }
}
