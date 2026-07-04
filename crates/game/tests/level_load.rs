//! The class-fire level load (m=6 -> 15 -> 10 -> 0) into the mode-0 gameplay
//! loop, driven from COLD BOOT through the real scripts — the state slice.
//! Byte-level validation against the real bytecode lives in the oracle sweep
//! (`level_matches_oracle`); these tests pin the CHAIN and the world-state
//! shape the L01 choreography must produce.

use game::shell::{Screen, Shell};
use game::text::TextMasks;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn boot_shell() -> Shell {
    let masks = TextMasks::load(&root().join("tests/fixtures/oracle/text_masks.txt")).unwrap();
    Shell::boot(root().join("assets"), masks).unwrap()
}

fn tap(s: &mut Shell, key: i32) {
    s.press(key);
    s.tick(50);
}

/// Boot to the class-select screen (title key -> menu -> New Game).
fn shell_at_class_select() -> Shell {
    let mut s = boot_shell();
    for _ in 0..400 {
        s.tick(50);
        if s.screen() == Some(Screen::Title) {
            break;
        }
    }
    assert_eq!(s.screen(), Some(Screen::Title), "boot never reached title");
    tap(&mut s, 53); // title key
    for _ in 0..20 {
        s.tick(50);
    }
    assert_eq!(s.screen(), Some(Screen::MainMenu));
    tap(&mut s, 53); // New Game
    assert_eq!(s.screen(), Some(Screen::ClassSelect));
    s
}

/// The full chain: class fire -> loader (m=6) -> op73 please-wait (m=15,
/// mode-gate closed) -> the level ops -> op74 + op66 intro text (m=10) ->
/// auto-scroll to the end -> mode 0 -> the entry-7 cutscene reaches its first
/// FIRE-gated guard dialogue.
#[test]
fn class_fire_loads_l01_into_gameplay() {
    let mut s = shell_at_class_select();
    tap(&mut s, 53); // fire Monk -> the loader on /l01_1.scr
    assert_eq!(s.take_leave(), None, "the class fire is ported, no Leave");

    // The load choreography runs one opcode per frame: collect the modes seen
    // and stop at the first FIRE-gated cutscene dialogue (running the whole
    // scripted fight unattended ends in the fenced player-death screen —
    // faithfully — so the state assertions land at the first settled hold).
    let mut seen = vec![s.mode()];
    let mut fired_dialogue = false;
    for _ in 0..4000 {
        // ~200s of virtual time cap: the intro scroll alone is ~35s
        s.tick(50);
        if seen.last() != Some(&s.mode()) {
            seen.push(s.mode());
        }
        if s.mode() == 0 && s.world.dialogue.is_some() {
            fired_dialogue = true;
            break;
        }
    }
    assert_eq!(
        seen,
        vec![6, 15, 10, 0],
        "the real chain is loader -> please-wait -> intro text -> gameplay"
    );
    assert!(fired_dialogue, "the entry-7 cutscene shows guard dialogue");
    // The dialogue holds the VM (e.b(J) prologue) until FIRE after >= 1000ms:
    // an early FIRE is swallowed by the 1s rule (d(7)).
    let held = s.world.dialogue.clone();
    tap(&mut s, 53); // too early: < 1000ms open
    assert!(
        s.world.dialogue.is_some(),
        "FIRE inside 1s does not dismiss"
    );
    for _ in 0..25 {
        s.tick(50); // 1.25s more: past the 1s hold
    }
    assert_eq!(
        s.world.dialogue.as_ref().map(|d| &d.lines),
        held.as_ref().map(|d| &d.lines),
        "the dialogue text holds while open"
    );
    s.press(53); // FIRE >= 1s after open dismisses (the b(J) tail d(7))
    s.tick(50);
    assert!(s.world.dialogue.is_none(), "FIRE dismissed the dialogue");

    // The world after the load: the player (Monk, class 1) in slot 0 with the
    // camera following; the L01 map loaded; spawned NPCs present.
    let p = s.world.actors[0].as_ref().expect("player in slot 0");
    assert_eq!(p.var_byte_f, 1, "class-select cursor 0 = class 1 (Monk)");
    assert_eq!(p.var_byte_c, 1);
    assert_eq!(p.var_byte_s, 0, "the player never drops loot");
    assert!(s.world.map_w > 0 && s.world.map_h > 0, "l01_1.jtm loaded");
    assert!(!s.world.layers.is_empty());
    let npcs = (1..25).filter(|&i| s.world.actors[i].is_some()).count();
    assert!(npcs > 0, "the L01 choreography spawns guards/emperor");
    assert_eq!(s.world.gold, 100, "b:I = 100 on class fire");
    assert!(s.world.respawn != [0, 0], "op71 set the respawn anchor");
}

