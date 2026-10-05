//! What a terminal makes of what a tool printed: the text it shows, and where each
//! hyperlink goes.
//!
//! A tool prints for a terminal, and a terminal shows only part of what it is sent. Escape
//! sequences colour text, name a window, or make text a hyperlink to an address of their
//! own, OSC 8. Claude Code prints its sign-in address through a helper that writes an OSC 8
//! hyperlink wherever it believes the terminal takes them, which it can believe while
//! piped; the register's `sign_in_output` says when. Read as plain text, that address ran
//! on into the BEL that ends the link's target, and from there into the link's text. So
//! what a tool printed is read here as a terminal reads it, and no byte of an escape
//! sequence reaches what comes out.
//!
//! Sequences are framed as ECMA-48 frames their 7-bit forms, with one end that xterm adds.
//! A control sequence, `ESC [`, ends at its final byte. A control string, `ESC ]`, `ESC P`,
//! `ESC X`, `ESC ^` or `ESC _`, ends at ST, `ESC \`, and another escape cancels it. An
//! operating system command, `ESC ]`, also ends at BEL: ECMA-48 ends it only at ST, but
//! xterm ends it at BEL too, and Claude Code ends its hyperlinks with BEL. BEL ends none of
//! the other control strings, in xterm or in ECMA-48. Any other escape ends at the byte that
//! finishes it. Output arrives in pieces, so a sequence not finished yet ends what has been
//! printed so far: the rest of it is still to come.

const ESC: char = '\u{1b}';
const BEL: char = '\u{7}';

/// One piece of what a tool printed, in the order a terminal meets it.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Printed<'a> {
    /// Text a terminal shows, with no escape sequence in it.
    Text(&'a str),
    /// The target of a hyperlink that opens here. The text it links follows as text.
    Link(&'a str),
}

/// What was printed, `said`, piece by piece.
pub(super) fn read(said: &str) -> impl Iterator<Item = Printed<'_>> {
    let mut rest = said;
    std::iter::from_fn(move || {
        loop {
            let Some(sequence) = rest.strip_prefix(ESC) else {
                if rest.is_empty() {
                    return None;
                }
                let (text, after) = rest.split_at(rest.find(ESC).unwrap_or(rest.len()));
                rest = after;
                return Some(Printed::Text(text));
            };
            let Some((command, after)) = escape(sequence) else {
                // Not finished yet, so nothing after it has been printed either.
                rest = "";
                return None;
            };
            rest = after;
            if let Some(target) = command.and_then(hyperlink_target) {
                return Some(Printed::Link(target));
            }
        }
    })
}

/// The escape sequence `sequence` starts, the `ESC` before it taken off: the command it
/// carries, if it is an operating system command, and what follows it. `None` while it is
/// not finished.
fn escape(sequence: &str) -> Option<(Option<&str>, &str)> {
    let first = sequence.chars().next()?;
    match first {
        '[' => {
            let body = &sequence[1..];
            let end = body.find(|c| !matches!(c, '\u{20}'..='\u{3f}'))?;
            Some((None, past_final(&body[end..], '\u{40}'..='\u{7e}')))
        }
        ']' | 'P' | 'X' | '^' | '_' => {
            let body = &sequence[1..];
            // BEL ends an operating system command, as xterm has it, and no other string.
            let end = if first == ']' {
                body.find([BEL, ESC])
            } else {
                body.find(ESC)
            };
            let (string, ended) = body.split_at(end?);
            match ended
                .strip_prefix(BEL)
                .or_else(|| ended.strip_prefix("\u{1b}\\"))
            {
                Some(after) => Some(((first == ']').then_some(string), after)),
                // An ESC that ST may yet follow.
                None if ended.len() == ESC.len_utf8() => None,
                // Cancelled by the escape that ends it, which starts a sequence of its own.
                None => Some((None, ended)),
            }
        }
        '\u{20}'..='\u{2f}' => {
            let end = sequence.find(|c| !matches!(c, '\u{20}'..='\u{2f}'))?;
            Some((None, past_final(&sequence[end..], '\u{30}'..='\u{7e}')))
        }
        '\u{30}'..='\u{7e}' => Some((None, &sequence[1..])),
        // A lone ESC, before something that cannot follow one.
        _ => Some((None, sequence)),
    }
}

/// What follows a sequence whose final byte is the first of `rest`, if it is one of
/// `finals`. A sequence broken off before its final byte ends where it was broken off.
fn past_final(rest: &str, finals: std::ops::RangeInclusive<char>) -> &str {
    match rest.chars().next() {
        Some(last) if finals.contains(&last) => &rest[last.len_utf8()..],
        _ => rest,
    }
}

