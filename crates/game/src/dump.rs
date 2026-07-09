//! Byte-comparable state dumps mirroring `Instrument.dumpWorld` / `dumpJtm`
//! exactly — the Rust half of the `dumpworld`/`dumpjtm` script commands (one
//! drive script produces the same artifacts on both sides).

use crate::shell::Shell;

/// `Instrument.dumpWorld`'s format: the settled-deterministic per-actor
/// fields + world scalars + the open dialogue's wrapped lines. Meant for
/// op21-gated holds; panics when no dialogue is open (the oracle side would
/// dump the STALE line vector there — `b.var_java_util_Vector_b` persists
/// after dismissal, our `Option<Dialogue>` does not).
pub fn world_dump(s: &Shell) -> String {
    world_dump_inner(s, false)
}

/// The generator-gate variant, for the maze + exit-chain gates: masks the
/// CARRIED / PRESENTATION b-state that the op47 generator and the room-load
/// chain do NOT deterministically produce, because reaching them runs the
/// unattended, non-deterministic L01 opening/fight. Masked fields:
/// - the player's (slot 0) INVENTORY (`inv=<carried>`) — the maze op15 spawn
///   reuses-or-freshly-spawns the fight-chaos player (the oracle itself drifts
///   run-to-run between its carried consumables and a base row-1 spawn);
/// - `hud` (op76 `var_boolean_e`) — a carried flag the maze never sets;
/// - the DIALOGUE presentation (`speaker`, `dlg`, the wrapped lines) — depends
///   on the entry-cell FIRE's 1s-dismiss timing and on lang-overlay load state
///   (the l01_1b/c "you return" text resolves empty on the real game here);
/// - `respawn` (op71) — at the L02 checkpoint-menu hold it races the
///   op45(mode 3)/op12 boundary (the cutscene's op71 ran or not depending on
///   frame timing; the oracle itself drifts run-to-run).
///
/// Everything the generator/chain DOES determine stays gated byte-for-byte:
/// the map layers, the event overlays, the seeded ENEMY spawns + pickups, the
/// player's class/stats/pos, and `cam`/`gold`/`lock`.
pub fn world_dump_gen(s: &Shell) -> String {
    world_dump_inner(s, true)
}

