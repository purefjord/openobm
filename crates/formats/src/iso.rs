//! Isometric world<->screen coordinate transforms.
//!
//! Ported verbatim from `b.java` methods `a(int[],int[])` (~line 1283) and
//! `b(int[],int[])` (~line 1288):
//!
//! ```text
//! // world -> screen
//! sx = (wx - wy) >> 3
//! sy = (wx + wy) >> 4
//! // screen -> world
//! wx = (sx << 2) + (sy << 3)
//! wy = (sy << 3) - (sx << 2)
//! ```
//!
//! In Java, `-`/`+` bind tighter than `>>`, so `nArray[0] - nArray[1] >> 3`
//! means `(wx - wy) >> 3` — we parenthesize explicitly. The shifts are
//! arithmetic on signed 32-bit ints; Rust `>>` on `i32` is arithmetic, matching.
//!
//! Note: `world_to_screen` is lossy (the `>>3`/`>>4` discard low bits), so a
//! `world -> screen -> world` round-trip is NOT identity. But `screen -> world`
//! produces coordinates that round-trip exactly back through `world_to_screen`
//! (proven in tests), which is the invariant the renderer relies on.

/// A 2D integer point. `x`/`y` are `i32` to match Java `int` arithmetic exactly.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash)]
pub struct Vec2i {
    pub x: i32,
    pub y: i32,
}

impl Vec2i {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// World -> screen. Lossy (discards low bits via arithmetic shift).
pub fn world_to_screen(p: Vec2i) -> Vec2i {
    Vec2i {
        x: (p.x - p.y) >> 3,
        y: (p.x + p.y) >> 4,
    }
}

/// Screen -> world. Exact inverse of `world_to_screen` for screen-space inputs.
pub fn screen_to_world(p: Vec2i) -> Vec2i {
    Vec2i {
        x: (p.x << 2) + (p.y << 3),
        y: (p.y << 3) - (p.x << 2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lock the exact arithmetic against hand-computed values so a stray shift
    /// constant fails immediately.
    #[test]
    fn exact_shift_values() {
        // world_to_screen((0,0)) = (0,0)
        assert_eq!(world_to_screen(Vec2i::new(0, 0)), Vec2i::new(0, 0));
        // (64, 0): sx = 64>>3 = 8, sy = 64>>4 = 4
        assert_eq!(world_to_screen(Vec2i::new(64, 0)), Vec2i::new(8, 4));
        // (0, 64): sx = (-64)>>3 = -8, sy = 64>>4 = 4
        assert_eq!(world_to_screen(Vec2i::new(0, 64)), Vec2i::new(-8, 4));
        // (16, 8): sx = (8)>>3 = 1, sy = (24)>>4 = 1
        assert_eq!(world_to_screen(Vec2i::new(16, 8)), Vec2i::new(1, 1));

        // screen_to_world((1,1)) = ((1<<2)+(1<<3), (1<<3)-(1<<2)) = (12, 4)
        assert_eq!(screen_to_world(Vec2i::new(1, 1)), Vec2i::new(12, 4));
        // screen_to_world((8,4)) = ((32)+(32), (32)-(32)) = (64, 0)
        assert_eq!(screen_to_world(Vec2i::new(8, 4)), Vec2i::new(64, 0));
    }

    /// Arithmetic (not logical) shift on negative coordinates.
    #[test]
    fn negative_arithmetic_shift() {
        // (-1 - 0) >> 3 = -1 (arithmetic); a logical shift would give a huge value.
        assert_eq!(world_to_screen(Vec2i::new(-1, 0)), Vec2i::new(-1, -1));
    }

    /// screen -> world -> screen is an exact identity (the load-bearing invariant).
    #[test]
    fn screen_world_round_trip_is_identity() {
        for sx in -50..=50 {
            for sy in -50..=50 {
                let s = Vec2i::new(sx, sy);
                assert_eq!(
                    world_to_screen(screen_to_world(s)),
                    s,
                    "round-trip failed at {s:?}"
                );
            }
        }
    }
}
