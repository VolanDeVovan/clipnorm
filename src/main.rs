// clipnorm — watch the Wayland clipboard, transform image-only entries into a
// "Claude Code-friendly" multi-MIME clipboard.
//
// Why: when you copy an image from Telegram/browser/etc, the clipboard carries
// only image/* MIME types. Claude Code in alacritty (and similar TUI agents)
// only attaches images when the clipboard exposes a text/plain absolute path,
// so a plain Ctrl+Shift+V pastes nothing useful. clipnorm sees image-only
// clipboards, saves the image to ~/Pictures/Pasted/, and republishes the
// clipboard with the original image bytes PLUS text/plain (path) +
// text/uri-list + x-special/gnome-copied-files. After that the clipboard
// pastes correctly into:
//   - Claude Code / Codex / Gemini CLI in alacritty (text/plain → path)
//   - browsers / chats / image editors (image/<original>)
//   - Nautilus and other GTK file managers (x-special/gnome-copied-files)
//
// Loop guard: skip clipboards that already advertise text/plain (or our
// sentinel MIME). That covers our own publication and ordinary text copies.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{Duration, SystemTime};

use chrono::Local;
use wayland_clipboard_listener::{ClipBoardListenMessage, WlClipboardPasteStream, WlListenType};
use wl_clipboard_rs::copy::{
    MimeSource, MimeType as CopyMime, Options as CopyOptions, ServeRequests, Source,
};

const SENTINEL_MIME: &str = "application/x-clipnorm-wrapped";

// Files older than this are purged after each event. Mirrors clipaste's
// retention; rationale: clipnorm's output is throwaway scratch for one paste,
// not an archive. Long enough to cover "copy → switch app → paste" workflows.
const RETENTION: Duration = Duration::from_secs(3600);

const PREFERRED_IMAGE_MIMES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/gif",
    "image/avif",
    "image/bmp",
];

const SKIP_IF_PRESENT: &[&str] = &[
    SENTINEL_MIME,
    "text/plain",
    "text/plain;charset=utf-8",
    "text/uri-list",
    "UTF8_STRING",
    "STRING",
    "x-special/gnome-copied-files",
];

fn main() {
    if let Err(e) = run() {
        eprintln!("clipnorm: fatal: {e}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = output_dir();
    fs::create_dir_all(&output_dir)?;
    eprintln!(
        "clipnorm: watching clipboard, output → {}",
        output_dir.display()
    );

    let mut stream = WlClipboardPasteStream::init(WlListenType::ListenOnCopy)?;
    let prio: Vec<String> = PREFERRED_IMAGE_MIMES
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    stream.set_priority(prio);

    for msg in stream.paste_stream().flatten() {
        if let Err(e) = handle(&msg, &output_dir) {
            eprintln!("clipnorm: handle error: {e}");
        }
    }
    Ok(())
}

fn handle(msg: &ClipBoardListenMessage, output_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mimes: HashSet<&str> = msg.mime_types.iter().map(String::as_str).collect();

    if SKIP_IF_PRESENT.iter().any(|t| mimes.contains(*t)) {
        return Ok(());
    }
    if !mimes.iter().any(|m| m.starts_with("image/")) {
        return Ok(());
    }

    let chosen = msg.context.mime_type.as_str();
    if !chosen.starts_with("image/") {
        eprintln!(
            "clipnorm: image clipboard but listener picked {chosen}, skipping (offered: {:?})",
            msg.mime_types
        );
        return Ok(());
    }
    if msg.context.context.is_empty() {
        return Ok(());
    }

    let ext = mime_to_ext(chosen);
    let ts = Local::now().format("%Y-%m-%d %H-%M-%S").to_string();
    let filename = format!("Pasted from {ts}.{ext}");
    let path = output_dir.join(&filename);
    fs::write(&path, &msg.context.context)?;
    eprintln!(
        "clipnorm: saved {} ({} bytes, {chosen})",
        path.display(),
        msg.context.context.len()
    );

    republish(&path, chosen, &msg.context.context)?;
    cleanup_old(output_dir);
    Ok(())
}

fn cleanup_old(dir: &Path) {
    let Some(cutoff) = SystemTime::now().checked_sub(RETENTION) else {
        return;
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        // modified() is portable across Linux filesystems; created() needs
        // birth-time support which tmpfs (XDG_RUNTIME_DIR) lacks.
        let Ok(ts) = meta.modified() else { continue };
        if ts < cutoff {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn republish(path: &Path, image_mime: &str, image_bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let path_str = path.to_string_lossy().to_string();
    let encoded = url_encode_path(&path_str);
    let gnome = format!("copy\nfile://{encoded}");
    let uri_list = format!("file://{encoded}\n");

    let sources = vec![
        MimeSource {
            source: Source::Bytes(image_bytes.to_vec().into_boxed_slice()),
            mime_type: CopyMime::Specific(image_mime.into()),
        },
        MimeSource {
            source: Source::Bytes(path_str.clone().into_bytes().into_boxed_slice()),
            mime_type: CopyMime::Specific("text/plain;charset=utf-8".into()),
        },
        MimeSource {
            source: Source::Bytes(path_str.into_bytes().into_boxed_slice()),
            mime_type: CopyMime::Specific("text/plain".into()),
        },
        MimeSource {
            source: Source::Bytes(uri_list.into_bytes().into_boxed_slice()),
            mime_type: CopyMime::Specific("text/uri-list".into()),
        },
        MimeSource {
            source: Source::Bytes(gnome.into_bytes().into_boxed_slice()),
            mime_type: CopyMime::Specific("x-special/gnome-copied-files".into()),
        },
        MimeSource {
            source: Source::Bytes(b"clipnorm".to_vec().into_boxed_slice()),
            mime_type: CopyMime::Specific(SENTINEL_MIME.into()),
        },
    ];

    let mut opts = CopyOptions::new();
    opts.foreground(false);
    opts.serve_requests(ServeRequests::Unlimited);
    opts.copy_multi(sources)?;
    Ok(())
}

fn output_dir() -> PathBuf {
    if let Ok(d) = std::env::var("CLIPNORM_OUTPUT_DIR") {
        return PathBuf::from(d);
    }
    // Prefer XDG_RUNTIME_DIR (tmpfs in RAM, cleared on logout). Falls back
    // to /tmp/clipnorm if not set (e.g. running outside a user session).
    if let Ok(d) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(d).join("clipnorm");
    }
    PathBuf::from("/tmp/clipnorm")
}

fn mime_to_ext(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/avif" => "avif",
        "image/bmp" => "bmp",
        "image/tiff" | "image/tif" => "tiff",
        _ => "bin",
    }
}

fn url_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let safe = matches!(b,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' |
            b'-' | b'_' | b'.' | b'~' | b'/'
        );
        if safe {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
