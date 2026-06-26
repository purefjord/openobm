//! `eso-dump` — emit ground-truth dumps of parsed assets in a canonical,
//! byte-comparable text format for diffing against the FreeJ2ME oracle.
//!
//! Usage:
//!   eso-dump jtm     <assets_dir> [/name.jtm ...]   # all .jtm if none named
//!   eso-dump lang    <assets_dir> [N ...]           # all lang_N if none named
//!   eso-dump jtm-sum <assets_dir>                   # compact per-layer hashes

use anyhow::{bail, Result};
use eso_tools::{dump_jtm, dump_lang, summarize_jtm};
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
