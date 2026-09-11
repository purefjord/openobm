//! `play` — the interactive window for the Rust port: the byte-validated
//! shell driven by your keyboard in real time. Build/run:
//!
//!     cargo run -p game --features interactive --bin play --release
//!
//! Controls (the MIDP keys the real game reads):
//!   arrows            move / menu left-right / scroll   (keys 2/4/6/8)
//!   Enter or Space    FIRE (select, attack, dismiss dialogue)  (key 5)
//!   Q                 LEFT soft key  (BACK / NO / close)
//!   W                 RIGHT soft key (action menu in-game / YES)
//!   0-9               the number keys (quick heal 7, quick magicka 9,
//!                     toggle attack 3 — rebindable in Custom Controls)
//!   Escape            quit
//!
//! The shell is the same code the parity suites gate byte-for-byte against
//! the real jar on FreeJ2ME; this frontend only forwards keys, ticks the
//! real frame dt, and blits the 240x320 LCD window (the logical frame is
//! 345 tall; the bottom 25 rows are clipped on any real device).

use game::paint::LCD_H;
use game::shell::Shell;
use game::text::TextMasks;
use macroquad::prelude::*;
use std::path::PathBuf;

const LCD_W: i32 = 240;

struct Args {
    root: PathBuf,
    /// "/name.scr" — jump straight into a level script after a fast boot.
    jump: Option<String>,
    /// Logical viewport width for the NON-CANONICAL widescreen viewer
    /// (`wide` = 16:9, `wide10` = 16:10 at the LCD's 320 height). None =
    /// the pure 240x320 frame only. Menus, dialogues and boundaries always
    /// fall back to the canonical frame, pillarboxed.
    wide: Option<i32>,
}

fn parse_args() -> Args {
    let mut a = Args {
        root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        jump: None,
        wide: None,
    };
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "wide" | "wide9" => a.wide = Some(LCD_H * 16 / 9), // 568
            "wide10" => a.wide = Some(LCD_H * 16 / 10),        // 512
            s if s.starts_with('/') && s.ends_with(".scr") => a.jump = Some(s.to_string()),
            s => a.root = PathBuf::from(s),
        }
    }
    a
}

fn conf() -> Conf {
    let view_w = parse_args().wide.unwrap_or(LCD_W);
    Conf {
        window_title: "The Elder Scrolls Travels: Oblivion — Rust port".into(),
        window_width: view_w * 2,
        window_height: LCD_H * 2,
        window_resizable: true,
        ..Default::default()
    }
}

/// KeyCode -> the MIDP keycode the shell's `b(J)` accepts.
fn midp_key(k: KeyCode) -> Option<i32> {
    Some(match k {
        KeyCode::Up => 50,
        KeyCode::Down => 56,
        KeyCode::Left => 52,
        KeyCode::Right => 54,
        KeyCode::Enter | KeyCode::Space => 53,
        KeyCode::Q => 21,
        KeyCode::W => 22,
        KeyCode::Key0 => 48,
        KeyCode::Key1 => 49,
        KeyCode::Key2 => 50,
        KeyCode::Key3 => 51,
        KeyCode::Key4 => 52,
        KeyCode::Key5 => 53,
        KeyCode::Key6 => 54,
        KeyCode::Key7 => 55,
        KeyCode::Key8 => 56,
        KeyCode::Key9 => 57,
        _ => return None,
    })
}

