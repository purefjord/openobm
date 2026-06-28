//! Animation playback layer — port of `g.java`'s frame-cursor primitives.
//!
//! `cml.rs` (M4) ports the `.cml` *loader* (`g.a(String)`), which the original
//! materializes as a linked list of `d` nodes. The runtime then *plays* those
//! animations by mutating a per-group playback cursor (`d.var_d_b`). This module
//! ports that playback layer — the small set of graphics-free primitives the
//! game loop calls every frame to advance/seek/reset an animation:
//!
//! | `g.java` (real descriptor) | CFR name           | here                |
//! |----------------------------|--------------------|---------------------|
//! | `d a(d,int)`               | `d_a(d,int)`       | [`Anim::lookup`]    |
//! | `boolean a(d,int)`         | `boolean_a(d,int)` | [`Anim::advance`]   |
//! | `boolean a(d,int,int)`     | `a(d,int,int)`     | [`Anim::seek`]      |
//! | `void a(d,int)`            | `void_a(d,int)`    | [`Anim::reset`]     |
//!
//! These four overloads differ **only by return type** — CFR disambiguated them
//! with suffixes, but the real `.class` names them all `a`, so a call site like
//! `g.a(d, n)` in `h.java`/`i.java` is ambiguous in the decompiled source and was
//! resolved against the bytecode descriptor (`javap -c`): `h.java`'s per-actor
//! tick advance (~line 362) is `boolean a(d,int)`; `i.java`'s effect step is
//! `boolean a(d,int,int)` (seek); the `>>1` width uses are `int a(d,int)`.
//!
//! ## The `d` structure, flattened
//!
//! `g.a(String)` builds one top-level chain (via `var_d_c`) whose nodes are
//! *either* a static record *or* an animation group — keyed by `var_byte_a`:
//! - static record: one node, a single frame (`var_d_a == null`); `var_byte_a` =
//!   `(byte)effective_id`, `var_byte_d` = `(byte)flags[7]` (loop flag).
//! - group: one node *per group*; the node IS frame 0, frames 1.. chained via
//!   `var_d_a`. The frame loop sets `var_byte_a = (byte)groupFlags[0]` and
//!   `var_byte_d = (byte)groupFlags[7]` (note: the group's key/loop come from the
//!   **group** flag block, the per-frame offsets from each **frame** block).
//!
//! The `/4.png` record is skipped entirely (the loader `continue`s).
//!
//! Playback depends only on `(key, loop, frame_count)` per node plus a cursor, so
//! [`Anim`] flattens the [`Cml`] into exactly that — no `d` pointers, no PNGs.
//! Width/height (`int a(d,int)` / `int b(d,int)`) come from the frame data / PNG
//! and belong to the renderer; they are not part of this state machine.

use crate::cml::Cml;

/// One playable node: a static record or an animation group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimNode {
    /// Lookup key (`d.var_byte_a`), a signed byte (keys like `-55`/`-56` exist).
    pub key: i8,
    /// `d.var_byte_d == 1`: the animation loops at its end instead of clamping.
    pub looping: bool,
    /// Number of frames in the `var_d_a` chain (always `>= 1`).
    pub frame_count: usize,
}

/// A flattened `.cml` model plus its mutable per-node playback cursors (the
/// `d.var_d_b` pointers the original mutates in place).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anim {
    /// Nodes in construction order (the original `var_d_c` chain order), so
    /// [`Anim::lookup`] resolves duplicate keys to the **first** like `g.d_a`.
    nodes: Vec<AnimNode>,
    /// Current frame index per node (parallel to `nodes`); the `var_d_b` cursor.
    cursors: Vec<usize>,
}

impl Anim {
    /// Flatten a parsed [`Cml`] into playable nodes, reproducing the order and
    /// the `(key, loop, frame_count)` extraction of `g.a(String)`'s `d`-build.
    pub fn from_cml(cml: &Cml) -> Self {
        let mut nodes = Vec::new();
        for rec in &cml.records {
            if rec.skipped {
                // `/4.png`: the loader `continue`s before building any node.
                continue;
            }
            if rec.is_static {
                // `g.a(int[],d)` copies the record flags: var_byte_a =
                // (byte)nArray[0] (== effective_id), var_byte_d = (byte)nArray[7].
                nodes.push(AnimNode {
                    key: rec.effective_id as i8,
                    looping: (rec.flags[7] as i8) == 1,
                    frame_count: 1,
                });
            } else {
                for g in &rec.anim_groups {
                    if g.frames.is_empty() {
                        // Degenerate group (frame_count byte == 0): the original
                        // still creates the head node, but the frame loop never
                        // runs, so var_byte_a/var_byte_d keep their `d` defaults
                        // (0/0) and the lone node has a single (empty) frame.
                        nodes.push(AnimNode {
                            key: 0,
                            looping: false,
                            frame_count: 1,
                        });
                    } else {
                        // var_byte_a = (byte)groupFlags[0]; var_byte_d =
                        // (byte)groupFlags[7]; frame_count = n5 = frames.len().
                        nodes.push(AnimNode {
                            key: g.flags[0] as i8,
                            looping: (g.flags[7] as i8) == 1,
                            frame_count: g.frames.len(),
                        });
                    }
                }
            }
        }
        let cursors = vec![0usize; nodes.len()];
        Anim { nodes, cursors }
    }

