//! ANSI escape-sequence stripper (PRD-09 §3.2).
//!
//! Takes a byte slice that may contain ANSI escape sequences and returns
//! a `String` with the sequences removed. The output is valid UTF-8; any
//! non-UTF-8 bytes in the input become replacement characters (`U+FFFD`)
//! via `String::from_utf8_lossy`.
//!
//! # Scope
//!
//! Handles the escape-sequence families that appear in normal terminal
//! output:
//!
//! - **CSI** (Control Sequence Introducer) — `ESC [ ... final`, where
//!   `final` is any byte in `0x40..=0x7e`. Covers SGR colour (`\x1b[31m`),
//!   cursor moves (`\x1b[10A`), erase (`\x1b[2J`), and the 256-colour /
//!   `TrueColor` variants (`\x1b[38;5;Nm`, `\x1b[38;2;R;G;Bm`).
//! - **OSC** (Operating System Command) — `ESC ] ... BEL` or
//!   `ESC ] ... ESC \`. Used for window titles, iTerm2 image sequences.
//! - **Plain 2-byte ESC** — `ESC X` for a small set of final bytes that
//!   don't take parameters (e.g. `ESC c` full reset).
//! - **Backspace `\x08`** — consumes the previous character if present,
//!   modelling the common progress-meter pattern of overstrike.
//! - **Carriage return `\r`** — preserved when followed by `\n`, dropped
//!   otherwise (lone `\r` inside a line is almost always a progress-bar
//!   rewind on the same line, which makes the final text-only view of
//!   that line the overwritten tail).
//!
//! Does **not** model full terminal state (cursor position, scrollback).
//! For that, run the raw bytes through a terminal emulator crate at
//! render time; the line view stored here is only for text search, FTS,
//! and deduplication.

