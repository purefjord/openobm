//! The mode-18 overview stat tables — `b.java`'s six memoized `[[String`
//! builders (CFR b.java 2261–2493, javap 13821–15554; the CFR here is clean,
//! cross-checked against javap for field writes). Each builder walks the
//! script VM's sized section tables (`e.c/d/e/k/h` + the subtype-5 aux
//! `i`/`j` lists) and formats display rows; results are cached until `k()`
//! (Load-Game fire / class fire) clears them.
//!
//! The original rows are FIXED-LENGTH arrays with null tails; every append is
//! contiguous, so a `Vec` of the non-null prefix is equivalent (the paint
//! stops at the first null / the Vec end). Several locals (`school`,
//! `target`, the permission tag) are declared OUTSIDE the row loop in the
//! original and keep their previous value when a row's discriminant matches
//! nothing — transcribed faithfully.

use crate::vm::GameVm;
use formats::lang::Lang;

/// The per-builder caches (`b.var_java_lang_String_arr_arr_c..h`), memoized
/// across menu visits; `k()` nulls them all.
#[derive(Default)]
pub struct StatCaches {
    pub weapons: Option<Vec<Vec<String>>>,  // c:[[String — f()
    pub armor: Option<Vec<Vec<String>>>,    // d:[[String — e()
    pub spells: Option<Vec<Vec<String>>>,   // e:[[String — d()
    pub items: Option<Vec<Vec<String>>>,    // f:[[String — c()
    pub classes: Option<Vec<Vec<String>>>,  // g:[[String — b()
    pub overview: Option<Vec<Vec<String>>>, // h:[[String — a()
}

