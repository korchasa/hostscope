//! Text and number formatting shared by the table, the cards and the dumps.
//!
//! Every string that comes from the host (a container name, a unit name, a
//! command line) passes through [`clean`] first: what breaks a frame is a
//! control character, not a letter, so the letters are kept as they are and
//! only the characters that move the cursor are removed (FR-12).
//!
//! Widths are counted in terminal cells rather than in `char`s: a name in any
//! script must not push the next column aside (section 11).

use unicode_width::UnicodeWidthChar;

/// Removes what would move the cursor or leave an invisible hole: control
/// characters become a space, and zero-width and line-separating characters are
/// dropped. Letters of any script are kept.
pub fn clean(s: &str) -> String {
    if s.chars().all(|c| !c.is_control() && !is_invisible(c)) {
        return s.to_string();
    }
    s.chars()
        .filter(|c| !is_invisible(*c))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Zero-width and direction-changing characters. They occupy no cell but do
/// change what the terminal draws, so they are dropped rather than kept.
fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

/// The terminal cells one character takes, from the Unicode tables rather than
/// from a list written here. The first version of this function carried its own
/// ranges and was missing whole blocks - a rocket (U+1F680) counted as one cell
/// and pushed every line that held it one cell out of true (D-18). A character
/// the tables call unprintable takes no cell: `clean` has already removed those.
pub fn char_width(c: char) -> usize {
    UnicodeWidthChar::width(c).unwrap_or(0)
}

/// The cells a string takes. Every column measurement in the frame uses this
/// and not `chars().count()`.
pub fn str_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// The longest prefix that fits `n` cells, with the width it actually took. A
/// wide character is never split in half: it either fits whole or is left out.
fn take_width(s: &str, n: usize) -> (String, usize) {
    let mut out = String::new();
    let mut w = 0usize;
    for c in s.chars() {
        let cw = char_width(c);
        if w + cw > n {
            break;
        }
        out.push(c);
        w += cw;
    }
    (out, w)
}

/// Pads on the right to exactly `n` cells, truncating if wider.
pub fn pad(s: &str, n: usize) -> String {
    let (mut out, w) = take_width(s, n);
    out.push_str(&" ".repeat(n - w));
    out
}

/// Pads on the left to exactly `n` cells, truncating if wider.
pub fn pad_left(s: &str, n: usize) -> String {
    let (out, w) = take_width(s, n);
    let mut left = " ".repeat(n - w);
    left.push_str(&out);
    left
}

/// Pads on the left to at least `n` cells and never cuts.
///
/// The figures of the header change on every tick, and a figure that gains a
/// digit pushes the label beside it one cell to the right - so the eye has to
/// find the label again on every frame (D-39). Each of them holds a place wide
/// enough for the range a host actually reaches; a value wider than its place
/// takes the room it needs, because a header that moves once an hour is better
/// than one that quietly drops a digit.
pub fn pad_num(s: &str, n: usize) -> String {
    let w = str_width(s);
    if w >= n {
        return s.to_string();
    }
    let mut out = " ".repeat(n - w);
    out.push_str(s);
    out
}

/// Truncates with an ellipsis to exactly `n` cells, padding when narrower.
/// Columns must never merge, whatever the name (section 11 of the requirements).
pub fn fit(s: &str, n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    if str_width(s) <= n {
        return pad(s, n);
    }
    let (mut out, w) = take_width(s, n - 1);
    out.push('\u{2026}');
    out.push_str(&" ".repeat(n - 1 - w));
    out
}

/// Truncates from the left, which is what the path line needs: the nearer
/// levels matter more than the root.
pub fn fit_left(s: &str, n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    if str_width(s) <= n {
        return pad(s, n);
    }
    let chars: Vec<char> = s.chars().collect();
    let mut w = 0usize;
    let mut i = chars.len();
    while i > 0 {
        let cw = char_width(chars[i - 1]);
        if w + cw > n - 1 {
            break;
        }
        w += cw;
        i -= 1;
    }
    let tail: String = chars[i..].iter().collect();
    format!("\u{2026}{tail}{}", " ".repeat(n - 1 - w))
}

/// Memory in the table: megabytes below 1 GiB, gigabytes above.
pub fn mem_str(bytes: f64) -> String {
    let mb = bytes / (1024.0 * 1024.0);
    if mb.abs() >= 1024.0 {
        format!("{:.1}G", mb / 1024.0)
    } else if mb != 0.0 && mb.abs() < 10.0 {
        // Below ten megabytes a whole number turns half a megabyte into `0M`,
        // which reads as nothing at all. A process card of a small helper
        // process is exactly where that number matters, so it keeps a decimal.
        format!("{:.1}M", mb)
    } else {
        format!("{}M", mb.round() as i64)
    }
}

/// CPU is written in busy cores and in nothing else (FR-1a, D-25). Three
/// decimals: 0.004 cores is a process ticking over and 0 is a process doing
/// nothing, and the difference has to be readable.
pub fn cores_str(cores: f64) -> String {
    format!("{:.3}", cores)
}

/// The same when the value may be unavailable, which is never a zero (FR-8).
pub fn cores_opt(cores: Option<f64>) -> String {
    or_na(cores, cores_str)
}

/// A read/write pair of byte rates as a single cell: `0/96K`. The unit is chosen for the pair so the two halves stay comparable.
pub fn pair_rate(r: f64, w: f64) -> String {
    let max = r.max(w);
    if max >= 1024.0 * 1024.0 * 100.0 {
        format!("{:.0}/{:.0}G", r / 1073741824.0, w / 1073741824.0)
    } else if max >= 1024.0 * 100.0 {
        format!("{:.0}/{:.0}M", r / 1048576.0, w / 1048576.0)
    } else {
        format!("{:.0}/{:.0}K", r / 1024.0, w / 1024.0)
    }
}

/// The same pair when the source cannot attribute the traffic (FR-11): the cell
/// says so instead of printing a zero.
pub fn pair_rate_opt(r: Option<f64>, w: Option<f64>) -> String {
    match (r, w) {
        (Some(r), Some(w)) => pair_rate(r, w),
        _ => "n/a".to_string(),
    }
}

/// A value that may be unavailable, printed as `n/a` when it is. Nothing on
/// screen may stand in for an unknown (FR-8).
pub fn or_na(v: Option<f64>, f: impl Fn(f64) -> String) -> String {
    match v {
        Some(v) => f(v),
        None => "n/a".to_string(),
    }
}

/// A single byte total, used in cards.
pub fn bytes_str(b: f64) -> String {
    const K: f64 = 1024.0;
    if b >= K * K * K {
        format!("{:.1} GB", b / (K * K * K))
    } else if b >= K * K {
        format!("{:.1} MB", b / (K * K))
    } else if b >= K {
        format!("{:.1} KB", b / K)
    } else {
        format!("{:.0} B", b)
    }
}

/// A duration as `12m04s`, the form the averaging window is labelled with.
pub fn dur_str(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}h{m:02}m{sec:02}s")
    } else if m > 0 {
        format!("{m}m{sec:02}s")
    } else {
        format!("{sec}s")
    }
}

