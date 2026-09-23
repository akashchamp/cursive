use super::LinesIterator;
use crate::style::{Effect, Style};
use crate::utils::markup::StyledString;
use crate::utils::span::Span;

fn input() -> StyledString {
    let mut text = StyledString::plain("I ");
    text.append(StyledString::styled("didn't", Effect::Bold));
    text.append(StyledString::plain(" say "));
    text.append(StyledString::styled("half", Effect::Italic));
    text.append(StyledString::plain(" the things people say I did.\n"));
    text.append(StyledString::plain(""));
    text.append(StyledString::plain("\n"));
    text.append(StyledString::plain("    - A. Einstein"));

    text
}

#[test]
fn test_replacement_char_has_width_1() {
    use unicode_width::UnicodeWidthStr;

    let replacement_char = "\u{FFFD}";
    assert_eq!(
        replacement_char.width(),
        1,
        "REPLACEMENT CHAR='{replacement_char}' should be width 1"
    );
}

#[test]
fn test_control_chars_have_width_0_or_1() {
    use unicode_width::UnicodeWidthStr;
    let control_chars = [
        "\u{0000}", "\u{0001}", "\u{0002}", "\u{0003}", "\u{0004}", "\u{0005}", "\u{0006}",
        "\u{0007}", "\u{0008}", "\u{0009}", "\u{000A}", "\u{000B}", "\u{000C}", "\u{000D}",
        "\u{000E}", "\u{000F}", "\u{0010}", "\u{0011}", "\u{0012}", "\u{0013}", "\u{0014}",
        "\u{0015}", "\u{0016}", "\u{0017}", "\u{0018}", "\u{0019}", "\u{001A}", "\u{001B}",
        "\u{001C}", "\u{001D}", "\u{001E}", "\u{001F}", "\u{007F}", "\u{0080}", "\u{0081}",
        "\u{0082}", "\u{0083}", "\u{0084}", "\u{0085}", "\u{0086}", "\u{0087}", "\u{0088}",
        "\u{0089}", "\u{008A}", "\u{008B}", "\u{008C}", "\u{008D}", "\u{008E}", "\u{008F}",
        "\u{0090}", "\u{0091}", "\u{0092}", "\u{0093}", "\u{0094}", "\u{0095}", "\u{0096}",
        "\u{0097}", "\u{0098}", "\u{0099}", "\u{009A}", "\u{009B}", "\u{009C}", "\u{009D}",
        "\u{009E}", "\u{009F}",
    ];

    for c in &control_chars {
        let width = c.width();
        assert_eq!(
            c.chars().count(),
            1,
            "it's supposed to be a string of 1 char"
        );
        let unicode_escape = format!("\\u{{{:04X}}}", c.chars().last().unwrap() as u32);
        assert!(
            (0..=1).contains(&width),
            "Width of control character {unicode_escape} is not 0 or 1, it's {width}"
        );
    }
}

#[test]
fn test_next_line_char() {
    use unicode_width::UnicodeWidthStr;

    // From https://github.com/gyscos/cursive/issues/489
    let d: Vec<u8> = vec![194, 133, 45, 127, 29, 127, 127];
    let text = std::str::from_utf8(&d).unwrap();
    let string = StyledString::plain(text);
    let iter = LinesIterator::new(&string, 20);
    let rows: Vec<_> = iter.map(|row| row.resolve(&string)).collect();
    assert_eq!(
        &rows[..],
        &[
            vec![],
            vec![Span {
                content: "-\u{7f}\u{1d}\u{7f}\u{7f}",
                attr: &Style::none(),
                // This is 1 with unicode_width < 1.1.13, 5 after.
                width: "-\u{7f}\u{1d}\u{7f}\u{7f}".width(),
            }],
        ],
    );
}

#[test]
fn test_line_breaks() {
    let input = input();

    let iter = LinesIterator::new(&input, 17);

    let rows: Vec<_> = iter.map(|row| row.resolve(&input)).collect();

    assert_eq!(
        &rows[..],
        &[
            vec![
                Span {
                    content: "I ",
                    attr: &Style::none(),
                    width: 2,
                },
                Span {
                    content: "didn't",
                    attr: &Style::from(Effect::Bold),
                    width: 6,
                },
                Span {
                    content: " say ",
                    attr: &Style::none(),
                    width: 5,
                },
                Span {
                    content: "half",
                    attr: &Style::from(Effect::Italic),
                    width: 4,
                },
            ],
            vec![Span {
                content: "the things people",
                attr: &Style::none(),
                width: 17,
            }],
            vec![Span {
                content: "say I did.",
                attr: &Style::none(),
                width: 10,
            }],
            vec![],
            vec![Span {
                content: "    - A. Einstein",
                attr: &Style::none(),
                width: 17
            }],
        ]
    );
}

#[test]
fn newlines_take_no_room_unless_showing_spaces() {
    // Rows as (width, is_wrapped).
    let rows = |text: &str, width: usize, show_spaces: bool| -> Vec<(usize, bool)> {
        let text = StyledString::plain(text);
        let iter = LinesIterator::new(&text, width);
        let iter = if show_spaces {
            iter.show_spaces()
        } else {
            iter
        };
        iter.map(|row| (row.width, row.is_wrapped)).collect()
    };

    // A line break doesn't need a column of its own.
    assert_eq!(rows("abc\ndef", 3, false), [(3, false), (3, false)]);
    assert_eq!(rows("a\nb\nc\nd", 1, false), [(1, false); 4]);

    // With `show_spaces` (for a cursor), a cell is kept after each line.
    assert_eq!(rows("abc\ndef", 4, true), [(3, false), (3, false)]);
    // When there is no room for it, the cursor gets a row of its own.
    assert_eq!(
        rows("abc\ndef", 3, true),
        [(3, true), (0, false), (3, true), (0, false)]
    );
}

#[test]
fn too_wide_characters_are_left_out() {
    // At width 1, `中` (2 columns) can't be shown: it's left out, but the
    // rest of the text isn't.
    let text = StyledString::plain("中a\nb");
    let mut iter = LinesIterator::new(&text, 1);
    let rows: Vec<_> = iter
        .by_ref()
        .map(|row| {
            row.resolve(&text)
                .iter()
                .map(|span| span.content)
                .collect::<String>()
        })
        .collect();
    assert_eq!(rows, ["a", "b"]);
    assert!(iter.is_truncated());
}
