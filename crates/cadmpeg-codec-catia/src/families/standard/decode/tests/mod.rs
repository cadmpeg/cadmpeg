mod binding;
mod circle;
mod evidence;
mod transfer;

fn checked_circle(
    center: cadmpeg_ir::math::Point3,
    radius: f64,
) -> crate::families::standard::records::StandardCurveGeometry {
    crate::families::standard::records::StandardCurveGeometry::Circle {
        center: cadmpeg_ir::features::FinitePoint3::new(center).expect("finite circle center"),
        radius: cadmpeg_ir::scalar::PositiveLength::new(radius).expect("positive circle radius"),
    }
}