/// An uptime as `1091d`, the form the header carries.
pub fn uptime_str(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    let d = s / 86400;
    if d > 0 {
        format!("{d}d")
    } else {
        format!("{}h", s / 3600)
    }
}

const BLOCKS: [&str; 8] = [
    "", "\u{258F}", "\u{258E}", "\u{258D}", "\u{258C}", "\u{258B}", "\u{258A}", "\u{2589}",
];

/// A horizontal bar of exactly `width` characters. A non-zero value always
/// draws at least one tick: an empty cell reads as zero, and 0 and 0.004 cores
/// are different things (section 11 of the requirements).
pub fn bar(frac: f64, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let f = frac.clamp(0.0, 1.0);
    let units = f * width as f64 * 8.0;
    let mut full = (units / 8.0).floor() as usize;
    let mut rest = (units % 8.0).round() as usize;
    if rest == 8 {
        full += 1;
        rest = 0;
    }
    let mut s = "\u{2588}".repeat(full.min(width));
    if rest > 0 && s.chars().count() < width {
        s.push_str(BLOCKS[rest]);
    }
    if s.is_empty() && frac > 0.0 {
        s.push_str(BLOCKS[1]);
    }
    let len = s.chars().count();
    if len < width {
        s.push_str(&" ".repeat(width - len));
    }
    s
}

const SPARK: [char; 8] = [
    '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}',
];