fn world_dump_inner(s: &Shell, mask_player_inv: bool) -> String {
    let mut out = String::new();
    out.push_str("# world dump: per-actor deterministic fields at a settled hold\n");
    for (n, slot) in s.world.actors.iter().enumerate() {
        let Some(a) = slot else { continue };
        out.push_str(&format!(
            "actor {n} c={} f={} o={} j={} r={} t={} y={} z={} u={} s={} k={} p={} g={} v={} dead={}",
            a.var_byte_c, a.var_byte_f, a.var_byte_o, a.var_byte_j, a.var_byte_r, a.var_byte_t,
            a.var_byte_y, a.var_byte_z, a.var_byte_u, a.var_byte_s, a.var_byte_k, a.var_byte_p,
            a.var_byte_g, a.var_byte_v, a.var_byte_q,
        ));
        out.push_str(&format!(
            " st={},{},{},{},{},{},{}",
            a.var_short_s,
            a.var_short_t,
            a.var_short_u,
            a.var_short_v,
            a.var_short_w,
            a.var_short_x,
            a.var_short_y,
        ));
        out.push_str(&format!(" hp={}/{}", a.var_short_q, a.var_short_o));
        out.push_str(&format!(" fat={}/{}", a.var_short_r, a.var_short_p));
        out.push_str(&format!(
            " E={} F={} m={} az={}",
            a.e_field, a.f_field, a.var_short_m, a.var_short_z
        ));
        out.push_str(&format!(
            " pos={},{}",
            a.var_int_arr_b[0], a.var_int_arr_b[1]
        ));
        out.push_str(&format!(
            " walk={},{}",
            a.var_int_arr_j[0], a.var_int_arr_j[1]
        ));
        let armor = a
            .var_int_arr_n
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        out.push_str(&format!(" armor={armor}"));
        if mask_player_inv && n == 0 {
            out.push_str(" inv=<carried>");
        } else {
            let inv: Vec<String> = a
                .var_int_arr_k
                .iter()
                .take_while(|&&v| v != 0)
                .map(ToString::to_string)
                .collect();
            out.push_str(&format!(
                " inv={}",
                if inv.is_empty() {
                    "-".into()
                } else {
                    inv.join(",")
                }
            ));
        }
        out.push_str(&format!(
            " spell={}",
            a.var_int_arr_l
                .as_ref()
                .map(|w| w[0].to_string())
                .unwrap_or_else(|| "-".into())
        ));
        out.push_str(&format!(" model={}", a.model_name));
        out.push_str(&format!(
            " name={}\n",
            a.display_name.as_deref().unwrap_or("-")
        ));
    }
    // The dialogue OPEN/CLOSED state (`dlg=` + the wrapped lines) is masked in
    // the generator variant: it depends on whether the entry-cell FIRE that
    // opened it has been dismissed by dump time (the 1s rule × wall-clock
    // jitter), which the generator does not determine. The strict variant
    // gates it at a settled op21 hold.
    // `hud` (the op76 var_boolean_e flag) and the dialogue state are CARRIED
    // b-flags the maze op47 does not set, so they reflect the non-deterministic
    // pre-maze fight — masked in the generator variant.
    // `respawn` (op71 var_short_i/j) is masked in the generator variant too:
    // at the L02 checkpoint-menu hold it RACES the op45(mode 3)/op12 boundary —
    // the cutscene's op71 either ran (a coord) or not (0,0) depending on frame
    // timing, non-deterministic on BOTH sides (the oracle drifts run-to-run).
    let (dlg_field, hud_field, respawn_field) = if mask_player_inv {
        (
            "<masked>".to_string(),
            "<masked>".to_string(),
            "<masked>".to_string(),
        )
    } else {
        (
            i32::from(s.world.dialogue.is_some()).to_string(),
            i32::from(s.world.hud_enabled).to_string(),
            format!("{},{}", s.world.respawn[0], s.world.respawn[1]),
        )
    };
    out.push_str(&format!(
        "cam={} respawn={} gold={} hud={} lock={} speaker={} dlg={} pickups={}\n",
        s.world.cam_follow,
        respawn_field,
        s.world.gold,
        hud_field,
        i32::from(!s.world.input_unlocked),
        if mask_player_inv {
            "<masked>"
        } else {
            s.world.speaker.as_deref().unwrap_or("-")
        },
        dlg_field,
        if s.world.pickup_count == 0 {
            "-".into()
        } else {
            s.world.pickups[..s.world.pickup_count as usize]
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        },
    ));
    if mask_player_inv {
        out.push_str("dialogue:<masked>\n");
    } else {
        let dlg = s
            .world
            .dialogue
            .as_ref()
            .expect("world_dump at a dialogue hold");
        out.push_str(&format!("dialogue:|{}\n", dlg.lines.join("|")));
    }
    out
}

/// `Instrument.dumpOver`'s format: the three event overlays (enter
/// `b.j:[B`, leave `b.k:[B`, action `b.c:[B`) as flat unsigned-decimal
/// bytes — the maze generator's j/k/c writes are invisible to `layers_dump`
/// (they're not vector layers), so the op47 gate pins them here.
pub fn overlays_dump(s: &Shell) -> String {
    let mut out = String::new();
    out.push_str("overlays=3\n");
    for layer in [&s.world.enter, &s.world.leave, &s.world.action] {
        let line = layer
            .iter()
            .map(|&v| (v as u8).to_string())
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// `Instrument.dumpJtm`'s format: collision + every visual layer as flat
/// unsigned-decimal bytes (x*height+y order).
pub fn layers_dump(s: &Shell) -> String {
    let mut out = String::new();
    out.push_str(&format!("layers={}\n", 1 + s.world.layers.len()));
    let dump = |out: &mut String, layer: &[i8]| {
        let line = layer
            .iter()
            .map(|&v| (v as u8).to_string())
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&line);
        out.push('\n');
    };
    dump(&mut out, &s.world.collision);
    for layer in &s.world.layers {
        dump(&mut out, layer);
    }
    out
}
