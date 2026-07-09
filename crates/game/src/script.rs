//! The OracleRun input-script grammar, parsed AND EXECUTED on the Rust side
//! so ONE script drives both the real jar (oracle/OracleRun.java) and the
//! shell ([`drive`]). The parity harness syncs on the explicit `shot`/
//! `shotnorm`/dump commands (frame counts differ between the wall-clock
//! oracle and the fixed-dt shell), so each names a checkpoint where both
//! sides have settled after the preceding input.
//!
//! Grammar (mirrors OracleRun; unknown/oracle-only commands are skipped):
//!   wait <ms>              advance time (REAL ms; multiplied by `timescale`)
//!   timescale <n>          game time runs n x wall speed from here on
//!   tap <key>              press+release
//!   press <key> / release <key>
//!   shot <name.png>        checkpoint the current frame
//!   shotnorm <name.png>    normalize (anims/effects/camera) + checkpoint
//!   dumpworld <name.txt>   the Instrument.dumpWorld-format state dump
//!   dumpjtm <name.txt>     the Instrument.dumpJtm-format layers dump
//!   pause / unpause        park the loop (waits advance NO ticks in between)
//!   teleport <slot> <x> <y>       the op36 native (h.a(j,int,int))
//!   follow <slot>                 the op26 native (b.b(int))
//!   setfloat <slot> <text|lang<id>>  install floating combat text
//!   setscroll <g>                 pin the text-page scroll (g:S; h:S = 0)
//!   sethud <0|1>                  the op76 HUD-enable flag (b.var_boolean_e)
//!   setflat <v...>                write the subtype-7 shop stock (e.f:[I)
//!   callf                         invoke b.f() (the op45 checkpoint menu)
//!   callhide / callshow           fire hideNotify/showNotify (m22 interrupt)
//!   callmode <n>                  invoke the REAL setter b.a((byte)n) (op61 etc.)
//!   calllang <id>                 the op56 native: load a lang overlay table
//!   callscript <res>              the op29 native b.a(String): load a script
//!   setseed <n>                   re-base the shared combat/maze RNG
//!   callmaze <row> <n> <n2>       the op47 native (maze regen; seed first)
//!   dumpover <name.txt>           the Instrument.dumpOver overlays dump
//!   callentry <n>                 the real e.a(int) script-entry push
//!   dumpworldg <name.txt>         world dump with the player's carried
//!                                 inventory masked (generator gates)

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
    TimeScale(u64),
    Tap(i32),
    Press(i32),
    Release(i32),
    Shot(String),
    ShotNorm(String),
    DumpWorld(String),
    DumpJtm(String),
    Pause,
    Unpause,
    Teleport(usize, i32, i32),
    Follow(i32),
    SetFloat(usize, String),
    SetScroll(i16),
    SetHud(bool),
    SetFlat(Vec<i32>),
    CallF,
    CallHide,
    CallShow,
    CallMode(i8),
    CallLang(u16),
    CallScript(String),
    SetSeed(i64),
    CallMaze(i32, i32, i32),
    DumpOver(String),
    CallEntry(u8),
    DumpWorldG(String),
}

/// Parse a script; unknown/oracle-only commands (modelog, …) are skipped
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
            "timescale" => Cmd::TimeScale(arg()?.parse()?),
            "tap" => Cmd::Tap(keycode(arg()?)?),
            "press" => Cmd::Press(keycode(arg()?)?),
            "release" => Cmd::Release(keycode(arg()?)?),
            "shot" => Cmd::Shot(arg()?.to_string()),
            "shotnorm" => Cmd::ShotNorm(arg()?.to_string()),
            "dumpworld" => Cmd::DumpWorld(arg()?.to_string()),
            "dumpjtm" => Cmd::DumpJtm(arg()?.to_string()),
            "pause" => Cmd::Pause,
            "unpause" => Cmd::Unpause,
            "teleport" => Cmd::Teleport(arg()?.parse()?, arg()?.parse()?, arg()?.parse()?),
            "follow" => Cmd::Follow(arg()?.parse()?),
            "setfloat" => Cmd::SetFloat(arg()?.parse()?, arg()?.to_string()),
            "setscroll" => Cmd::SetScroll(arg()?.parse()?),
            "sethud" => Cmd::SetHud(arg()? != "0"),
            "setflat" => Cmd::SetFlat(
                it.by_ref()
                    .map(str::parse)
                    .collect::<Result<Vec<i32>, _>>()?,
            ),
            "callf" => Cmd::CallF,
            "callhide" => Cmd::CallHide,
            "callshow" => Cmd::CallShow,
            "callmode" => Cmd::CallMode(arg()?.parse()?),
            "calllang" => Cmd::CallLang(arg()?.parse()?),
            "callscript" => Cmd::CallScript(arg()?.to_string()),
            "setseed" => Cmd::SetSeed(arg()?.parse()?),
            "callmaze" => Cmd::CallMaze(arg()?.parse()?, arg()?.parse()?, arg()?.parse()?),
            "dumpover" => Cmd::DumpOver(arg()?.to_string()),
            "callentry" => Cmd::CallEntry(arg()?.parse()?),
            "dumpworldg" => Cmd::DumpWorldG(arg()?.to_string()),
            // oracle-only instrumentation (modelog, dump sweeps, …): ignore
            _ => continue,
        });
    }
    Ok(out)
}

