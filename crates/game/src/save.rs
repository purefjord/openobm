//! `b.g()` (save) / `b.b(boolean)` (load) — building the `ESO` blob from the
//! live shell state and restoring it. The byte format itself is the
//! oracle-validated M9 [`formats::save`]; this module bridges it to the
//! runtime `World` (the write recomputes each item's `active` bit from the
//! live actor, the one thing M9 deferred).

use formats::save::{Save, SaveActor, SavePlayer};
use formats::Actor;

/// `h.a(j, ByteArrayOutputStream)` (h.java:1812) — build the actor blob's
/// fields from a live player, recomputing each inventory tag's `active` bit
/// (weapons: `var_byte_j == id`; armor: equipped in a slot; consumables:
/// always 0). `gold` is `b.var_int_b`.
pub fn build_save_actor(p: &Actor, gold: i32, tables: &formats::Tables) -> SaveActor {
    let mut items = Vec::new();
    for &tag in p.var_int_arr_k.iter().take_while(|&&v| v != 0) {
        let kind = (tag >> 8) & 0xFF;
        let id = tag & 0xFF;
        let active = match kind {
            0 => tables
                .row(4, id)
                .is_some_and(|row| p.is_active_row(row, false)),
            1 => p.has_armor_equipped(id),
            _ => false,
        };
        items.push((((active as u8) << 7) | (kind as u8), id as u8));
    }
    SaveActor {
        var_byte_c: p.var_byte_c,
        var_byte_f: p.var_byte_f,
        var_int_b: p.var_int_b,
        var_byte_o: p.var_byte_o,
        // NOTE the writer's field order is s,t,v,w,x,u (SaveActor encodes it).
        var_short_s: p.var_short_s,
        var_short_t: p.var_short_t,
        var_short_v: p.var_short_v,
        var_short_w: p.var_short_w,
        var_short_x: p.var_short_x,
        var_short_u: p.var_short_u,
        var_byte_j: p.var_byte_j,
        e: p.e_field,
        f: p.f_field,
        var_byte_r: p.var_byte_r,
        global_int_b: gold as u16,
        model_name: p.model_name.clone().into_bytes(),
        items,
    }
}

/// `b.g()` (b.java:2845) — serialize the whole `ESO` record: the key
/// bindings (`var_byte_arr_f` — the quick-key table Custom Controls edits,
/// NOT progress flags), the sound flag (`bool_o`), then (if a player
/// exists) the level-script name (`var_java_lang_String_c`) + the actor blob.
pub fn build_save(
    bindings: [i32; 3],
    bool_o: bool,
    level_script: &str,
    player: Option<&Actor>,
    gold: i32,
    tables: &formats::Tables,
) -> Vec<u8> {
    let save = Save {
        flags: [bindings[0] as u8, bindings[1] as u8, bindings[2] as u8],
        bool_o: u8::from(bool_o),
        player: player.map(|p| SavePlayer {
            name: level_script.as_bytes().to_vec(),
            actor: build_save_actor(p, gold, tables),
        }),
    };
    formats::serialize_save(&save)
}

/// `b.boolean_b()` (b.java:2875) — the has-save probe the `l()` menu build
/// reads: a stored record whose player-present byte (index `flags.len() + 1`
/// = 4) is 1. Modeled on the in-memory save slot.
pub fn has_save(slot: Option<&[u8]>) -> bool {
    slot.is_some_and(|b| b.get(4) == Some(&1))
}

/// `h.a(byte[], int)` (h.java:40) — rebuild a live player from a save-actor:
/// a default actor with the stored scalar fields, the class hang
/// (`class_init(from_save=true)` — no equip/attr overwrite), the inventory
/// re-equipped (each tag's `active` bit becomes the `force` flag), then the
/// health/fatigue derivation (`h.f`) and the model box extents.
pub fn restore_actor(
    a: &SaveActor,
    models: &mut crate::world::ModelCache,
    tables: &formats::Tables,
) -> Actor {
    let model = String::from_utf8_lossy(&a.model_name).into_owned();
    let frame_w = models.frame_w(&model);
    let mut p = Actor::create(&model, a.var_byte_c, frame_w);
    p.var_byte_f = a.var_byte_f;
    p.var_int_b = a.var_int_b;
    p.var_byte_o = a.var_byte_o;
    p.var_short_s = a.var_short_s;
    p.var_short_t = a.var_short_t;
    p.var_short_v = a.var_short_v;
    p.var_short_w = a.var_short_w;
    p.var_short_x = a.var_short_x;
    p.var_short_u = a.var_short_u;
    p.var_byte_j = a.var_byte_j;
    p.e_field = a.e;
    p.f_field = a.f;
    p.var_byte_r = a.var_byte_r;
    p.display_name = Some("Champion".into()); // j2.var_java_lang_String_c
    p.class_init(a.var_byte_f, true, tables); // h.a(j, var_byte_f, true)
    p.var_int_arr_k.iter_mut().for_each(|v| *v = 0);
    for &(b0, id) in &a.items {
        let active = (b0 & 0x80) != 0;
        let kind = i32::from(b0 & 0x7F);
        let subtype = match kind {
            0 => 4u8,
            1 => 1,
            2 => 2,
            k => panic!("save item kind {k} out of range"),
        };
        let row = tables
            .row(subtype, i32::from(id))
            .expect("save item row")
            .to_vec();
        p.equip(kind, &row, active, tables); // h.a(j, kind, e.b(kind,id), active)
    }
    p.class_progression(tables); // h.f
    p.var_byte_arr_a = [0, 0];
    p
}

/// Read a persisted `ESO` record file for the `play` frontend. Missing,
/// unreadable, or unparseable (corrupt/truncated — the file is
/// user-editable) all yield `None` = the wiped-RMS baseline. Persistence is
/// frontend-owned: the parity suites never touch these helpers.
pub fn read_save_file(path: &std::path::Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    // parse_save is lenient (trailing junk, odd player bytes) — require the
    // exact serialize round-trip a genuine record always satisfies (M9).
    let save = formats::parse_save(&bytes).ok()?;
    (formats::serialize_save(&save) == bytes).then_some(bytes)
}

/// Write the `ESO` record file atomically: temp file in the same directory
/// (same volume, so the rename can't cross filesystems), then rename over
/// the target (`MOVEFILE_REPLACE_EXISTING` semantics on Windows). Returns
/// `Err` instead of panicking — a transiently locked file (AV/indexer) is
/// the caller's cue to retry next frame, never to crash the window.
pub fn write_save_file(path: &std::path::Path, blob: &[u8]) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("save path has no parent directory"))?;
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension("bin.tmp");
    std::fs::write(&tmp, blob)?;
    std::fs::rename(&tmp, path)
}
