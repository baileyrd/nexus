//! iTerm2 inline-image protocol (`OSC 1337 ; File=<args> : <base64>`).
//!
//! Only the inline-image subcommand is handled here; other `1337;` subcommands
//! (SetUserVar, CurrentDir, shell integration, …) are ignored by the OSC
//! dispatcher. The payload is a complete image file — PNG (via [`super::png`])
//! or baseline JPEG (via [`super::jpeg`]) — decoded to pixels and handed to the
//! shared [`Grid::render_image`] sink (half-block cells, plus the full-res
//! overlay under the `gui` renderer). The display is auto-fit to the available
//! columns like Sixel/Kitty; the optional `width`/`height`/`preserveAspectRatio`
//! geometry hints are not honored yet.

use super::grid::Grid;
use super::{base64, jpeg, png};

/// Cap on the decoded file size (8 MiB) so a huge payload can't exhaust memory.
const MAX_FILE: usize = 8 * 1024 * 1024;

/// Handle an `OSC 1337` argument string (everything after `1337;`). Renders the
/// image when it is a well-formed inline PNG/JPEG transfer; otherwise no-ops.
pub(crate) fn feed(text: &str, g: &mut Grid) {
    let Some(rest) = text.strip_prefix("File=") else {
        return; // not the inline-image subcommand
    };
    let Some((args, b64)) = rest.split_once(':') else {
        return; // no payload separator
    };
    // iTerm2 displays inline only when `inline=1`; absent/0 means a file
    // download, which a terminal emulator has no surface for.
    let inline = args.split(';').any(|kv| {
        kv.split_once('=')
            .is_some_and(|(k, v)| k.eq_ignore_ascii_case("inline") && v == "1")
    });
    if !inline {
        return;
    }
    // Bound the work before allocating: base64 expands ~4:3.
    if b64.len() / 4 * 3 > MAX_FILE {
        return;
    }
    let Some(file) = base64::decode(b64.as_bytes()) else {
        return;
    };
    let decoded = if file.starts_with(&[0x89, b'P', b'N', b'G']) {
        png::decode(&file).map(|im| (im.width, im.height, im.rgba))
    } else if file.starts_with(&[0xFF, 0xD8, 0xFF]) {
        jpeg::decode(&file).map(|im| (im.width, im.height, im.rgba))
    } else {
        None // GIF / WebP / other formats not supported
    };
    let Some((w, h, rgba)) = decoded else {
        return;
    };
    // Pack RGBA8 into the grid's pixel format (alpha 0 -> transparent).
    let pixels: Vec<Option<u32>> = rgba
        .chunks_exact(4)
        .map(|p| {
            if p[3] == 0 {
                None
            } else {
                Some(((p[0] as u32) << 16) | ((p[1] as u32) << 8) | p[2] as u32)
            }
        })
        .collect();
    g.render_image(w, h, &pixels);
}
