use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;

pub(super) fn aligned_window(area: Rectangle, bounds: Rectangle) -> Option<Rectangle> {
    let area = area.intersection(&bounds);
    let end = area.bottom_right()?;
    // CO5300 CASET/RASET require even origins and sizes; retain the BSP's stricter X alignment.
    let start = Point::new(area.top_left.x / 4 * 4, area.top_left.y / 2 * 2);
    let end = Point::new((end.x + 4) / 4 * 4 - 1, (end.y + 2) / 2 * 2 - 1);
    Some(Rectangle::with_corners(start, end))
}