/// A sparkline of the CPU history shown in the header.
pub fn sparkline(values: &[f64], width: usize) -> String {
    let mut out = String::new();
    let start = values.len().saturating_sub(width);
    let slice = &values[start..];
    let max = slice.iter().cloned().fold(0.0f64, f64::max);
    for _ in slice.len()..width {
        out.push(SPARK[0]);
    }
    for v in slice {
        let idx = if max <= 0.0 {
            0
        } else {
            ((v / max) * 7.0).round().clamp(0.0, 7.0) as usize
        };
        out.push(SPARK[idx]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_memory_value_keeps_a_decimal_instead_of_rounding_to_nothing() {
        // Measured on the test host: the PSS of a `timeout` process is 0.4 MB, and
        // whole megabytes printed that as `0M`.
        assert_eq!(mem_str(0.4 * 1024.0 * 1024.0), "0.4M");
        assert_eq!(mem_str(6.0 * 1024.0 * 1024.0), "6.0M");
        assert_eq!(mem_str(0.0), "0M");
        assert_eq!(mem_str(62.0 * 1024.0 * 1024.0), "62M");
        assert_eq!(mem_str(-1.5 * 1024.0 * 1024.0), "-1.5M");
    }

    #[test]
    fn a_name_keeps_its_letters_whatever_the_script() {
        assert_eq!(
            clean("\u{0431}\u{043E}\u{0442}"),
            "\u{0431}\u{043E}\u{0442}"
        );
        assert_eq!(clean("hs-plain"), "hs-plain");
        assert_eq!(clean("a\u{4E2D}b"), "a\u{4E2D}b");
    }

    #[test]
    fn only_what_moves_the_cursor_is_removed() {
        assert_eq!(clean("a\nb\tc"), "a b c");
        assert_eq!(clean("a\u{200B}b\u{FEFF}"), "ab");
        assert_eq!(clean("a\u{1B}[31mb"), "a [31mb");
    }

    #[test]
    fn width_is_counted_in_cells_not_in_characters() {
        assert_eq!(str_width("abc"), 3);
        // Cyrillic takes one cell, a CJK ideograph and an emoji take two.
        assert_eq!(str_width("\u{0431}\u{043E}\u{0442}"), 3);
        assert_eq!(str_width("\u{4E2D}\u{6587}"), 4);
        assert_eq!(str_width("\u{1F600}"), 2);
        // A combining mark rides on the letter before it.
        assert_eq!(str_width("e\u{0301}"), 1);
    }

    #[test]
    fn every_block_of_wide_characters_counts_as_two_cells() {
        // The table this used to carry by hand covered the two emoji blocks it
        // was written from and silently dropped the rest, so a rocket in a
        // command line moved the whole line one cell (D-18). Every character
        // here is outside those two blocks and would have counted as one.
        for c in [
            '\u{1F680}', // rocket, transport block
            '\u{2705}',  // check mark, dingbats
            '\u{26C4}',  // snowman, miscellaneous symbols
            '\u{231A}',  // watch
            '\u{2B50}',  // star
            '\u{1FA79}', // adhesive bandage, a block added in Unicode 12
            '\u{1F004}', // mahjong tile
        ] {
            assert_eq!(char_width(c), 2, "{c:?} must take two cells");
        }
        // And the narrow neighbours of those blocks stay at one.
        for c in ['\u{2502}', '\u{2026}', '\u{203A}', '\u{2588}'] {
            assert_eq!(char_width(c), 1, "{c:?} must take one cell");
        }
    }

    #[test]
    fn fit_never_exceeds_the_column() {
        assert_eq!(str_width(&fit("abcdef", 4)), 4);
        assert_eq!(fit("abcdef", 4), "abc\u{2026}");
        assert_eq!(fit("ab", 4), "ab  ");
    }

    #[test]
    fn a_wide_character_is_never_split_across_the_column_edge() {
        // Three ideographs are six cells. Five cells hold two of them and the
        // ellipsis; four hold one of them, the ellipsis, and a space where the
        // second one would only half fit.
        let out = fit("\u{4E2D}\u{6587}\u{5B57}", 5);
        assert_eq!(str_width(&out), 5);
        assert_eq!(out, "\u{4E2D}\u{6587}\u{2026}");
        let out = fit("\u{4E2D}\u{6587}\u{5B57}", 4);
        assert_eq!(str_width(&out), 4);
        assert_eq!(out, "\u{4E2D}\u{2026} ");
        assert_eq!(str_width(&pad("\u{4E2D}\u{6587}", 3)), 3);
        assert_eq!(str_width(&pad_left("\u{4E2D}\u{6587}", 3)), 3);
    }

    #[test]
    fn fit_left_keeps_the_tail() {
        assert_eq!(fit_left("host > a > b", 6), "\u{2026}a > b");
        assert_eq!(str_width(&fit_left("host \u{203a} \u{4E2D}\u{6587}", 6)), 6);
    }

    #[test]
    fn bar_draws_a_tick_for_any_non_zero_value() {
        assert_eq!(bar(0.0, 4), "    ");
        assert_ne!(bar(0.0001, 4), "    ");
        assert_eq!(bar(1.0, 4).chars().count(), 4);
    }

    /// The one unit CPU is written in, and the one thing an unavailable value
    /// may not become (FR-1a, FR-8, D-25).
    #[test]
    fn cpu_is_written_in_cores_and_an_unknown_is_not_a_zero() {
        assert_eq!(cores_str(1.1404), "1.140");
        assert_eq!(cores_str(0.0004), "0.000");
        assert_eq!(cores_opt(Some(16.64)), "16.640");
        assert_eq!(cores_opt(None), "n/a");
    }
}
