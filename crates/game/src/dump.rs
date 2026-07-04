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
    out.push_str(&format!(
        "cam={} respawn={},{} gold={} hud={} lock={} speaker={} dlg={} pickups={}\n",
        s.world.cam_follow,
        s.world.respawn[0],
        s.world.respawn[1],
        s.world.gold,
        i32::from(s.world.hud_enabled),
        i32::from(!s.world.input_unlocked),
        s.world.speaker.as_deref().unwrap_or("-"),
        i32::from(s.world.dialogue.is_some()),
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
    let dlg = s
        .world
        .dialogue
        .as_ref()
        .expect("world_dump at a dialogue hold");
    out.push_str(&format!("dialogue:|{}\n", dlg.lines.join("|")));
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
