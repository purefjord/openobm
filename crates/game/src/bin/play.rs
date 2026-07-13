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

fn conf() -> Conf {
    Conf {
        window_title: "The Elder Scrolls Travels: Oblivion — Rust port".into(),
        window_width: LCD_W * 2,
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
    // args (both optional, any order): a path = the port root (default =
    // this crate's ../..); a "/name.scr" = jump straight into that level
    // script after a fast boot (custom maps, e.g. /lush.scr).
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut jump_script: Option<String> = None;
    for arg in std::env::args().skip(1) {
        if arg.starts_with('/') && arg.ends_with(".scr") {
            jump_script = Some(arg);
        } else {
            root = PathBuf::from(arg);
        }
    }
    let masks = TextMasks::load(&root.join("tests/fixtures/oracle/text_masks.txt"))
        .expect("text masks fixture");
    let mut shell = Shell::boot(root.join("assets"), masks).expect("shell boot");
    if let Some(script) = jump_script {
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

    let mut image = Image::gen_image_color(LCD_W as u16, LCD_H as u16, BLACK);
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

        let fb = shell.render().expect("paint");
        let px = fb.pixels();
        let data = image.get_image_data_mut();
        for y in 0..LCD_H as usize {
            let row = y * fb.w as usize;
            for x in 0..LCD_W as usize {
                let rgb = px[row + x];
                data[y * LCD_W as usize + x] =
                    [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 0xFF];
            }
        }
        texture.update(&image);

        // integer-ish scale to the window, centered, aspect kept
        clear_background(BLACK);
        let scale = (screen_width() / LCD_W as f32).min(screen_height() / LCD_H as f32);
        let (dw, dh) = (LCD_W as f32 * scale, LCD_H as f32 * scale);
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