/// The post-choreography MAP STATE against the REAL game: `oracle/to_l01.txt`
/// drove the real jar to the same first-dialogue hold and `dumpjtm`'d the
/// live layers (collision + the visual layer Vector, flat unsigned bytes,
/// x*height+y). Every op18/22/49 write lands during the deterministic load
/// choreography, so the state is settled at the hold — diff byte-for-byte.
#[test]
fn l01_layers_match_the_real_game_at_dialogue1() {
    let mut s = shell_at_class_select();
    tap(&mut s, 53);
    for _ in 0..4000 {
        s.tick(50);
        if s.mode() == 0 && s.world.dialogue.is_some() {
            break;
        }
    }
    assert!(
        s.world.dialogue.is_some(),
        "never reached the dialogue hold"
    );
    let mut out = String::new();
    out.push_str(&format!("layers={}\n", 1 + s.world.layers.len()));
    let mut dump = |layer: &[i8]| {
        let line = layer
            .iter()
            .map(|&v| (v as u8).to_string())
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&line);
        out.push('\n');
    };
    dump(&s.world.collision);
    for layer in &s.world.layers {
        dump(layer);
    }
    let fixture =
        std::fs::read_to_string(root().join("tests/fixtures/oracle/l01_layers_dialogue1.txt"))
            .unwrap()
            .replace("\r\n", "\n");
    assert_eq!(out, fixture, "L01 layer state differs from the real game");
}

/// Build the world dump in `Instrument.dumpWorld`'s exact format (the
/// settled-deterministic per-actor fields + world scalars + dialogue lines),
/// shared by the hold-1 and hold-2 state gates.
fn world_dump(s: &Shell) -> String {
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
    let dlg = s.world.dialogue.as_ref().unwrap();
    out.push_str(&format!("dialogue:|{}\n", dlg.lines.join("|")));
    out
}

/// The post-choreography WORLD STATE against the REAL game: `dumpworld` read
/// the live actor array + world scalars at the same first-dialogue hold. The
/// dump carries only settled-deterministic fields (walkers have ARRIVED at
/// their exact targets behind the op21 gate; stats/equipment are op-driven;
/// wall-clock-coupled timers/anim/camera-offset state is excluded) — so the
/// whole spawn + class-init + equip + attr choreography diffs byte-for-byte.
#[test]
fn l01_world_state_matches_the_real_game_at_dialogue1() {
    let mut s = shell_at_class_select();
    tap(&mut s, 53);
    for _ in 0..4000 {
        s.tick(50);
        if s.mode() == 0 && s.world.dialogue.is_some() {
            break;
        }
    }
    assert!(
        s.world.dialogue.is_some(),
        "never reached the dialogue hold"
    );
    let fixture =
        std::fs::read_to_string(root().join("tests/fixtures/oracle/l01_world_dialogue1.txt"))
            .unwrap()
            .replace("\r\n", "\n");
    assert_eq!(
        world_dump(&s),
        fixture,
        "L01 world state differs from the real game"
    );
}

/// Render the gameplay frame at the first-dialogue hold and save it for
/// visual review (target/parity/l01_dialogue1_rust.png vs the real
/// artifacts/l01/l5_first_dialogue.png). Not pixel-gated yet: animated
/// sprite/effect frames are wall-clock-phased; the normalized-shot gate
/// follows.
#[test]
fn l01_gameplay_at_parity() {
    let mut s = shell_at_class_select();
    tap(&mut s, 53);
    for _ in 0..4000 {
        s.tick(50);
        if s.mode() == 0 && s.world.dialogue.is_some() {
            break;
        }
    }
    assert!(s.world.dialogue.is_some());
    // The NORMALIZED shot: both sides reset anim cursors + effect counters
    // and re-follow the camera at the settled hold (the oracle side is
    // Instrument.normalizeWorld behind the `shotnorm` command), making the
    // frame byte-comparable — anims/effects are otherwise wall-clock-phased.
    s.normalize_for_shot();
    let fb = s.render().expect("gameplay paint");
    let out = root().join("target/parity");
    std::fs::create_dir_all(&out).unwrap();
    fb.save_png(&out.join("l01_dialogue1_rust.png")).unwrap();
    let real =
        game::fb::Fb::load_png(&root().join("tests/fixtures/oracle/frames/l01_dialogue1_norm.png"))
            .unwrap();
    let (diff, bad) = fb.diff_region(&real, game::paint::LCD_H);
    if bad != 0 {
        diff.save_png(&out.join("l01_dialogue1_diff.png")).unwrap();
    }
    assert_eq!(
        bad, 0,
        "gameplay frame differs from the normalized real shot"
    );
}