    /// Build directly from a node list (construction order preserved). Used by
    /// the oracle harness to mirror synthetic `d`-graphs built in Java; the
    /// playback semantics depend only on `(key, loop, frame_count)`.
    pub fn from_nodes(nodes: Vec<AnimNode>) -> Self {
        let cursors = vec![0usize; nodes.len()];
        Anim { nodes, cursors }
    }

    /// `g.d_a(d,int)`: the first node whose key equals `n` (compared as `int`,
    /// the `byte` key sign-extended), or `None`. Returns the node's slot index.
    pub fn lookup(&self, n: i32) -> Option<usize> {
        self.nodes.iter().position(|node| i32::from(node.key) == n)
    }

    /// `g.boolean a(d,int)` (CFR `boolean_a`): advance the cursor of group `n`
    /// by one frame. Returns `true` exactly on the tick a **non-looping**
    /// animation reaches its end (the frame clamps at the last). A looping
    /// animation wraps to frame 0 and returns `false`; a missing group is "done"
    /// (`true`). This is the gating primitive the per-actor tick uses.
    pub fn advance(&mut self, n: i32) -> bool {
        let Some(i) = self.lookup(n) else {
            // d3 == null -> return true (the original NPE-guards with this).
            return true;
        };
        let node = self.nodes[i];
        let next = self.cursors[i] + 1;
        if next < node.frame_count {
            self.cursors[i] = next;
            false
        } else if node.looping {
            // var_byte_d == 1: var_d_b = d_a(d2,n) -> head (frame 0).
            self.cursors[i] = 0;
            false
        } else {
            // Non-loop: var_d_b restored to the old cursor (the last frame).
            // The cursor is already there (we did not advance it); report done.
            true
        }
    }

    /// `g.boolean a(d,int,int)` (CFR 3-arg `a`): seek group `n`'s cursor to frame
    /// `n2`. Returns `true` if `n2` is **past the last frame** (`n2 >= frame_count`)
    /// or the group is missing — leaving the cursor unchanged in that case. This
    /// is how `i.java` detects an effect's animation has played out (expiry).
    ///
    /// Edge case matching the bytecode: `n2 <= 0` seeks to frame 0 and returns
    /// `false` (a negative count makes the original's `for n3<n2` loop empty).
    pub fn seek(&mut self, n: i32, n2: i32) -> bool {
        let Some(i) = self.lookup(n) else {
            return true;
        };
        if n2 <= 0 {
            self.cursors[i] = 0;
            return false;
        }
        let frame = n2 as usize;
        if frame >= self.nodes[i].frame_count {
            // Could not walk that far down the var_d_a chain: cursor untouched.
            return true;
        }
        self.cursors[i] = frame;
        false
    }

    /// `g.void a(d,int)` (CFR `void_a`): reset group `n`'s cursor to frame 0
    /// (`d_a(d,n).var_d_b = d_a(d,n)`). No-op if the group is missing (the
    /// original would NPE; callers only reset existing groups).
    pub fn reset(&mut self, n: i32) {
        if let Some(i) = self.lookup(n) {
            self.cursors[i] = 0;
        }
    }

    /// The current frame index of group `n` (its `var_d_b` cursor position), or
    /// `None` if the group is missing. For the renderer and for trace validation.
    pub fn current_frame(&self, n: i32) -> Option<usize> {
        self.lookup(n).map(|i| self.cursors[i])
    }

