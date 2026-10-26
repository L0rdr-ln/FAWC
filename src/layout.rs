//! Window frame geometry and hit testing. All in logical pixels.

use smithay::utils::{Logical, Point, Rectangle};

use crate::glass::GlassStyle;
use crate::state::Edges;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Close,
    Maximize,
    Minimize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    /// over the client's own pixels
    Client,
    Titlebar,
    Button(Button),
    /// on the border or a corner: the edges a resize from here would move
    Border(Edges),
}

/// The window with its frame, from the client's geometry.
pub fn outer_rect(client: Rectangle<i32, Logical>, style: &GlassStyle) -> Rectangle<i32, Logical> {
    let b = style.border as i32;
    let t = style.titlebar as i32;
    Rectangle::new(
        (client.loc.x - b, client.loc.y - b - t).into(),
        (client.size.w + 2 * b, client.size.h + 2 * b + t).into(),
    )
}

/// The client's geometry from the outer rectangle.
pub fn client_rect(outer: Rectangle<i32, Logical>, style: &GlassStyle) -> Rectangle<i32, Logical> {
    let b = style.border as i32;
    let t = style.titlebar as i32;
    Rectangle::new(
        (outer.loc.x + b, outer.loc.y + b + t).into(),
        ((outer.size.w - 2 * b).max(1), (outer.size.h - 2 * b - t).max(1)).into(),
    )
}

/// Centre of a button in the same coordinates as `outer`.
pub fn button_center(outer: Rectangle<i32, Logical>, style: &GlassStyle, b: Button) -> Point<f64, Logical> {
    let slot = match b {
        Button::Close => 0.0,
        Button::Maximize => 1.0,
        Button::Minimize => 2.0,
    };
    let x = outer.loc.x as f64 + outer.size.w as f64 - style.border as f64 - style.orb_margin as f64
        - style.orb_size as f64 * 0.5
        - slot * (style.orb_size + style.orb_gap) as f64;
    let y = outer.loc.y as f64 + style.border as f64 + style.titlebar as f64 * 0.5;
    (x, y).into()
}

pub fn hit(client: Rectangle<i32, Logical>, style: &GlassStyle, p: Point<f64, Logical>) -> Option<Hit> {
    let outer = outer_rect(client, style);
    let grip = (style.border as f64).max(6.0);
    let (x0, y0) = (outer.loc.x as f64, outer.loc.y as f64);
    let (x1, y1) = (x0 + outer.size.w as f64, y0 + outer.size.h as f64);
    // a little outside the frame is still the resize grip
    if p.x < x0 - 3.0 || p.x >= x1 + 3.0 || p.y < y0 - 3.0 || p.y >= y1 + 3.0 {
        return None;
    }
    let c = Rectangle::<f64, Logical>::new(
        (client.loc.x as f64, client.loc.y as f64).into(),
        (client.size.w as f64, client.size.h as f64).into(),
    );
    if p.x >= c.loc.x && p.x < c.loc.x + c.size.w && p.y >= c.loc.y && p.y < c.loc.y + c.size.h {
        return Some(Hit::Client);
    }
    for b in [Button::Close, Button::Maximize, Button::Minimize] {
        let ctr = button_center(outer, style, b);
        let r = style.orb_size as f64 * 0.5 + 2.0;
        if (p.x - ctr.x).powi(2) + (p.y - ctr.y).powi(2) <= r * r {
            return Some(Hit::Button(b));
        }
    }
    let edges = Edges {
        left: p.x < x0 + grip,
        right: p.x >= x1 - grip,
        top: p.y < y0 + grip.min(5.0),
        bottom: p.y >= y1 - grip,
    };
    if edges.left || edges.right || edges.top || edges.bottom {
        return Some(Hit::Border(edges));
    }
    Some(Hit::Titlebar)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip_and_hits() {
        let style = GlassStyle::default();
        let client = Rectangle::<i32, Logical>::new((100, 100).into(), (400, 300).into());
        let outer = outer_rect(client, &style);
        assert_eq!(client_rect(outer, &style), client);
        assert_eq!(hit(client, &style, (300.0, 200.0).into()), Some(Hit::Client));
        // the title bar is above the client
        assert_eq!(hit(client, &style, (150.0, 85.0).into()), Some(Hit::Titlebar));
        // close button: the right-most orb
        let c = button_center(outer, &style, Button::Close);
        assert_eq!(hit(client, &style, c), Some(Hit::Button(Button::Close)));
        let m = button_center(outer, &style, Button::Maximize);
        assert!(m.x < c.x);
        assert_eq!(hit(client, &style, m), Some(Hit::Button(Button::Maximize)));
        // the left border is a resize grip for the left edge only
        match hit(client, &style, (outer.loc.x as f64 + 1.0, 200.0).into()) {
            Some(Hit::Border(e)) => assert!(e.left && !e.right && !e.top && !e.bottom),
            other => panic!("{other:?}"),
        }
        // the bottom right corner moves two edges
        match hit(client, &style, (outer.loc.x as f64 + outer.size.w as f64 - 1.0, outer.loc.y as f64 + outer.size.h as f64 - 1.0).into()) {
            Some(Hit::Border(e)) => assert!(e.right && e.bottom && !e.left && !e.top),
            other => panic!("{other:?}"),
        }
        assert_eq!(hit(client, &style, (10.0, 10.0).into()), None);
    }
}