/// The mode-15 please-wait screen at normalized parity.
#[test]
fn l01_please_wait_at_parity() {
    let mut s = shell_at_class_select();
    tap(&mut s, 53);
    for _ in 0..40 {
        s.tick(50);
        if s.mode() == 15 {
            break;
        }
    }
    assert_eq!(s.mode(), 15, "the loader chain reaches the please-wait");
    // A few more frames so the oh_pc anim has advanced (the oracle shot at
    // ~2.5s is mid-anim) — the normalization resets it on both sides anyway.
    for _ in 0..10 {
        s.tick(50);
    }
    assert_eq!(s.mode(), 15);
    s.normalize_for_shot();
    let fb = s.render().expect("please-wait paint");
    let out = root().join("target/parity");
    std::fs::create_dir_all(&out).unwrap();
    fb.save_png(&out.join("l01_please_wait_rust.png")).unwrap();
    let real = game::fb::Fb::load_png(
        &root().join("tests/fixtures/oracle/frames/l01_please_wait_norm.png"),
    )
    .unwrap();
    let (diff, bad) = fb.diff_region(&real, game::paint::LCD_H);
    if bad != 0 {
        diff.save_png(&out.join("l01_please_wait_diff.png"))
            .unwrap();
    }
    assert_eq!(bad, 0, "please-wait differs from the normalized real shot");
}

/// Drive to the SECOND dialogue hold: dismiss the first guard dialogue (FIRE
/// after the 1s rule) and let the cutscene run to its next op53 ("Quick,
/// Emperor, the secret passageway is in this cell.") — another settled,
/// VM-halted hold (the oracle's a1 == a2 shots pin that it is settled).
fn shell_at_dialogue2() -> Shell {
    let mut s = shell_at_class_select();
    tap(&mut s, 53);
    for _ in 0..4000 {
        s.tick(50);
        if s.mode() == 0 && s.world.dialogue.is_some() {
            break;
        }
    }
    assert!(s.world.dialogue.is_some(), "never reached hold 1");
    for _ in 0..24 {
        s.tick(50); // clear the 1s FIRE-dismiss rule
    }
    tap(&mut s, 53);
    assert!(s.world.dialogue.is_none(), "dismiss failed");
    for _ in 0..100 {
        s.tick(50);
        if s.world.dialogue.is_some() {
            break;
        }
    }
    assert!(s.world.dialogue.is_some(), "never reached hold 2");
    s
}

/// The normalized-shot pixel gate shared by the anchor tests.
fn assert_frame(s: &mut Shell, name: &str, fixture: &str) -> game::fb::Fb {
    s.normalize_for_shot();
    let fb = s.render().expect("gameplay paint");
    let out = root().join("target/parity");
    std::fs::create_dir_all(&out).unwrap();
    fb.save_png(&out.join(format!("{name}_rust.png"))).unwrap();
    let real =
        game::fb::Fb::load_png(&root().join(format!("tests/fixtures/oracle/frames/{fixture}")))
            .unwrap();
    let (diff, bad) = fb.diff_region(&real, game::paint::LCD_H);
    if bad != 0 {
        diff.save_png(&out.join(format!("{name}_diff.png")))
            .unwrap();
    }
    assert_eq!(bad, 0, "{name} differs from the normalized real shot");
    fb
}

/// The SECOND dialogue hold at normalized-shot parity (oracle a1, pinned
/// settled by a1 == a2).
#[test]
fn l01_dialogue2_at_parity() {
    let mut s = shell_at_dialogue2();
    assert_frame(&mut s, "l01_dialogue2", "l01_dialogue2_norm.png");
}

/// The hold-2 WORLD STATE against the real game's `dumpworld` (the whole
/// dismiss -> cutscene-advance -> second-op53 chain diffs byte-for-byte).
#[test]
fn l01_world_state_matches_the_real_game_at_dialogue2() {
    let s = shell_at_dialogue2();
    let fixture =
        std::fs::read_to_string(root().join("tests/fixtures/oracle/l01_world_dialogue2.txt"))
            .unwrap()
            .replace("\r\n", "\n");
    assert_eq!(
        world_dump(&s),
        fixture,
        "L01 world state differs from the real game at hold 2"
    );
}

