//! The palettes the screen can be drawn in, and the one that is current.
//!
//! Colour was hard-coded in the renderer until this module: seven functions
//! naming the sixteen terminal colours, which are whatever the reader's theme
//! says they are. A palette is a design decision and a design decision has to
//! be comparable, so the palettes live here as data and the renderer asks for
//! the current one.
//!
//! Which one is current is an index held per thread rather than an argument
//! threaded through every span: the renderer is one thread drawing one screen,
//! and a colour is not a property of a node or a column. `render::frame` sets
//! it from the application state at the top of every frame, so the two never
//! disagree by more than the frame being drawn.

use std::cell::Cell;

use ratatui::style::Color;

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub name: &'static str,
    /// What `--help` says about it, and what the switcher flashes.
    pub about: &'static str,
    pub frame: Color,
    pub label: Color,
    /// Whether the label carries the DIM modifier. Every terminal renders DIM
    /// differently, so a palette given in RGB does not ask for it.
    pub label_dim: bool,
    /// `None` leaves the terminal's own foreground, which is what the screen
    /// did before there were themes.
    pub ink: Option<Color>,
    pub accent: Color,
    pub calm: Color,
    pub signal: Color,
    pub sel_bg: Color,
    pub sel_fg: Option<Color>,
    /// The pale text of the selected row - the path in front of a name.
    pub sel_label: Color,
    pub mark_bg: Color,
    pub mark_fg: Color,
}

// The chassis of the panel theme, which is the only palette invented here.
const PLATE: Color = Color::Rgb(20, 20, 18);
const CHASSIS: Color = Color::Rgb(88, 88, 84);
const LABEL: Color = Color::Rgb(134, 134, 128);
const INK: Color = Color::Rgb(232, 232, 226);
const ORANGE: Color = Color::Rgb(240, 90, 35);
const BLUE: Color = Color::Rgb(58, 110, 165);
const SIGNAL: Color = Color::Rgb(214, 40, 40);
const TAPE: Color = Color::Rgb(245, 197, 24);
const RECESS: Color = Color::Rgb(64, 64, 60);