/// Where the hyperlink an operating system command opens goes: OSC 8 is
/// `8;<parameters>;<target>`. An empty target ends a hyperlink rather than opening one.
fn hyperlink_target(command: &str) -> Option<&str> {
    let (_, target) = command.strip_prefix("8;")?.split_once(';')?;
    (!target.is_empty()).then_some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pieces(said: &str) -> Vec<Printed<'_>> {
        read(said).collect()
    }

    /// A hyperlink is its target, then the text it links, whether BEL or ST ends each
    /// part, and whatever parameters it has. The sequence that ends it opens nothing.
    #[test]
    fn a_hyperlink_is_where_it_goes_and_then_its_text() {
        for (opens, ends) in [
            ("\u{1b}]8;;https://a.b/c\u{7}", "\u{1b}]8;;\u{7}"),
            ("\u{1b}]8;;https://a.b/c\u{1b}\\", "\u{1b}]8;;\u{1b}\\"),
            ("\u{1b}]8;id=1:x=y;https://a.b/c\u{7}", "\u{1b}]8;;\u{7}"),
        ] {
            assert_eq!(
                pieces(&format!("visit {opens}sign in{ends}.")),
                [
                    Printed::Text("visit "),
                    Printed::Link("https://a.b/c"),
                    Printed::Text("sign in"),
                    Printed::Text("."),
                ],
                "{opens:?}"
            );
        }
        assert_eq!(
            pieces("\u{1b}]8;;https://a.b/c;d=e\u{7}"),
            [Printed::Link("https://a.b/c;d=e")]
        );
    }

    /// Every other sequence is passed over whole: a colour, a window's title, a character
    /// set, a string for the terminal itself, and an escape of one byte. Text either side
    /// of one is a piece of its own.
    #[test]
    fn no_byte_of_another_sequence_is_read() {
        let sequences = [
            "\u{1b}[94m",
            "\u{1b}[38;2;1;2;3m",
            "\u{1b}[?25l",
            "\u{1b}]0;https://title.example\u{7}",
            "\u{1b}]0;https://title.example\u{1b}\\",
            "\u{1b}(B",
            "\u{1b}P1$r0m\u{1b}\\",
            "\u{1b}_Ga=T;AAAA\u{1b}\\",
            "\u{1b}7",
        ];
        for sequence in sequences {
            assert_eq!(
                pieces(&format!("a{sequence}b")),
                [Printed::Text("a"), Printed::Text("b")],
                "{sequence:?}"
            );
        }
    }

    /// Only an operating system command ends at BEL. A device control string, SOS, PM or
    /// APC reads on past one to ST, so nothing between the BEL and ST is text, and before
    /// ST arrives the string is not finished.
    #[test]
    fn only_an_operating_system_command_ends_at_bel() {
        for opens in ["\u{1b}P", "\u{1b}X", "\u{1b}^", "\u{1b}_"] {
            assert_eq!(
                pieces(&format!("a{opens}1\u{7}https://x.y\u{1b}\\b")),
                [Printed::Text("a"), Printed::Text("b")],
                "{opens:?}"
            );
            assert_eq!(
                pieces(&format!("a{opens}1\u{7}https://x.y")),
                [Printed::Text("a")],
                "{opens:?}"
            );
        }
        assert_eq!(
            pieces("a\u{1b}]0;title\u{7}https://x.y"),
            [Printed::Text("a"), Printed::Text("https://x.y")]
        );
    }

    /// A control string another escape breaks into is cancelled, and that escape is read
    /// as the start of a sequence of its own.
    #[test]
    fn a_string_another_escape_breaks_into_is_cancelled() {
        assert_eq!(
            pieces("\u{1b}]8;;https://a.b/c\u{1b}[0mtext"),
            [Printed::Text("text")]
        );
    }

    /// Output arrives in pieces. A sequence not finished yet ends what there is so far,
    /// down to an ESC that might be the first half of ST, and nothing of it is read.
    #[test]
    fn a_sequence_not_finished_yet_ends_what_there_is() {
        for said in [
            "a\u{1b}",
            "a\u{1b}[",
            "a\u{1b}[38;5",
            "a\u{1b}(",
            "a\u{1b}]8;;https://a.b/c",
            "a\u{1b}]8;;https://a.b/c\u{1b}",
        ] {
            assert_eq!(pieces(said), [Printed::Text("a")], "{said:?}");
        }
    }

    /// A sequence broken off by a byte that cannot be part of it ends there, and the byte
    /// is read as text.
    #[test]
    fn a_sequence_broken_off_ends_where_it_was_broken_off() {
        assert_eq!(pieces("\u{1b}[1\u{e9}a"), [Printed::Text("\u{e9}a")]);
        assert_eq!(pieces("\u{1b}\u{e9}a"), [Printed::Text("\u{e9}a")]);
    }
}
