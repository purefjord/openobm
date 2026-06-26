//! `eso-dump` — emit ground-truth dumps of parsed assets in a canonical,
//! byte-comparable text format for diffing against the FreeJ2ME oracle.
//!
//! Usage:
//!   eso-dump jtm     <assets_dir> [/name.jtm ...]   # all .jtm if none named
//!   eso-dump lang    <assets_dir> [N ...]           # all lang_N if none named
//!   eso-dump jtm-sum <assets_dir>                   # compact per-layer hashes

use anyhow::{bail, Result};
use eso_tools::{
    dump_cml, dump_jtm, dump_lang, dump_scr, dump_scr_trace, scr_coverage, summarize_jtm,
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
        "cml" => dump_cml(&store, rest)?,
        "scr" => dump_scr(&store, rest)?,
        "scr-coverage" => scr_coverage(&store)?,
        "scr-trace" => {
            let res = rest.first().map(String::as_str).unwrap_or("/startup.scr");
            let entry = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(1u8);
            let cap = rest
                .get(2)
                .and_then(|s| s.parse().ok())
                .unwrap_or(4096usize);
            dump_scr_trace(&store, res, entry, cap)?
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