/// Strip ANSI escape sequences from `bytes` and return a `String` view
/// suitable for text search and display-as-plain-text.
///
/// Invalid UTF-8 is replaced with `U+FFFD`. The function never panics.
#[must_use]
pub fn strip_ansi(bytes: &[u8]) -> String {
    // Pre-size slightly under the input length since stripping is a net
    // shrink — this avoids the Vec growing on realistic inputs.
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            0x1b => {
                // ESC. Decide family by the next byte, if any.
                let Some(&next) = bytes.get(i + 1) else {
                    // Trailing ESC — drop it.
                    break;
                };
                match next {
                    b'[' => {
                        // CSI: read params+intermediates then final byte
                        // in 0x40..=0x7e.
                        let mut j = i + 2;
                        while j < bytes.len() {
                            let c = bytes[j];
                            if (0x40..=0x7e).contains(&c) {
                                j += 1;
                                break;
                            }
                            j += 1;
                        }
                        i = j;
                    }
                    b']' => {
                        // OSC: terminated by BEL (0x07) or ST (ESC \\).
                        let mut j = i + 2;
                        while j < bytes.len() {
                            if bytes[j] == 0x07 {
                                j += 1;
                                break;
                            }
                            if bytes[j] == 0x1b && bytes.get(j + 1) == Some(&b'\\') {
                                j += 2;
                                break;
                            }
                            j += 1;
                        }
                        i = j;
                    }
                    _ => {
                        // 2-byte escape — just skip the ESC and the next
                        // byte. Covers `ESC c`, `ESC =`, `ESC >`, etc.
                        i += 2;
                    }
                }
            }
            0x08 => {
                // Backspace — drop the last emitted byte, if any. Walks
                // back to the start of the last UTF-8 scalar so we
                // don't leave a dangling continuation byte behind.
                while let Some(&last) = out.last() {
                    out.pop();
                    if last < 0x80 || (last & 0xc0) == 0xc0 {
                        // ASCII byte or start of a UTF-8 scalar: stop.
                        break;
                    }
                    // else: continuation byte 10xxxxxx — keep popping.
                }
                i += 1;
            }
            b'\r' => {
                // Preserve CR only when followed by LF (normal CRLF line
                // ending); otherwise drop it — a lone CR in a line is
                // almost always a progress-bar rewind on the same line.
                if bytes.get(i + 1) == Some(&b'\n') {
                    out.push(b);
                }
                i += 1;
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_unchanged() {
        assert_eq!(strip_ansi(b"hello world"), "hello world");
    }

    #[test]
    fn csi_sgr_color_is_removed() {
        // "red ERROR reset"
        assert_eq!(strip_ansi(b"\x1b[31mERROR\x1b[0m"), "ERROR");
    }

    #[test]
    fn csi_256color_is_removed() {
        assert_eq!(strip_ansi(b"\x1b[38;5;208morange\x1b[0m"), "orange");
    }

    #[test]
    fn csi_truecolor_is_removed() {
        assert_eq!(
            strip_ansi(b"\x1b[38;2;255;0;0mred\x1b[0m"),
            "red"
        );
    }

    #[test]
    fn csi_cursor_move_is_removed() {
        assert_eq!(strip_ansi(b"start\x1b[5Aend"), "startend");
    }

    #[test]
    fn csi_erase_is_removed() {
        assert_eq!(strip_ansi(b"\x1b[2Jcleared"), "cleared");
    }

    #[test]
    fn osc_window_title_is_removed_with_bel_terminator() {
        assert_eq!(
            strip_ansi(b"\x1b]0;window title\x07visible text"),
            "visible text",
        );
    }

    #[test]
    fn osc_window_title_is_removed_with_st_terminator() {
        assert_eq!(
            strip_ansi(b"\x1b]0;window title\x1b\\visible text"),
            "visible text",
        );
    }

    #[test]
    fn two_byte_escape_is_dropped() {
        // `\x1bc` is the full-reset two-byte escape. The parser drops
        // the ESC and the final byte, splicing the surrounding text.
        assert_eq!(strip_ansi(b"before\x1bcafter"), "beforeafter");
    }

    #[test]
    fn trailing_lone_esc_is_dropped() {
        assert_eq!(strip_ansi(b"text\x1b"), "text");
    }

    #[test]
    fn backspace_removes_previous_ascii_char() {
        assert_eq!(strip_ansi(b"hellp\x08o"), "hello");
    }

    #[test]
    fn backspace_removes_previous_multibyte_scalar() {
        // é = 0xc3 0xa9 (2 bytes). Ensure backspace removes both.
        assert_eq!(strip_ansi("café\x08".as_bytes()), "caf");
    }

    #[test]
    fn backspace_on_empty_output_is_noop() {
        assert_eq!(strip_ansi(b"\x08a"), "a");
    }

    #[test]
    fn crlf_is_preserved() {
        assert_eq!(strip_ansi(b"line1\r\nline2"), "line1\r\nline2");
    }

    #[test]
    fn lone_cr_is_dropped() {
        // Progress-bar rewind pattern: the final visible text is
        // whatever came after the last CR.
        assert_eq!(strip_ansi(b"25%\r100%"), "25%100%");
    }

    #[test]
    fn invalid_utf8_becomes_replacement_char() {
        // 0xff is never a valid UTF-8 start byte.
        let s = strip_ansi(&[b'a', 0xff, b'b']);
        assert!(s.contains('\u{fffd}'), "expected REPLACEMENT in {s:?}");
    }

    #[test]
    fn mixed_content_strips_all_escapes() {
        let input = b"\x1b[1m\x1b[31mBOLD RED\x1b[0m plain \x1b]0;title\x07tail";
        assert_eq!(strip_ansi(input), "BOLD RED plain tail");
    }

    #[test]
    fn unterminated_csi_consumes_rest() {
        // No final byte ever arrives; the parser should drain the buffer
        // without panicking and the output is the pre-CSI text.
        assert_eq!(strip_ansi(b"before\x1b[31"), "before");
    }
}