/// FLOATING COMBAT TEXT at normalized-shot parity: at the settled hold both
/// sides install the same floats (the oracle injected them into the PAUSED
/// real game: slot 3 "12" — the plain red damage-number path — and slot 2
/// lang 471 "- Dodge - " — the green string-compared path), then shoot with
/// no tick in between. The paint's `Q == 0` branch initializes the rise
/// position and color on both sides (h.a draw, h.java:530).
#[test]
fn l01_floating_text_at_parity() {
    let mut s = shell_at_dialogue2();
    for (slot, text) in [(3usize, "12"), (2, "<471>")] {
        let a = s.world.actors[slot].as_mut().expect("anchor actor");
        a.floating_text = Some(text.into());
        a.q_field = 0;
        a.r_field = 0;
        a.var_short_h = 0;
    }
    assert_frame(&mut s, "l01_float", "l01_float_norm.png");
}

/// The PICKUP-PROXIMITY HINT anchor: both sides teleport the player (the real
/// op36 native `h.a(j,int,int)`) next to the op49 marker at tile (21,30) (the
/// neighboring scamps have E=1 — no aggro), re-follow the camera (the real
/// op26 native `b.b(int)` — the speaker becomes the player, so the dialogue's
/// first line loses its dark-red prefix, faithfully), then run live ticks: the
/// run()-loop proximity scan fires the lang-363 "Examine" HUD hint. The hint
/// BAND paints at y=323.. — inside the clipped 320..345 logical band, so it
/// is asserted as state; the frame pins the marker tile, the enemy health
/// bars, and the post-teleport camera.
#[test]
fn l01_pickup_hint_at_parity() {
    let mut s = shell_at_dialogue2();
    {
        let a = s.world.actors[0].as_mut().expect("player");
        formats::set_position(a, 2752, 3904);
    }
    s.world.camera_follow(0);
    for _ in 0..16 {
        s.tick(50); // ~800ms: the actor-loop proximity scan fires the hint
    }
    assert_eq!(
        s.world.hud.as_ref().map(|h| h.text.as_str()),
        Some("Examine"), // lang 363
        "the pickup-proximity hint is showing"
    );
    assert_frame(&mut s, "l01_pickup_hint", "l01_pickup_hint_norm.png");
}

/// The intro page (m=10) at a FIXED-SCROLL normalized shot (mirrors
/// `oracle/to_textpages.txt` -> `artifacts/textpages`): the page auto-scrolls
/// on the wall clock, so both sides pin `g:S = 180` (every intro line lands
/// on the visible LCD; the end condition stays false — final y 326 > the 305
/// limit) and shoot. First pixel gate for the mode-10 render — the parchment
/// body text; the m10 bottom bar (0xE9E1C3) + down arrow land in the clipped
/// 320..345 band.
#[test]
fn l01_intro_fixed_scroll_at_parity() {
    let mut s = shell_at_class_select();
    tap(&mut s, 53); // class fire
    for _ in 0..400 {
        s.tick(50);
        if s.mode() == 10 {
            break;
        }
    }
    assert_eq!(s.mode(), 10, "never reached the intro page");
    for _ in 0..40 {
        s.tick(50); // ~2s into the roll, like the oracle drive
    }
    assert_eq!(s.mode(), 10);
    s.set_scroll(180);
    let fb = assert_frame(&mut s, "l01_intro_g180", "l01_intro_g180_norm.png");
    // The m10 tail (parchment bar + the group-53 down arrow at final y 326 >
    // the 305 limit) lands wholly in the clipped 320..345 logical band — no
    // oracle can see it, so at least pin that the arrow INKED the band (the
    // bar itself matches the page fill, so only the arrow leaves a trace).
    let mut inked = 0;
    for y in game::paint::LCD_H..game::paint::SCREEN_H {
        for x in 0..game::paint::SCREEN_W {
            if fb.get(x, y) != 0xE9_E1_C3 {
                inked += 1;
            }
        }
    }
    assert!(
        inked > 0,
        "the m10 down arrow should ink the clipped band (tail liveness)"
    );
}

/// The intro text page is fed by op66 (`b.e(String)` = lang 445 via the L01
/// overlay lang table) and auto-scrolls without input.
#[test]
fn intro_text_appears_and_autoscrolls_unattended() {
    let mut s = shell_at_class_select();
    tap(&mut s, 53);
    let mut reached_intro = false;
    for _ in 0..4000 {
        s.tick(50);
        if s.screen() == Some(Screen::IntroText) {
            reached_intro = true;
        }
        if reached_intro && s.mode() == 0 {
            return; // scrolled through unattended
        }
    }
    panic!("the intro text never auto-scrolled into mode 0 (reached_intro={reached_intro})");
}