/// The palettes, in the order the `t` key walks them.
///
/// The six schemes after the first two are taken from the terminal themes
/// their readers already live in, and the colours are the published ones. Only
/// where this screen asks for a tone a scheme does not name - a frame line, or
/// the pale path in front of a selected name - is a tone chosen, and then from
/// within the scheme's own range. Three roles are constant across all of them:
/// the frame is the darkest of the three greys, the label sits above it, and
/// the text above that.
pub const THEMES: [Theme; 8] = [
    // What the screen drew before this module existed, kept exactly: the
    // sixteen names, so a terminal with its own palette keeps its own look.
    Theme {
        name: "classic",
        about: "the sixteen terminal colours, as the reader's own theme renders them",
        frame: Color::Gray,
        label: Color::DarkGray,
        label_dim: true,
        ink: None,
        accent: Color::Yellow,
        calm: Color::Cyan,
        signal: Color::Red,
        sel_bg: Color::DarkGray,
        sel_fg: None,
        sel_label: Color::Gray,
        mark_bg: Color::Yellow,
        mark_fg: Color::Black,
    },
    // A grey chassis and one orange, in the manner of a device panel. The
    // selection is a recessed key, so the bar on it keeps its band.
    Theme {
        name: "panel",
        about: "a grey chassis and one orange, the selected row recessed",
        frame: CHASSIS,
        label: LABEL,
        label_dim: false,
        ink: Some(INK),
        accent: ORANGE,
        calm: BLUE,
        signal: SIGNAL,
        sel_bg: RECESS,
        sel_fg: Some(INK),
        sel_label: LABEL,
        mark_bg: TAPE,
        mark_fg: PLATE,
    },
    Theme {
        name: "gruvbox",
        about: "gruvbox dark - warm greys, yellow and aqua",
        frame: Color::Rgb(80, 73, 69),
        label: Color::Rgb(146, 131, 116),
        label_dim: false,
        ink: Some(Color::Rgb(235, 219, 178)),
        accent: Color::Rgb(250, 189, 47),
        calm: Color::Rgb(142, 192, 124),
        signal: Color::Rgb(251, 73, 52),
        sel_bg: Color::Rgb(60, 56, 54),
        sel_fg: Some(Color::Rgb(251, 241, 199)),
        sel_label: Color::Rgb(168, 153, 132),
        mark_bg: Color::Rgb(250, 189, 47),
        mark_fg: Color::Rgb(40, 40, 40),
    },
    Theme {
        name: "solarized",
        about: "solarized dark - the measured greys with yellow and cyan",
        frame: Color::Rgb(88, 110, 117),
        label: Color::Rgb(131, 148, 150),
        label_dim: false,
        ink: Some(Color::Rgb(147, 161, 161)),
        accent: Color::Rgb(181, 137, 0),
        calm: Color::Rgb(42, 161, 152),
        signal: Color::Rgb(220, 50, 47),
        sel_bg: Color::Rgb(7, 54, 66),
        sel_fg: Some(Color::Rgb(238, 232, 213)),
        sel_label: Color::Rgb(101, 123, 131),
        mark_bg: Color::Rgb(181, 137, 0),
        mark_fg: Color::Rgb(0, 43, 54),
    },
    Theme {
        name: "nord",
        about: "nord - cold blue-grey, frost and aurora",
        frame: Color::Rgb(67, 76, 94),
        label: Color::Rgb(123, 136, 161),
        label_dim: false,
        ink: Some(Color::Rgb(216, 222, 233)),
        accent: Color::Rgb(235, 203, 139),
        calm: Color::Rgb(136, 192, 208),
        signal: Color::Rgb(191, 97, 106),
        sel_bg: Color::Rgb(59, 66, 82),
        sel_fg: Some(Color::Rgb(236, 239, 244)),
        sel_label: Color::Rgb(154, 165, 187),
        mark_bg: Color::Rgb(235, 203, 139),
        mark_fg: Color::Rgb(46, 52, 64),
    },
    Theme {
        name: "dracula",
        about: "dracula - a violet ground with cyan and pink",
        frame: Color::Rgb(68, 71, 90),
        label: Color::Rgb(98, 114, 164),
        label_dim: false,
        ink: Some(Color::Rgb(248, 248, 242)),
        accent: Color::Rgb(241, 250, 140),
        calm: Color::Rgb(139, 233, 253),
        signal: Color::Rgb(255, 85, 85),
        sel_bg: Color::Rgb(58, 61, 78),
        sel_fg: Some(Color::Rgb(248, 248, 242)),
        sel_label: Color::Rgb(139, 149, 196),
        mark_bg: Color::Rgb(241, 250, 140),
        mark_fg: Color::Rgb(40, 42, 54),
    },
    Theme {
        name: "tokyo-night",
        about: "tokyo night - deep blue with a soft blue text",
        frame: Color::Rgb(41, 46, 66),
        label: Color::Rgb(86, 95, 137),
        label_dim: false,
        ink: Some(Color::Rgb(192, 202, 245)),
        accent: Color::Rgb(224, 175, 104),
        calm: Color::Rgb(125, 207, 255),
        signal: Color::Rgb(247, 118, 142),
        sel_bg: Color::Rgb(40, 52, 87),
        sel_fg: Some(Color::Rgb(192, 202, 245)),
        sel_label: Color::Rgb(121, 130, 184),
        mark_bg: Color::Rgb(224, 175, 104),
        mark_fg: Color::Rgb(26, 27, 38),
    },
    Theme {
        name: "catppuccin",
        about: "catppuccin mocha - a muted pastel set on a dark ground",
        frame: Color::Rgb(49, 50, 68),
        label: Color::Rgb(108, 112, 134),
        label_dim: false,
        ink: Some(Color::Rgb(205, 214, 244)),
        accent: Color::Rgb(249, 226, 175),
        calm: Color::Rgb(137, 220, 235),
        signal: Color::Rgb(243, 139, 168),
        sel_bg: Color::Rgb(69, 71, 90),
        sel_fg: Some(Color::Rgb(205, 214, 244)),
        sel_label: Color::Rgb(147, 153, 178),
        mark_bg: Color::Rgb(249, 226, 175),
        mark_fg: Color::Rgb(30, 30, 46),
    },
];

// Per thread rather than per process. The screen is drawn on one thread, so
// the two are the same thing in a running application - but the tests draw on
// several at once, and a palette shared between them is a test reading another
// test's colours.
thread_local! {
    static CURRENT: Cell<usize> = const { Cell::new(0) };
}