#[macroquad::main(conf)]
async fn main() {
    // args (all optional, any order): a path = the port root; "/name.scr"
    // = jump into that level after a fast boot (custom maps, e.g.
    // /lush.scr); "wide"/"wide10" = the non-canonical widescreen viewer.
    let args = parse_args();
    let masks = TextMasks::bundled();
    let mut shell = Shell::boot(args.root.join("assets"), masks).expect("shell boot");

    // RecordStore persistence (frontend-owned; the shell stays pure): the
    // `ESO` record lives in playdata/eso.bin. Install = the ctor's b(false).
    // DISABLED for /x.scr jump sessions — a custom-map save must not clobber
    // the real playthrough's record, and a present save would divert the
    // blind pre-roll taps into the New-Game overwrite confirm (m16).
    let persist = args.jump.is_none();
    let save_path = args.root.join("playdata/eso.bin");
    if persist {
        if let Some(blob) = game::save::read_save_file(&save_path) {
            // The file is user-editable: `read_save_file` gates its structure,
            // `install_save` its semantics. A record that clears the first but
            // fails the second is treated exactly like a corrupt one — the
            // wiped-RMS baseline (no save) — with a loud note, never a crash.
            if let Err(e) = shell.install_save(blob) {
                eprintln!(
                    "warning: ignoring the saved game at {} — {e}",
                    save_path.display()
                );
            }
        }
    }
    let mut written: Option<Vec<u8>> = shell.save_blob().map(<[u8]>::to_vec);

    if let Some(script) = &args.jump {
        // The proven fast pre-roll (logos -> title -> menu -> class fire),
        // then the op29 native jumps to the requested script.
        game::script::drive(
            &mut shell,
            "timescale 10\nwait 5000\ntap fire\nwait 1000\ntap fire\nwait 500\ntap fire\nwait 20000\n",
        )
        .expect("boot pre-roll");
        game::script::drive(&mut shell, &format!("callscript {script}\nwait 3000\n"))
            .expect("jump script");
    }

    let view_w = args.wide.unwrap_or(LCD_W);
    let mut image = Image::gen_image_color(view_w as u16, LCD_H as u16, BLACK);
    let texture = Texture2D::from_image(&image);
    texture.set_filter(FilterMode::Nearest);

    loop {
        if is_key_pressed(KeyCode::Escape) || shell.exited() {
            break;
        }
        // keyPressed: the canvas latches ONE key (last press wins);
        // keyReleased (any key) sets the release flag — held keys keep
        // re-dispatching per frame, exactly like the real b(J).
        for k in get_keys_pressed() {
            if let Some(code) = midp_key(k) {
                shell.hold(code);
            }
        }
        if get_keys_released()
            .into_iter()
            .any(|k| midp_key(k).is_some())
        {
            shell.release();
        }

        // At an unported content boundary (an opcode the port hasn't reached
        // yet) the VM has halted: keep painting the last live frame, but stop
        // ticking gameplay and show an honest overlay instead of a hard crash.
        let boundary = shell.unported_boundary();
        if boundary.is_none() {
            // run(): dt is wall-clock; clamp a stall (window drag etc.) so
            // one monster tick can't fast-forward timers unrealistically.
            let dt = (get_frame_time() * 1000.0) as i32;
            shell.tick(dt.clamp(1, 250));
        }

        // Mirror the ESO slot to disk when g() rewrote it (RecordStore
        // semantics). A failed write (AV/indexer holding the file) is
        // skipped and retried next frame — never a crash.
        if persist && shell.save_blob() != written.as_deref() {
            if let Some(blob) = shell.save_blob() {
                if game::save::write_save_file(&save_path, blob).is_ok() {
                    written = Some(blob.to_vec());
                }
            }
        }

        // The wide viewer only composites clean gameplay; menus, dialogues
        // and boundary screens use the canonical byte-gated frame,
        // pillarboxed into the wide window.
        let wide_fb = if args.wide.is_some()
            && boundary.is_none()
            && shell.mode() == 0
            && shell.world.dialogue.is_none()
        {
            shell.render_wide(view_w, LCD_H).ok()
        } else {
            None
        };
        let data = image.get_image_data_mut();
        match wide_fb {
            Some(fb) => {
                let px = fb.pixels();
                for y in 0..LCD_H as usize {
                    let row = y * fb.w as usize;
                    for x in 0..view_w as usize {
                        let rgb = px[row + x];
                        data[y * view_w as usize + x] =
                            [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 0xFF];
                    }
                }
            }
            None => {
                let fb = shell.render().expect("paint");
                let px = fb.pixels();
                let x0 = ((view_w - LCD_W) / 2) as usize;
                for y in 0..LCD_H as usize {
                    let row = y * fb.w as usize;
                    for x in 0..view_w as usize {
                        let rgb = if x >= x0 && x < x0 + LCD_W as usize {
                            px[row + (x - x0)]
                        } else {
                            0
                        };
                        data[y * view_w as usize + x] =
                            [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 0xFF];
                    }
                }
            }
        }
        texture.update(&image);

        // integer-ish scale to the window, centered, aspect kept
        clear_background(BLACK);
        let scale = (screen_width() / view_w as f32).min(screen_height() / LCD_H as f32);
        let (dw, dh) = (view_w as f32 * scale, LCD_H as f32 * scale);
        let (ox, oy) = ((screen_width() - dw) / 2.0, (screen_height() - dh) / 2.0);
        draw_texture_ex(
            &texture,
            ox,
            oy,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(dw, dh)),
                ..Default::default()
            },
        );
        if let Some(msg) = boundary {
            // a dim scrim + the honest boundary message over the last frame
            draw_rectangle(ox, oy, dw, dh, Color::new(0.0, 0.0, 0.0, 0.72));
            let lines = [
                msg.as_str(),
                "The engine is complete; this level room",
                "is the next thing to port.",
                "",
                "Esc to quit.",
            ];
            let fs = (dh * 0.045).max(14.0);
            for (i, line) in lines.iter().enumerate() {
                let d = measure_text(line, None, fs as u16, 1.0);
                draw_text(
                    line,
                    ox + (dw - d.width) / 2.0,
                    oy + dh * 0.4 + i as f32 * fs * 1.4,
                    fs,
                    WHITE,
                );
            }
        }
        next_frame().await;
    }
}
