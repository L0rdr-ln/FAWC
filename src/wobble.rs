//! Wobbly windows: while a window is dragged its picture hangs on a grid of springs, so it
//! bends and swings and then settles.

use smithay::utils::{Logical, Rectangle};

/// Points per side of the grid (the window is drawn as `(N - 1)^2` cells).
pub const N: usize = 25;

const NEIGHBOUR_K: f64 = 1300.0;
const ANCHOR_K: f64 = 55.0;
const DAMPING: f64 = 7.5;
const SUBSTEP: f64 = 1.0 / 240.0;

#[derive(Debug, Clone)]
pub struct Wobble {
    pub pos: [[(f64, f64); N]; N],
    vel: [[(f64, f64); N]; N],
    /// the node the pointer holds
    pub grab: Option<(usize, usize)>,
    rest: Rectangle<f64, Logical>,
}

impl Wobble {
    pub fn new(rest: Rectangle<f64, Logical>) -> Wobble {
        let mut w = Wobble { pos: [[(0.0, 0.0); N]; N], vel: [[(0.0, 0.0); N]; N], grab: None, rest };
        for i in 0..N {
            for j in 0..N {
                w.pos[i][j] = w.rest_of(i, j);
            }
        }
        w
    }

    fn rest_of(&self, i: usize, j: usize) -> (f64, f64) {
        let f = (N - 1) as f64;
        (self.rest.loc.x + self.rest.size.w * j as f64 / f, self.rest.loc.y + self.rest.size.h * i as f64 / f)
    }

    /// The node nearest to `p` (logical coordinates).
    pub fn nearest(&self, p: (f64, f64)) -> (usize, usize) {
        let f = (N - 1) as f64;
        let j = (((p.0 - self.rest.loc.x) / self.rest.size.w.max(1.0)) * f).round().clamp(0.0, f) as usize;
        let i = (((p.1 - self.rest.loc.y) / self.rest.size.h.max(1.0)) * f).round().clamp(0.0, f) as usize;
        (i, j)
    }

    /// Advance by `dt` seconds with the window now at `rest` (it moves when it is dragged).
    pub fn step(&mut self, dt: f64, rest: Rectangle<f64, Logical>) {
        // a resized window just snaps: the grid follows the new size
        let resized = (rest.size.w - self.rest.size.w).abs() > 0.5 || (rest.size.h - self.rest.size.h).abs() > 0.5;
        self.rest = rest;
        if resized {
            for i in 0..N {
                for j in 0..N {
                    self.pos[i][j] = self.rest_of(i, j);
                    self.vel[i][j] = (0.0, 0.0);
                }
            }
            return;
        }
        let mut left = dt.min(0.05);
        while left > 1e-6 {
            let h = left.min(SUBSTEP);
            left -= h;
            self.substep(h);
        }
    }

    fn substep(&mut self, h: f64) {
        let mut acc = [[(0.0f64, 0.0f64); N]; N];
        for i in 0..N {
            for j in 0..N {
                let (px, py) = self.pos[i][j];
                let (rx, ry) = self.rest_of(i, j);
                let mut ax = ANCHOR_K * (rx - px) - DAMPING * self.vel[i][j].0;
                let mut ay = ANCHOR_K * (ry - py) - DAMPING * self.vel[i][j].1;
                for (di, dj) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    let (ni, nj) = (i as i32 + di, j as i32 + dj);
                    if ni < 0 || nj < 0 || ni >= N as i32 || nj >= N as i32 {
                        continue;
                    }
                    let (ni, nj) = (ni as usize, nj as usize);
                    let (qx, qy) = self.pos[ni][nj];
                    let (nrx, nry) = self.rest_of(ni, nj);
                    ax += NEIGHBOUR_K * ((qx - px) - (nrx - rx));
                    ay += NEIGHBOUR_K * ((qy - py) - (nry - ry));
                }
                acc[i][j] = (ax, ay);
            }
        }
        for i in 0..N {
            for j in 0..N {
                self.vel[i][j].0 += acc[i][j].0 * h;
                self.vel[i][j].1 += acc[i][j].1 * h;
                self.pos[i][j].0 += self.vel[i][j].0 * h;
                self.pos[i][j].1 += self.vel[i][j].1 * h;
            }
        }
        if let Some((i, j)) = self.grab {
            self.pos[i][j] = self.rest_of(i, j);
            self.vel[i][j] = (0.0, 0.0);
        }
    }

    /// The largest distance of a node from where it rests, in logical pixels.
    pub fn displacement(&self) -> f64 {
        let mut m: f64 = 0.0;
        for i in 0..N {
            for j in 0..N {
                let (rx, ry) = self.rest_of(i, j);
                m = m.max((self.pos[i][j].0 - rx).abs().max((self.pos[i][j].1 - ry).abs()));
            }
        }
        m
    }

    fn speed(&self) -> f64 {
        let mut m: f64 = 0.0;
        for row in &self.vel {
            for v in row {
                m = m.max(v.0.abs().max(v.1.abs()));
            }
        }
        m
    }

    /// Hanging still: nothing to draw bent any more.
    pub fn settled(&self) -> bool {
        self.grab.is_none() && self.displacement() < 0.25 && self.speed() < 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64) -> Rectangle<f64, Logical> {
        Rectangle::new((x, y).into(), (300.0, 200.0).into())
    }

    #[test]
    fn dragging_bends_the_window_and_it_settles_afterwards() {
        let mut w = Wobble::new(rect(0.0, 0.0));
        assert!(w.settled());
        w.grab = Some(w.nearest((150.0, 10.0)));
        // drag to the right at 600 px/s for a third of a second
        for k in 1..=20 {
            w.step(1.0 / 60.0, rect(10.0 * k as f64, 0.0));
        }
        // the corner far from the grab trails behind
        let far = w.pos[N - 1][N - 1].0 - (rect(200.0, 0.0).loc.x + 300.0);
        assert!(far < -3.0, "the far corner lags: {far}");
        assert!(w.displacement() > 3.0);
        w.grab = None;
        for _ in 0..600 {
            w.step(1.0 / 60.0, rect(200.0, 0.0));
        }
        assert!(w.settled(), "settles: {}", w.displacement());
    }

    #[test]
    fn the_grabbed_point_stays_on_the_pointer() {
        let mut w = Wobble::new(rect(0.0, 0.0));
        let g = w.nearest((150.0, 100.0));
        w.grab = Some(g);
        for k in 1..=10 {
            w.step(1.0 / 60.0, rect(20.0 * k as f64, 5.0 * k as f64));
        }
        let want = (200.0 + 300.0 * g.1 as f64 / (N - 1) as f64, 50.0 + 200.0 * g.0 as f64 / (N - 1) as f64);
        assert!((w.pos[g.0][g.1].0 - want.0).abs() < 1e-6 && (w.pos[g.0][g.1].1 - want.1).abs() < 1e-6);
    }
}