    /// The flattened nodes, in construction order.
    pub fn nodes(&self) -> &[AnimNode] {
        &self.nodes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cml::{Cml, CmlAnimGroup, CmlRecord};

    /// Build a one-record Cml with a single animation group of `n` frames and the
    /// given key/loop flag, so we can exercise the primitives without real bytes.
    fn group_model(key: i32, looping: bool, frames: usize) -> Anim {
        let mut gflags = [0i32; 10];
        gflags[0] = key;
        gflags[7] = i32::from(looping);
        let rec = CmlRecord {
            frame_id: 0,
            effective_id: key,
            path: String::new(),
            flags: [0; 10],
            boxes: vec![],
            anim_groups: vec![CmlAnimGroup {
                flags: gflags,
                frames: vec![[0; 10]; frames],
            }],
            is_static: false,
            skipped: false,
        };
        Anim::from_cml(&Cml {
            prefix: String::new(),
            records: vec![rec],
            consumed: 0,
        })
    }

    #[test]
    fn extraction_key_loop_count() {
        let a = group_model(5, true, 3);
        assert_eq!(
            a.nodes(),
            &[AnimNode {
                key: 5,
                looping: true,
                frame_count: 3
            }]
        );
        // Negative keys round-trip through the i8 cast (e.g. shadow group -56).
        let b = group_model(-56, false, 1);
        assert_eq!(b.nodes()[0].key, -56);
    }

    #[test]
    fn advance_loops_through_frames() {
        let mut a = group_model(0, true, 3); // looping, 3 frames
        assert_eq!(a.current_frame(0), Some(0));
        assert!(!a.advance(0)); // 0 -> 1
        assert_eq!(a.current_frame(0), Some(1));
        assert!(!a.advance(0)); // 1 -> 2
        assert_eq!(a.current_frame(0), Some(2));
        assert!(!a.advance(0)); // 2 -> end -> wrap to 0 (loop, false)
        assert_eq!(a.current_frame(0), Some(0));
        assert!(!a.advance(0)); // 0 -> 1 again
        assert_eq!(a.current_frame(0), Some(1));
    }

    #[test]
    fn advance_clamps_and_signals_done_when_not_looping() {
        let mut a = group_model(0, false, 3); // non-looping, 3 frames
        assert!(!a.advance(0)); // 0 -> 1
        assert!(!a.advance(0)); // 1 -> 2
        assert!(a.advance(0)); // 2 -> end -> clamp at 2, return true
        assert_eq!(a.current_frame(0), Some(2));
        // Persistent "done": stays clamped and keeps returning true.
        assert!(a.advance(0));
        assert_eq!(a.current_frame(0), Some(2));
    }

    #[test]
    fn advance_single_frame() {
        // Looping single frame: stays at 0 forever, never "done".
        let mut loop1 = group_model(0, true, 1);
        assert!(!loop1.advance(0));
        assert_eq!(loop1.current_frame(0), Some(0));
        // Non-looping single frame: immediately "done", clamped at 0.
        let mut once1 = group_model(0, false, 1);
        assert!(once1.advance(0));
        assert_eq!(once1.current_frame(0), Some(0));
    }

    #[test]
    fn advance_missing_group_is_done() {
        let mut a = group_model(0, true, 3);
        assert!(a.advance(99)); // no such key -> true
    }

    #[test]
    fn seek_within_past_and_negative() {
        let mut a = group_model(0, true, 4); // frames 0..=3
        assert!(!a.seek(0, 2));
        assert_eq!(a.current_frame(0), Some(2));
        assert!(!a.seek(0, 3)); // last valid frame
        assert_eq!(a.current_frame(0), Some(3));
        // Past the end: returns true, cursor left untouched (still 3).
        assert!(a.seek(0, 4));
        assert_eq!(a.current_frame(0), Some(3));
        assert!(a.seek(0, 100));
        assert_eq!(a.current_frame(0), Some(3));
        // n2 <= 0 -> frame 0, false.
        assert!(!a.seek(0, 0));
        assert_eq!(a.current_frame(0), Some(0));
        a.seek(0, 2);
        assert!(!a.seek(0, -5));
        assert_eq!(a.current_frame(0), Some(0));
    }

    #[test]
    fn seek_missing_group_is_past_end() {
        let mut a = group_model(0, true, 4);
        assert!(a.seek(7, 0));
    }

    #[test]
    fn reset_returns_to_frame_zero() {
        let mut a = group_model(0, false, 4);
        a.seek(0, 3);
        assert_eq!(a.current_frame(0), Some(3));
        a.reset(0);
        assert_eq!(a.current_frame(0), Some(0));
        a.reset(123); // missing -> no-op, no panic
    }

    #[test]
    fn static_record_and_skipped_4png() {
        let rec_static = CmlRecord {
            frame_id: 9,
            effective_id: 9,
            path: "/x.png".into(),
            flags: [9, 0, 0, 0, 0, 0, 0, 1, 0, 0], // flags[7]=1 -> looping
            boxes: vec![],
            anim_groups: vec![],
            is_static: true,
            skipped: false,
        };
        let rec_skip = CmlRecord {
            frame_id: 4,
            effective_id: 4,
            path: "/4.png".into(),
            flags: [0; 10],
            boxes: vec![],
            anim_groups: vec![],
            is_static: false,
            skipped: true,
        };
        let a = Anim::from_cml(&Cml {
            prefix: String::new(),
            records: vec![rec_static, rec_skip],
            consumed: 0,
        });
        // Only the static record produced a node; /4.png was skipped.
        assert_eq!(
            a.nodes(),
            &[AnimNode {
                key: 9,
                looping: true,
                frame_count: 1
            }]
        );
    }
}
