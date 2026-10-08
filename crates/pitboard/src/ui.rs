//! Terminal presentation: styles, and column arithmetic that counts what a terminal shows.
//! Text is written through anstream, which drops the styling when the output is not a
//! terminal, or when `NO_COLOR` asks it to.

use anstyle::{AnsiColor, Style};
use pitboard_core::pace::{Pace, Standing};
use pitboard_core::words::{self, UsageLevel};
use std::fmt::Display;
use unicode_width::UnicodeWidthStr;

pub const BOLD: Style = Style::new().bold();
pub const DIM: Style = Style::new().dimmed();
pub const GOOD: Style = AnsiColor::Green.on_default();
pub const WARN: Style = AnsiColor::Yellow.on_default();
pub const BAD: Style = AnsiColor::Red.on_default();

pub fn paint(style: Style, text: impl Display) -> String {
    format!("{style}{text}{style:#}")
}

/// Left-aligned in `width` columns of a terminal, which is not the same as characters: an
/// accented letter can be one column and a CJK one is two. Pad before painting: escape
/// codes take no columns at all.
pub fn pad(text: &str, width: usize) -> String {
    let shown = UnicodeWidthStr::width(text);
    format!("{text}{}", " ".repeat(width.saturating_sub(shown)))
}

/// What a terminal gives this text, for working out a column width.
pub fn columns(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// How alarming a share of a limit is: the colour of its level, which changes where the
/// app's tint does.
pub fn level(percent: f64) -> Style {
    match words::usage_level(percent) {
        UsageLevel::Plenty => GOOD,
        UsageLevel::Low => WARN,
        UsageLevel::Out => BAD,
    }
}

/// The colour a pace is said in: red over, green under, and dim at an even pace, which
/// is nothing to act on.
pub fn pace(standing: Standing) -> Style {
    match standing {
        Standing::Over { .. } => BAD,
        Standing::Under => GOOD,
        Standing::Even => DIM,
    }
}

/// A limit's bar: the share used, in the colour of how much that is, and a mark in the cell
/// where an even use would be by now, in the colour of its pace. No mark at an even pace,
/// nor where the pace means nothing.
pub fn bar(percent: f64, pace: Option<&Pace>, width: usize) -> String {
    let filled = ((percent / 100.0) * width as f64)
        .round()
        .clamp(0.0, width as f64) as usize;
    // The cells from `from` to `to`, filled as far as the share goes. A run of no cells is
    // left out rather than painted empty.
    let run = |style: Style, cell: &str, count: usize| {
        if count == 0 {
            String::new()
        } else {
            paint(style, cell.repeat(count))
        }
    };
    let cells = |from: usize, to: usize| {
        let full = filled.clamp(from, to);
        format!(
            "{}{}",
            run(level(percent), "█", full - from),
            run(DIM, "░", to - full)
        )
    };
    let Some(pace) = pace.filter(|p| p.standing != Standing::Even) else {
        return cells(0, width);
    };
    let at = ((pace.expected / 100.0) * width as f64).floor() as usize;
    let at = at.min(width.saturating_sub(1));
    format!(
        "{}{}{}",
        cells(0, at),
        paint(self::pace(pace.standing), "│"),
        cells(at + 1, width)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(styled: &str) -> String {
        anstream::adapter::strip_str(styled).to_string()
    }

    #[test]
    fn bars_are_the_width_they_claim() {
        assert_eq!(plain(&bar(0.0, None, 10)), "░░░░░░░░░░");
        assert_eq!(plain(&bar(62.0, None, 10)), "██████░░░░");
        assert_eq!(plain(&bar(150.0, None, 10)), "██████████");
    }

    /// A limit's pace is a mark in the cell where an even use would be by now, red where the
    /// limit is over pace and green where it is under, and no mark at an even pace.
    #[test]
    fn a_pace_is_a_mark_where_an_even_use_would_be() {
        let pace = |expected: f64, standing| Pace {
            expected,
            delta: 0.0,
            standing,
        };
        let over = pace(6.0, Standing::Over { runs_out_in: 60 });
        let under = pace(50.0, Standing::Under);
        assert_eq!(plain(&bar(33.0, Some(&over), 10)), "│██░░░░░░░");
        assert_eq!(plain(&bar(10.0, Some(&under), 10)), "█░░░░│░░░░");
        assert_eq!(
            plain(&bar(50.0, Some(&pace(50.0, Standing::Even)), 10)),
            "█████░░░░░"
        );
        assert_eq!(
            plain(&bar(0.0, Some(&pace(99.9, Standing::Under)), 10)),
            "░░░░░░░░░│",
            "the last cell holds the end of the window"
        );
        assert!(bar(33.0, Some(&over), 10).contains(&paint(BAD, "│")));
        assert!(bar(10.0, Some(&under), 10).contains(&paint(GOOD, "│")));
    }

    /// A terminal lines columns up by what it draws. A CJK character is two columns and an
    /// accented Latin one is still one, so counting characters puts everything after a name
    /// out of line.
    #[test]
    fn padding_counts_what_a_terminal_shows() {
        assert_eq!(pad("░", 3), "░  ");
        assert_eq!(pad("longer", 3), "longer", "never truncates");
        assert_eq!(pad("công", 8), "công    ", "an accent is one column");
        assert_eq!(pad("工作", 8), "工作    ", "and a CJK character is two");
        assert_eq!(columns("工作"), 4);
    }
}