pub fn set(index: usize) {
    CURRENT.with(|c| c.set(index % THEMES.len()));
}

pub fn current() -> &'static Theme {
    &THEMES[CURRENT.with(|c| c.get()) % THEMES.len()]
}

/// The index of a theme by name. Used by `--theme` and by the environment
/// variable, so the two cannot accept different spellings.
pub fn index_of(name: &str) -> Option<usize> {
    THEMES.iter().position(|t| t.name == name)
}

pub fn names() -> String {
    THEMES.iter().map(|t| t.name).collect::<Vec<_>>().join("|")
}

/// The theme named by `HOSTSCOPE_THEME`, when it names one. An unreadable or
/// unknown value is ignored rather than refused: the variable is set once in a
/// shell profile and read on every host, and a host whose binary is older than
/// the name should still start.
pub fn from_env() -> Option<usize> {
    index_of(std::env::var("HOSTSCOPE_THEME").ok()?.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_is_reachable_by_its_name() {
        for (i, t) in THEMES.iter().enumerate() {
            assert_eq!(index_of(t.name), Some(i), "{} is not reachable", t.name);
            assert!(!t.about.is_empty(), "{} says nothing about itself", t.name);
        }
        assert_eq!(index_of("no-such-theme"), None);
    }

    /// The first theme is what the screen drew before themes existed. A run
    /// that names nothing must look exactly as it did.
    #[test]
    fn the_first_theme_is_the_one_the_screen_had() {
        assert_eq!(THEMES[0].name, "classic");
        assert_eq!(THEMES[0].frame, Color::Gray);
        assert_eq!(THEMES[0].sel_bg, Color::DarkGray);
        assert!(THEMES[0].ink.is_none());
    }

    /// The variable is the way a reader keeps a palette across hosts: it goes
    /// in a shell profile once, and every binary it reaches opens in it.
    #[test]
    fn the_environment_names_a_theme_and_a_name_it_does_not_know_is_ignored() {
        std::env::set_var("HOSTSCOPE_THEME", " panel ");
        assert_eq!(from_env(), index_of("panel"), "the name was not read");
        // A profile is written once and read on every host, so a binary older
        // than the name in it still has to start.
        std::env::set_var("HOSTSCOPE_THEME", "a-theme-from-a-later-version");
        assert_eq!(from_env(), None, "an unknown name was not ignored");
        std::env::remove_var("HOSTSCOPE_THEME");
        assert_eq!(from_env(), None);
    }

    /// A reader who already lives in one of these schemes should find it by
    /// the name they know it by. The trial palette that lit the selected row
    /// in the accent is gone: the bar on that row went dark to survive it, and
    /// a bar that changes meaning with the palette is worse than a dull row.
    #[test]
    fn the_palette_holds_the_schemes_a_reader_already_knows() {
        for name in [
            "classic",
            "panel",
            "gruvbox",
            "solarized",
            "nord",
            "dracula",
            "tokyo-night",
            "catppuccin",
        ] {
            assert!(index_of(name).is_some(), "{name} is not among the themes");
        }
        assert_eq!(index_of("lit"), None, "the lit trial is still here");
    }

    /// The bar of the selected row keeps the colour of its band (D-20), so a
    /// theme whose selection ground is one of those colours hides the reading
    /// on exactly the row the reader is looking at.
    #[test]
    fn no_theme_hides_its_reading_in_the_ground_of_the_selected_row() {
        for t in THEMES.iter() {
            for (role, c) in [
                ("calm", t.calm),
                ("accent", t.accent),
                ("signal", t.signal),
                ("label", t.sel_label),
            ] {
                assert_ne!(c, t.sel_bg, "{}: {role} is the selection ground", t.name);
            }
            let ink = t.sel_fg.or(t.ink);
            if let Some(ink) = ink {
                assert_ne!(ink, t.sel_bg, "{}: the text is its own ground", t.name);
            }
        }
    }

    #[test]
    fn setting_a_theme_past_the_end_wraps_rather_than_panics() {
        set(THEMES.len() + 1);
        assert_eq!(current().name, THEMES[1].name);
        set(0);
    }
}
