//! The OracleRun input-script grammar, parsed on the Rust side so ONE script
//! drives both the real jar (oracle/OracleRun.java) and the shell. The parity
//! harness syncs on the explicit `shot` command (frame counts differ between
//! the wall-clock oracle and the fixed-dt shell), so a `shot` marks a
//! checkpoint where both sides have settled after the preceding input.
//!
//! Grammar (mirrors OracleRun; dump* commands are oracle-only and ignored here):
//!   wait <ms>            advance time
//!   tap <key>            press+release
//!   press <key> / release <key>
//!   shot <name.png>      checkpoint the current frame

/// MIDP key codes, matching OracleRun.keycode.
pub fn keycode(k: &str) -> anyhow::Result<i32> {
    Ok(match k {
        "up" => 50,
        "down" => 56,
        "left" => 52,
        "right" => 54,
        "fire" => 53,
        "star" => 42,
        "pound" => 35,
        _ => {
            if let Some(n) = k.strip_prefix("num") {
                48 + n.parse::<i32>()?
            } else {
                k.parse()?
            }
        }
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cmd {
    Wait(u64),
    Tap(i32),
    Press(i32),
    Release(i32),
    Shot(String),
}

/// Parse a script; unknown/oracle-only commands (dump*, modelog) are skipped
/// so the same file works verbatim on both sides.
pub fn parse(src: &str) -> anyhow::Result<Vec<Cmd>> {
    let mut out = Vec::new();
    for raw in src.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        let op = it.next().unwrap();
        let mut arg = || {
            it.next()
                .ok_or_else(|| anyhow::anyhow!("{op} needs an argument: {line}"))
        };
        out.push(match op {
            "wait" => Cmd::Wait(arg()?.parse()?),
            "tap" => Cmd::Tap(keycode(arg()?)?),
            "press" => Cmd::Press(keycode(arg()?)?),
            "release" => Cmd::Release(keycode(arg()?)?),
            "shot" => Cmd::Shot(arg()?.to_string()),
            // oracle-only instrumentation (dumpjtm, modelog, …): ignore
            _ => continue,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_skips_oracle_only() {
        // the real to_recon.txt grammar, incl. oracle-only lines
        let cmds = parse(
            "# comment\nmodelog modes.txt\nwait 21000\ntap fire\nshot 00.png\ndumpjtm g.txt\n",
        )
        .unwrap();
        assert_eq!(
            cmds,
            vec![Cmd::Wait(21000), Cmd::Tap(53), Cmd::Shot("00.png".into())]
        );
    }

    #[test]
    fn keycodes_match_oracle() {
        assert_eq!(keycode("fire").unwrap(), 53);
        assert_eq!(keycode("right").unwrap(), 54);
        assert_eq!(keycode("num7").unwrap(), 55);
        assert_eq!(keycode("21").unwrap(), 21);
    }
}
