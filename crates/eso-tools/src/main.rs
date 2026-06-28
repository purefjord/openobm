//! `eso-dump` — emit ground-truth dumps of parsed assets in a canonical,
//! byte-comparable text format for diffing against the FreeJ2ME oracle.
//!
//! Usage:
//!   eso-dump jtm     <assets_dir> [/name.jtm ...]   # all .jtm if none named
//!   eso-dump lang    <assets_dir> [N ...]           # all lang_N if none named
//!   eso-dump jtm-sum <assets_dir>                   # compact per-layer hashes

use anyhow::{bail, Result};
use eso_tools::{
    dump_anim_trace, dump_cml, dump_collision_sweep, dump_combat_sweep, dump_dist_sweep,
    dump_effects_sweep, dump_hf_sweep, dump_jtm, dump_jtm_flat, dump_lang, dump_move_sweep,
    dump_scr, dump_scr_exec, dump_scr_trace, dump_targeting_sweep, dump_tick_sweep, dump_xp_sweep,
    save_roundtrip, scr_coverage, summarize_jtm,
};
use formats::AssetStore;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        bail!("usage: eso-dump <jtm|lang|jtm-sum> <assets_dir> [names...]");
    }
    let store = AssetStore::new(&args[1]);
    let rest = &args[2..];

    let out = match args[0].as_str() {
        "jtm" => dump_jtm(&store, rest)?,
        "jtm-sum" => summarize_jtm(&store)?,
        "jtm-flat" => dump_jtm_flat(
            &store,
            rest.first().map(String::as_str).unwrap_or("/l01_1.jtm"),
        )?,
        "cml" => dump_cml(&store, rest)?,
        "scr" => dump_scr(&store, rest)?,
        "scr-coverage" => scr_coverage(&store)?,
        // save-roundtrip takes a blob file path as the second arg (not an assets dir).
        "save-roundtrip" => save_roundtrip(&args[1])?,
        // hf-sweep takes the oracle's table dump file path (not an assets dir).
        "hf-sweep" => dump_hf_sweep(&args[1])?,
        // combat-sweep / dist-sweep / targeting-sweep are self-contained.
        "combat-sweep" => dump_combat_sweep()?,
        "dist-sweep" => dump_dist_sweep()?,
        "targeting-sweep" => dump_targeting_sweep()?,
        "collision-sweep" => dump_collision_sweep()?,
        "move-sweep" => dump_move_sweep()?,
        "anim-trace" => dump_anim_trace(&store)?,
        "effects-sweep" => dump_effects_sweep(&store)?,
        // tick-sweep takes the captured hf_tables.txt (the regen recompute calls h.f).
        "tick-sweep" => dump_tick_sweep(&args[1])?,
        // xp-sweep takes the oracle's table dump file path (for the level-up h.f).
        "xp-sweep" => dump_xp_sweep(&args[1])?,
        "scr-trace" => {
            let res = rest.first().map(String::as_str).unwrap_or("/startup.scr");
            let entry = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(1u8);
            let cap = rest
                .get(2)
                .and_then(|s| s.parse().ok())
                .unwrap_or(4096usize);
            dump_scr_trace(&store, res, entry, cap)?
        }
        "scr-exec" => {
            let res = rest.first().map(String::as_str).unwrap_or("/startup.scr");
            let entry = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(1u8);
            let cap = rest
                .get(2)
                .and_then(|s| s.parse().ok())
                .unwrap_or(4096usize);
            dump_scr_exec(&store, res, entry, cap)?
        }
        "lang" => {
            let ids = rest
                .iter()
                .map(|s| {
                    s.parse::<u8>()
                        .map_err(|_| anyhow::anyhow!("bad lang id {s}"))
                })
                .collect::<Result<Vec<u8>>>()?;
            dump_lang(&store, &ids)?
        }
        other => bail!("unknown dump kind {other:?}"),
    };
    print!("{out}");
    Ok(())
}