impl StatCaches {
    /// `b.k()` — null the b..h tables.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// `e.java_lang_String_a(int)` — a `0xF___`-marked id resolves through the
/// lang table, anything else is a string-pool index.
fn e_name(lang: &Lang, vm: &GameVm, id: i32) -> String {
    if (id & 0xF000) == 0xF000 {
        lang.get((id & 0xFFF) as u16).to_string()
    } else {
        vm.pool_string(id).to_string()
    }
}

/// `e.b(int)` (static) — skill-tag name: lang(523 + tag) for 0..=14, null
/// beyond (`"   " + null` renders the literal "null" in Java — kept).
fn skill_name(lang: &Lang, tag: i32) -> String {
    match tag {
        0..=14 => lang.get((523 + tag) as u16).to_string(),
        _ => "null".to_string(),
    }
}

/// `a()` — the Game Overview table: `[[lang 574]]` (built by the lang-573
/// dispatch alongside mode 23, unused by the text-page paint — kept for
/// state fidelity).
pub fn overview(lang: &Lang) -> Vec<Vec<String>> {
    vec![vec![lang.get(574).to_string()]]
}

/// `b()` — the Classes Overview: name, the six ×3 attributes, starting
/// weapon/armor, the aux-i skill list, the aux-j spell list.
pub fn classes(lang: &Lang, vm: &GameVm) -> Vec<Vec<String>> {
    let h = vm.tables.rows(5);
    let weapons = vm.tables.rows(4);
    let armor = vm.tables.rows(1);
    let spells = vm.tables.rows(8);
    let aux_i = vm.tables.class_aux_i_rows();
    let aux_j = vm.tables.class_aux_j_rows();
    let mut out = Vec::new();
    for (r, row) in h.iter().enumerate().skip(1) {
        let mut v = vec![format!("{}{}", lang.get(481), e_name(lang, vm, row[1]))];
        for (lid, col) in [
            (415u16, 7),
            (416, 8),
            (417, 9),
            (418, 10),
            (419, 11),
            (420, 12),
        ] {
            v.push(format!("{}: {}", lang.get(lid), row[col] * 3));
        }
        v.push(format!(
            "{}{}",
            lang.get(305),
            e_name(lang, vm, weapons[row[4] as usize][1])
        ));
        v.push(format!(
            "{}{}",
            lang.get(499),
            e_name(lang, vm, armor[row[5] as usize][1])
        ));
        v.push(lang.get(538).to_string()); // "Skills: "
        for &tag in aux_i[r].iter().take(15).take_while(|&&t| t != -1) {
            v.push(format!("   {}", skill_name(lang, tag)));
        }
        v.push(lang.get(539).to_string()); // "Spells: "
                                           // The loop bound is the aux-j TABLE's row count, not the row length
                                           // (original quirk: `n4 < e.j.length` on a [rows][15] array).
        for (n4, &sp) in aux_j[r].iter().take(aux_j.len()).enumerate() {
            if sp == -1 {
                if n4 == 0 {
                    v.push(format!("   {}", lang.get(572))); // "None"
                }
                break;
            }
            v.push(format!("   {}", e_name(lang, vm, spells[sp as usize][1])));
        }
        // new String[30] — overflow would AIOOBE in the original.
        assert!(v.len() <= 30, "class overview row exceeds the [30] alloc");
        out.push(v);
    }
    out
}

/// `c()` — the Items Overview: buy/sell values + the conditional stat lines.
pub fn items(lang: &Lang, vm: &GameVm) -> Vec<Vec<String>> {
    let e = vm.tables.rows(2);
    let mut out = Vec::new();
    for row in e.iter().skip(1) {
        let mut v = vec![
            format!("{}{}", lang.get(481), e_name(lang, vm, row[1])),
            format!("{}{}", lang.get(484), row[13]),
            format!("{}{}", lang.get(485), row[13] >> 2),
        ];
        for (lid, col) in [
            (496u16, 2),
            (497, 3),
            (498, 6),
            (499, 7),
            (500, 8),
            (501, 10),
        ] {
            if row[col] > 0 {
                v.push(format!("{}{}", lang.get(lid), row[col]));
            }
        }
        if row[5] > 0 {
            v.push(format!(
                "{}{}{}",
                lang.get(502),
                row[5] / 1000,
                lang.get(521)
            ));
        }
        assert!(v.len() <= 10, "item overview row exceeds the [10] alloc");
        out.push(v);
    }
    out
}

/// `d()` — the Spells Overview: fixed 17-line rows (school/target names by
/// discriminant; unmatched values keep the PREVIOUS row's string — original
/// leak, `String string = null` outside the loop).
pub fn spells(lang: &Lang, vm: &GameVm) -> Vec<Vec<String>> {
    let k = vm.tables.rows(8);
    let mut out = Vec::new();
    let mut school = "null".to_string();
    let mut target = "null".to_string();
    for row in k.iter().skip(1) {
        match row[2] {
            0 | 1 => school = lang.get(437).to_string(), // both map to "Armor Adj."
            3 => school = lang.get(439).to_string(),
            6 => school = lang.get(503).to_string(),
            5 => school = lang.get(433).to_string(),
            4 => school = lang.get(504).to_string(),
            2 => school = lang.get(505).to_string(),
            _ => {}
        }
        match row[7] {
            0 => target = lang.get(506).to_string(),
            1 => target = lang.get(507).to_string(),
            2 => target = lang.get(508).to_string(),
            3 => target = lang.get(509).to_string(),
            4 => target = lang.get(510).to_string(),
            _ => {}
        }
        out.push(vec![
            format!("{}{}", lang.get(481), e_name(lang, vm, row[1])),
            format!("{}{}", lang.get(511), school),
            lang.get(512).to_string(), // "Upgrade Level: "
            format!("{}{}", lang.get(513), row[8]),
            format!("{}{}", lang.get(514), row[9]),
            format!("{}{}", lang.get(515), row[10]),
            lang.get(516).to_string(), // "Magnitude: "
            format!("{}{}", lang.get(513), row[3].abs()),
            format!("{}{}", lang.get(514), row[4].abs()),
            format!("{}{}", lang.get(515), row[5].abs()),
            lang.get(517).to_string(), // "Magicka Cost: "
            format!("{}{}", lang.get(513), row[11]),
            format!("{}{}", lang.get(514), row[12]),
            format!("{}{}", lang.get(515), row[13]),
            format!("{}{}{}", lang.get(518), row[6] / 1000, lang.get(521)),
            format!("{}{}", lang.get(519), target),
            format!("{}{}", lang.get(520), row[14]),
        ]);
    }
    out
}

/// `e()` — the Armor Overview: 7 fixed lines + the allowed-class list
/// (`e.boolean_a(h[n][0], perm)`, iterating class rows FROM 0).
pub fn armor(lang: &Lang, vm: &GameVm) -> Vec<Vec<String>> {
    let d = vm.tables.rows(1);
    let h = vm.tables.rows(5);
    let mut out = Vec::new();
    let mut slot = "null".to_string();
    let mut kind = "null".to_string();
    let mut perm = 0i32;
    for row in d.iter().skip(1) {
        match row[3] {
            0 => slot = lang.get(28).to_string(), // Arms
            1 => slot = lang.get(29).to_string(), // Body
            2 => slot = lang.get(30).to_string(), // Feet
            7 => slot = lang.get(35).to_string(), // Finger
            3 => slot = lang.get(31).to_string(), // Hands
            4 => slot = lang.get(32).to_string(), // Legs
            6 => slot = lang.get(34).to_string(), // Neck
            5 => slot = lang.get(33).to_string(), // Shield
            _ => {}
        }
        match row[2] {
            2 => kind = lang.get(487).to_string(), // Heavy
            1 => kind = lang.get(488).to_string(), // Medium
            0 => kind = lang.get(489).to_string(), // Light
            _ => {}
        }
        let mut v = vec![
            format!("{}{}", lang.get(481), e_name(lang, vm, row[1])),
            format!("{}{}", lang.get(482), kind),
            format!("{}{}", lang.get(490), slot),
            format!("{}{}", lang.get(483), row[4]),
            format!("{}{}", lang.get(484), row[9]),
            format!("{}{}", lang.get(485), row[9] >> 2),
            lang.get(486).to_string(), // "Availability:"
        ];
        for crow in h {
            match row[2] {
                2 => perm = 4,
                1 => perm = 3,
                0 => perm = 1,
                _ => {}
            }
            if vm.tables.class_allows(crow[0] as i8, perm) {
                v.push(format!("   {}", e_name(lang, vm, crow[1])));
            }
        }
        assert!(v.len() <= 15, "armor overview row exceeds the [15] alloc");
        out.push(v);
    }
    out
}

/// `f()` — the Weapons Overview: 6 fixed lines + the allowed-class list.
pub fn weapons(lang: &Lang, vm: &GameVm) -> Vec<Vec<String>> {
    let c = vm.tables.rows(4);
    let h = vm.tables.rows(5);
    let mut out = Vec::new();
    let mut kind = "null".to_string();
    let mut perm = 0i32;
    for row in c.iter().skip(1) {
        match row[2] {
            0 => kind = lang.get(476).to_string(), // Axe
            1 => kind = lang.get(477).to_string(), // Blunt
            4 => kind = lang.get(478).to_string(), // Bow
            2 => kind = lang.get(479).to_string(), // Long Blade
            3 => kind = lang.get(480).to_string(), // Short Blade
            _ => {}
        }
        let mut v = vec![
            format!("{}{}", lang.get(481), e_name(lang, vm, row[1])),
            format!("{}{}", lang.get(482), kind),
            format!("{}{}", lang.get(483), row[3]),
            format!("{}{}", lang.get(484), row[7]),
            format!("{}{}", lang.get(485), row[7] >> 2),
            lang.get(486).to_string(), // "Availability:"
        ];
        for crow in h {
            match row[2] {
                0 => perm = 14,
                1 => perm = 5,
                4 => perm = 8,
                2 => perm = 6,
                3 => perm = 7,
                _ => {}
            }
            if vm.tables.class_allows(crow[0] as i8, perm) {
                v.push(format!("   {}", e_name(lang, vm, crow[1])));
            }
        }
        assert!(v.len() <= 14, "weapon overview row exceeds the [14] alloc");
        out.push(v);
    }
    out
}