/// A named checkpoint a [`drive`] produced — a rendered frame (`shot`/
/// `shotnorm`) or a state dump (`dumpworld`/`dumpjtm`).
pub enum Artifact {
    Frame(crate::fb::Fb),
    Text(String),
}

/// Execute a drive script against the shell — the Rust half of the ONE
/// script that also drives the real jar through OracleRun. `wait` advances
/// fixed 50ms ticks covering `ms x timescale` GAME milliseconds (the oracle
/// side's waits are real time against the scaled clock); between `pause`
/// and `unpause` waits advance NO ticks (the real loop is parked, so
/// injections land on frozen state). Injections mirror the Instrument
/// commands exactly: `teleport` = the op36 native, `follow` = the op26
/// native, `setfloat` installs the combat floating-text fields (`lang<id>`
/// becomes the `<id>` placeholder the paint resolves through the lang
/// table, byte-exact with the oracle's real `b.a(int)` lookup), `setscroll`
/// pins `g:S`/`h:S`.
pub fn drive(
    shell: &mut crate::shell::Shell,
    script: &str,
) -> anyhow::Result<std::collections::HashMap<String, Artifact>> {
    let mut artifacts = std::collections::HashMap::new();
    let mut scale: u64 = 1;
    let mut paused = false;
    for cmd in parse(script)? {
        match cmd {
            Cmd::Wait(ms) => {
                if paused {
                    continue;
                }
                let mut left = (ms * scale) as i64;
                while left > 0 {
                    shell.tick(50.min(left) as i32);
                    left -= 50;
                }
            }
            Cmd::TimeScale(n) => scale = n,
            Cmd::Tap(k) => shell.press(k),
            Cmd::Press(k) => shell.hold(k),
            Cmd::Release(_) => shell.release(),
            Cmd::Shot(name) => {
                artifacts.insert(name, Artifact::Frame(shell.render()?));
            }
            Cmd::ShotNorm(name) => {
                shell.normalize_for_shot();
                artifacts.insert(name, Artifact::Frame(shell.render()?));
            }
            Cmd::DumpWorld(name) => {
                artifacts.insert(name, Artifact::Text(crate::dump::world_dump(shell)));
            }
            Cmd::DumpJtm(name) => {
                artifacts.insert(name, Artifact::Text(crate::dump::layers_dump(shell)));
            }
            Cmd::Pause => paused = true,
            Cmd::Unpause => paused = false,
            Cmd::Teleport(slot, x, y) => {
                let a = shell.world.actors[slot]
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("teleport: empty slot {slot}"))?;
                formats::set_position(a, x, y);
            }
            Cmd::Follow(slot) => shell.world.camera_follow(slot),
            Cmd::SetFloat(slot, text) => {
                let text = match text.strip_prefix("lang") {
                    Some(id) => format!("<{id}>"),
                    None => text,
                };
                let a = shell.world.actors[slot]
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("setfloat: empty slot {slot}"))?;
                a.floating_text = Some(text);
                a.q_field = 0;
                a.r_field = 0;
                a.var_short_h = 0;
            }
            Cmd::SetScroll(g) => shell.set_scroll(g),
            Cmd::SetHud(on) => shell.world.hud_enabled = on,
            Cmd::SetFlat(vals) => shell.set_flat7(&vals),
            Cmd::CallF => shell.f_checkpoint_menu(),
            Cmd::CallHide => shell.hide_notify(),
            Cmd::CallShow => shell.show_notify(),
            Cmd::CallMode(n) => shell.call_mode(n),
            Cmd::CallLang(id) => shell.load_lang_overlay(id),
            Cmd::CallScript(name) => shell.call_script(&name),
            Cmd::SetSeed(seed) => shell.set_seed(seed),
            Cmd::CallMaze(row, n, n2) => shell.op47_maze(row, n, n2),
            Cmd::DumpOver(name) => {
                artifacts.insert(name, Artifact::Text(crate::dump::overlays_dump(shell)));
            }
            Cmd::CallEntry(n) => shell.call_entry(n),
            Cmd::DumpWorldG(name) => {
                artifacts.insert(name, Artifact::Text(crate::dump::world_dump_gen(shell)));
            }
        }
    }
    Ok(artifacts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_skips_oracle_only() {
        // the real drive grammar, incl. an oracle-only line (modelog) and the
        // shared checkpoint/injection commands
        let cmds = parse(
            "# comment\nmodelog modes.txt\ntimescale 10\nwait 21000\ntap fire\n\
             shot 00.png\nshotnorm n0.png\ndumpjtm g.txt\npause\nsetfloat 3 lang471\n\
             teleport 0 2752 3904\nsetscroll -50\nunpause\n",
        )
        .unwrap();
        assert_eq!(
            cmds,
            vec![
                Cmd::TimeScale(10),
                Cmd::Wait(21000),
                Cmd::Tap(53),
                Cmd::Shot("00.png".into()),
                Cmd::ShotNorm("n0.png".into()),
                Cmd::DumpJtm("g.txt".into()),
                Cmd::Pause,
                Cmd::SetFloat(3, "lang471".into()),
                Cmd::Teleport(0, 2752, 3904),
                Cmd::SetScroll(-50),
                Cmd::Unpause,
            ]
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
