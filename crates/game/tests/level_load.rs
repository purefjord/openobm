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

/// ONE script, BOTH sides: `oracle/to_l01_fast.txt` (the 10x-timescale L01
/// drive) executed verbatim through the shared [`game::script::drive`]
/// runner; every artifact it names (the normalized shot + both dumps at the
/// first-dialogue hold) must equal the committED fixtures — which the REAL
/// jar reproduced byte-for-byte when OracleRun ran the same file (see the
/// script's header). Timescale semantics: the runner ticks `wait x scale`
/// GAME-ms; the oracle's waits are real ms against its scaled clock.
#[test]
fn unified_l01_fast_drive_matches_the_oracle() {
    let script =
        std::fs::read_to_string(root().join("oracle/to_l01_fast.txt")).expect("drive script");
    let mut s = boot_shell();
    let artifacts = game::script::drive(&mut s, &script).expect("drive");
    let fixture = |name: &str| {
        std::fs::read_to_string(root().join("tests/fixtures/oracle").join(name))
            .unwrap()
            .replace("\r\n", "\n")
    };
    match &artifacts["f_world.txt"] {
        game::script::Artifact::Text(t) => {
            assert_eq!(t, &fixture("l01_world_dialogue1.txt"), "world dump")
        }
        _ => panic!("f_world.txt is a text artifact"),
    }
    match &artifacts["f_layers.txt"] {
        game::script::Artifact::Text(t) => {
            assert_eq!(t, &fixture("l01_layers_dialogue1.txt"), "layers dump")
        }
        _ => panic!("f_layers.txt is a text artifact"),
    }
    match &artifacts["f0_dialogue1.png"] {
        game::script::Artifact::Frame(fb) => {
            let real = game::fb::Fb::load_png(
                &root().join("tests/fixtures/oracle/frames/l01_dialogue1_norm.png"),
            )
            .unwrap();
            let (_, bad) = fb.diff_region(&real, game::paint::LCD_H);
            assert_eq!(bad, 0, "normalized shot differs");
        }
        _ => panic!("f0_dialogue1.png is a frame artifact"),
    }
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
    let out = game::dump::layers_dump(&s);
    let fixture =
        std::fs::read_to_string(root().join("tests/fixtures/oracle/l01_layers_dialogue1.txt"))
            .unwrap()
            .replace("\r\n", "\n");
    assert_eq!(out, fixture, "L01 layer state differs from the real game");
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
        game::dump::world_dump(&s),
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
        game::dump::world_dump(&s),
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

/// Drive from the class fire into the fight and on to the player's death:
/// FIRE taps spaced ~4s dismiss the cutscene dialogues (and swing at air —
/// harmless), then the scripted assassination fight plays out until
/// `b.void_a(0)` fires the death sequence -> mode 11. The fight itself is
/// RNG-phased (our fixed seed differs from the real game's wall seed), but
/// the death screen's paint reads NO world state, so both sides converge on
/// the same frame.
fn shell_at_death() -> Shell {
    let mut s = shell_at_class_select();
    tap(&mut s, 53); // class fire
    let mut next_fire = 0;
    for n in 0..12000 {
        s.tick(50);
        if s.mode() == 11 {
            return s;
        }
        // PASSIVE play: FIRE only dismisses dialogues (fighting back can win
        // the fight instead — the unattended player is scripted to lose).
        if s.mode() == 0 && s.world.dialogue.is_some() && n >= next_fire {
            s.press(53);
            next_fire = n + 40; // dismissal taps ~2s apart
        }
    }
    panic!("the L01 fight never reached the mode-11 death screen");
}

/// The player-death screen (m=11) at screenshot parity, plus the death
/// sequence itself: the player is re-initialized (`h.a(j)`) and teleported to
/// the op71 respawn anchor BEFORE the screen shows; YES (a:B=22) resumes
/// gameplay with that respawned player.
#[test]
fn player_death_screen_at_parity() {
    let mut s = shell_at_death();
    assert_eq!(s.screen(), Some(Screen::Death));
    {
        let p = s.world.actors[0].as_ref().expect("the slot is NOT nulled");
        assert_eq!(p.var_short_q, p.var_short_o, "h.a(j) refilled health");
        assert_eq!(
            [p.var_int_arr_b[0] as i16, p.var_int_arr_b[1] as i16],
            [s.world.respawn[0], s.world.respawn[1]],
            "teleported to the op71 respawn anchor"
        );
        assert!(
            s.world.hud.is_none(),
            "b.a(null,0,0,0) cleared the HUD text"
        );
    }
    // The static screen (black + lang 428 + the clipped NO/YES row).
    let fb = s.render().expect("death paint");
    let out = root().join("target/parity");
    std::fs::create_dir_all(&out).unwrap();
    fb.save_png(&out.join("death_rust.png")).unwrap();
    let real =
        game::fb::Fb::load_png(&root().join("tests/fixtures/oracle/frames/death_continue.png"))
            .unwrap();
    let (diff, bad) = fb.diff_region(&real, game::paint::LCD_H);
    if bad != 0 {
        diff.save_png(&out.join("death_diff.png")).unwrap();
    }
    assert_eq!(bad, 0, "death screen differs from the real shot");
    // YES (a:B = 22) -> mode 0 (the oracle modelog pins the same 11 -> 0).
    tap(&mut s, 22);
    assert_eq!(s.mode(), 0, "Continue? YES resumes gameplay");
    assert!(s.world.actors[0].is_some());
}

/// Death-screen NO (b:B = 21): `p()` zeroes every cursor and lands on the
/// in-game pause page (f:Z is set — we left gameplay), mode 3.
#[test]
fn death_screen_no_returns_to_the_pause_menu() {
    let mut s = shell_at_death();
    tap(&mut s, 21);
    assert_eq!(s.screen(), None); // mode 3, page 5 — the pause-style menu
    assert_eq!(s.mode(), 3);
}

/// The quick heal key (bound key 7 = code 55 -> remap 0 -> `h.a(j, true)`):
/// with no ARMED potion the key is a faithful no-op (the Monk's starting
/// kind-2 items are quest keys — `row[2] == 0` never arms `var_int_arr_f`);
/// once a real potion row is equipped (the exact path a potion pickup takes),
/// the key consumes it — health restored (clamped), the 0x2xx inventory
/// entry removed, and `arr_f` left unarmed (no other positive-restore
/// consumable remains). The key is consumed before the VM tail (the 3615
/// sentinel check).
#[test]
fn quick_heal_consumes_an_armed_potion() {
    let mut s = shell_at_class_select();
    tap(&mut s, 53);
    // Run into the fight until the player is damaged but alive, with input
    // unlocked and no dialogue (the quick keys sit behind the same guard as
    // movement).
    let mut next_fire = 0;
    let mut ready = false;
    for n in 0..12000 {
        s.tick(50);
        if s.mode() != 0 {
            if s.mode() == 11 {
                break;
            }
            continue;
        }
        if s.world.dialogue.is_some() && n >= next_fire {
            s.press(53);
            next_fire = n + 40;
        }
        let damaged = s.world.actors[0]
            .as_ref()
            .is_some_and(|p| p.var_short_q < p.var_short_o && p.var_short_q > 0);
        if damaged && s.world.dialogue.is_none() && s.world.input_unlocked {
            ready = true;
            break;
        }
    }
    assert!(ready, "never reached a damaged-player hold in the fight");
    // Unarmed: the starting quest keys never armed arr_f — the key no-ops.
    let potions = |p: &formats::Actor| p.var_int_arr_k.iter().filter(|&&e| (e >> 8) == 2).count();
    {
        let p = s.world.actors[0].as_ref().unwrap();
        assert!(p.var_int_arr_f.is_none(), "start items must not arm arr_f");
    }
    let before = potions(s.world.actors[0].as_ref().unwrap());
    tap(&mut s, 55); // the quick-health binding (g:[B[0] = key '7')
    assert_eq!(
        potions(s.world.actors[0].as_ref().unwrap()),
        before,
        "no armed potion -> faithful no-op"
    );
    // Arm a real potion (subtype-2 row 1: +50 health, row[5] == 0) the way a
    // pickup would, then use it.
    let row = s.tables().row(2, 1).expect("potion row").to_vec();
    let tables = s.tables().clone();
    {
        let p = s.world.actors[0].as_mut().unwrap();
        p.equip(2, &row, false, &tables);
        assert!(p.var_int_arr_f.is_some(), "the potion row arms arr_f");
    }
    let (hp, max, count) = {
        let p = s.world.actors[0].as_ref().unwrap();
        (p.var_short_q, p.var_short_o, potions(p))
    };
    tap(&mut s, 55);
    let p = s.world.actors[0].as_ref().unwrap();
    assert_eq!(potions(p), count - 1, "one consumable removed");
    assert!(p.var_int_arr_f.is_none(), "no other potion to re-arm");
    assert_eq!(
        i32::from(p.var_short_q),
        i32::from(max).min(i32::from(hp) + row[2]),
        "health restored by the potion row, clamped"
    );
}

/// The mode-2 ACTION MENU at screenshot parity: `oracle/to_menu.txt` (one
/// script, both sides, timescale 10) opens the menu at the second-dialogue
/// hold via the sethud inject + key 22, walks all four tabs, descends into
/// the Body armor page, and uses a Remove Poison from the Items page. The
/// menu freezes the world and covers the whole screen, so plain shots are
/// byte-comparable. Also asserts the activation side effects: the potion is
/// consumed from the ACTOR (the menu graph is a snapshot — the row still
/// shows, checkmarked), and BACK at a top page closes to mode 0.
#[test]
fn action_menu_at_parity() {
    let script = std::fs::read_to_string(root().join("oracle/to_menu.txt")).expect("drive script");
    let mut s = boot_shell();
    let artifacts = game::script::drive(&mut s, &script).expect("drive");
    for name in [
        "m0_attack.png",
        "m1_armor.png",
        "m2_body.png",
        "m3_items.png",
        "m4_stats.png",
        "m5_items_used.png",
    ] {
        let game::script::Artifact::Frame(fb) = &artifacts[name] else {
            panic!("{name} is a frame artifact");
        };
        let real = game::fb::Fb::load_png(
            &root().join(format!("tests/fixtures/oracle/frames/menu_{name}")),
        )
        .unwrap();
        let (diff, bad) = fb.diff_region(&real, game::paint::LCD_H);
        if bad != 0 {
            let out = root().join("target/parity");
            std::fs::create_dir_all(&out).unwrap();
            fb.save_png(&out.join(format!("menu_{name}"))).unwrap();
            diff.save_png(&out.join(format!("menu_diff_{name}")))
                .unwrap();
        }
        assert_eq!(bad, 0, "{name} differs from the real menu shot");
    }
    // The Items fire consumed one Remove Poison from the actor.
    let p = s.world.actors[0].as_ref().unwrap();
    let potions = p.var_int_arr_k.iter().filter(|&&e| (e >> 8) == 2).count();
    assert_eq!(potions, 2, "one of the three Remove Poison was consumed");
    // BACK at the (descended-from) Items page pops... the drive left the
    // menu on the Items page; two BACKs close it to mode 0.
    assert_eq!(s.screen(), Some(Screen::ActionMenu));
    tap(&mut s, 21);
    assert_eq!(s.mode(), 0, "BACK at a top page closes the menu");
    assert!(!s.fmenu.open);
}

/// SAVE PARITY (the write path): drive to the L01 second-dialogue hold and
/// invoke `b.g()` (`save_and_get_blob`); the shell's ESO record must equal
/// the REAL game's captured blob byte-for-byte (`eso_l01_dialogue2.bin`,
/// which `oracle/to_save.txt` captured via the callsave native at the SAME
/// hold). Every field feeding the save (progress flags, level-script name,
/// the player actor with its recomputed item `active` bits, gold) is settled
/// there, so one byte fails this. Closes the M9 loop from BOTH ends: the
/// deserializer round-trips a real blob (`save_roundtrips_the_real_blob`),
/// and our serializer reproduces one from live state.
#[test]
fn save_blob_matches_the_real_game() {
    let mut s = shell_at_dialogue2();
    let blob = s.save_and_get_blob();
    let real = std::fs::read(root().join("tests/fixtures/oracle/eso_l01_dialogue2.bin"))
        .expect("the captured real blob");
    assert_eq!(
        blob, real,
        "the shell's b.g() save differs from the real game's ESO record"
    );
}

/// The save/load CONFIRM screens at screenshot parity. The three paints read
/// NO world state (like the exit/death dialogs), so a fresh boot with a save
/// present renders them exactly. We reach them the same way the real captures
/// did: save at the L01 hold (so `boolean_b()` is true), open the in-game
/// pause menu, and pick New Game -> m16 "Saved Game Exists / Overwrite?" and
/// Load Game -> m14 "Load Saved Game?". The real fixtures were shot from the
/// MAIN menu, but the paints are menu-independent, so they match.
#[test]
fn save_load_confirm_screens_at_parity() {
    let assert_frame = |s: &mut Shell, fixture: &str, label: &str| {
        let fb = s.render().expect("confirm paint");
        let real =
            game::fb::Fb::load_png(&root().join(format!("tests/fixtures/oracle/frames/{fixture}")))
                .unwrap();
        let (diff, bad) = fb.diff_region(&real, game::paint::LCD_H);
        if bad != 0 {
            let out = root().join("target/parity");
            std::fs::create_dir_all(&out).unwrap();
            fb.save_png(&out.join(format!("{label}_rust.png"))).unwrap();
            diff.save_png(&out.join(format!("{label}_diff.png")))
                .unwrap();
        }
        assert_eq!(bad, 0, "{label} differs from the real shot");
    };
    let mut s = shell_at_dialogue2();
    let _ = s.save_and_get_blob(); // boolean_b() true from here on
                                   // Pause menu (key 21): with a save, page 5 =
                                   // [Continue, New Game, Load Game, Help, About, Exit].
    tap(&mut s, 21);
    assert_eq!(s.mode(), 3);
    // New Game (index 1) -> the overwrite confirm.
    tap(&mut s, 54);
    tap(&mut s, 53);
    assert_eq!(s.screen(), Some(Screen::OverwriteConfirm));
    assert_frame(&mut s, "overwrite_confirm.png", "overwrite");
    tap(&mut s, 21); // NO -> back to the pause menu, cursor still on New Game
    assert_eq!(s.mode(), 3);
    // Load Game is one item right of New Game (index 1 -> 2).
    tap(&mut s, 54);
    tap(&mut s, 53);
    assert_eq!(s.screen(), Some(Screen::LoadConfirm));
    assert_frame(&mut s, "load_confirm.png", "load");
}

/// The "Game Saved" screen (m13) at screenshot parity: fire Save Game (only
/// reachable via the script-gated page-4 in-game menu in normal play, so the
/// test drives it directly through the menu machinery by seeding page 4).
#[test]
fn game_saved_screen_at_parity() {
    let mut s = shell_at_dialogue2();
    s.enter_save_screen_for_test(); // g() + mode 13
    assert_eq!(s.screen(), Some(Screen::GameSaved));
    let fb = s.render().expect("game-saved paint");
    let real = game::fb::Fb::load_png(&root().join("tests/fixtures/oracle/frames/game_saved.png"))
        .unwrap();
    let (diff, bad) = fb.diff_region(&real, game::paint::LCD_H);
    if bad != 0 {
        let out = root().join("target/parity");
        std::fs::create_dir_all(&out).unwrap();
        fb.save_png(&out.join("game_saved_rust.png")).unwrap();
        diff.save_png(&out.join("game_saved_diff.png")).unwrap();
    }
    assert_eq!(bad, 0, "the Game Saved screen differs from the real shot");
}

/// SAVE -> LOAD through the menu: save at the hold, corrupt the live player,
/// open the pause menu, pick "Load Game" -> confirm YES. The load re-runs the
/// loader on the stored script name and installs the RESTORED player as
/// `var_j_a` (slot 0). Assert the restored player state right after the load
/// (mode 6, before the L01 choreography re-equips) matches the save: the same
/// class/stats/health and the saved inventory tags. (A save-load-save byte
/// round-trip would NOT hold — the reloaded level re-runs its equip opcodes on
/// the reused player, adding items, exactly as the real game does.)
#[test]
fn save_then_load_restores_the_player() {
    let mut s = shell_at_dialogue2();
    let saved = s.save_and_get_blob();
    let want = formats::parse_save(&saved).unwrap().player.unwrap().actor;
    // Corrupt the live player so a stale-state load would be visible.
    {
        let p = s.world.actors[0].as_mut().unwrap();
        p.var_short_q = 3;
        p.var_short_s = 99;
    }
    s.world.gold = 5;
    // Open the in-game pause menu (key 21); page 5 with a save = [Continue,
    // New Game, Load Game, Help, About, Exit] — two RIGHTs land on Load Game.
    tap(&mut s, 21);
    assert_eq!(s.mode(), 3, "pause menu");
    tap(&mut s, 54);
    tap(&mut s, 54);
    tap(&mut s, 53); // fire Load Game -> the m14 confirm
    assert_eq!(s.screen(), Some(Screen::LoadConfirm));
    tap(&mut s, 22); // YES -> h() load (synchronous)
    assert_eq!(s.mode(), 6, "load enters the loader");
    let p = s.world.actors[0]
        .as_ref()
        .expect("restored player in slot 0");
    assert_eq!(p.var_byte_f, want.var_byte_f, "class restored");
    assert_eq!(p.var_short_s, want.var_short_s, "strength restored");
    // (health is refilled later by the spawner's player_reset, not by restore)
    assert_eq!(s.world.gold, i32::from(want.global_int_b), "gold restored");
    let inv: Vec<i32> = p
        .var_int_arr_k
        .iter()
        .take_while(|&&v| v != 0)
        .copied()
        .collect();
    assert_eq!(inv.len(), want.items.len(), "saved inventory tags restored");
}

/// Attack-page activation (`b.a(c)` -> `h.a(j,String)`): firing a spell arms
/// `var_int_arr_m`, checkmarks the spell node, and RE-MARKS the active
/// weapon (`b.var_c_a.var_boolean_a = true` — both stay checked); the
/// weapon stays equipped (`var_byte_j` untouched).
#[test]
fn action_menu_spell_activation_cross_marks() {
    let script = "\
        timescale 10\n\
        wait 5000\n\
        tap fire\n\
        wait 1000\n\
        tap fire\n\
        wait 500\n\
        tap fire\n\
        wait 20000\n\
        tap fire\n\
        wait 1000\n\
        sethud 1\n\
        tap 22\n\
        wait 200\n\
        tap down\n\
        wait 200\n\
        tap fire\n\
        wait 200\n";
    let mut s = boot_shell();
    game::script::drive(&mut s, script).expect("drive");
    assert_eq!(s.screen(), Some(Screen::ActionMenu));
    let p = s.world.actors[0].as_ref().unwrap();
    let arm = p.var_int_arr_m.as_ref().expect("spell armed");
    assert_eq!(
        s.tables().row(8, arm[0]).map(|r| r[0]),
        Some(arm[0]),
        "arr_m holds a subtype-8 spell row"
    );
    assert_eq!(p.var_byte_j, 7, "the equipped weapon is untouched");
    let checked: Vec<&str> = s
        .fmenu
        .items
        .iter()
        .filter(|i| i.active)
        .map(|i| i.name.as_str())
        .collect();
    assert!(
        checked.contains(&"Weapon: Iron Club") && checked.contains(&"Spell: Shield"),
        "both the weapon and the fired spell show checkmarks: {checked:?}"
    );
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
