//! `digest-decode` — command-line tool for decoding a visual digest QR code.
//!
//! # Usage
//!
//! ```text
//! digest-decode <path-to-png>
//! ```
//!
//! Reads the PNG at `<path-to-png>`, locates and decodes the visual digest
//! QR code, and prints the parsed `Digest` as pretty JSON to stdout.
//!
//! # Exit codes
//!
//! | Code | Meaning |
//! |------|---------|
//! | 0 | success; JSON printed to stdout |
//! | 1 | usage error (wrong number of arguments) |
//! | 2 | I/O or image-decode error (file missing, not a PNG, etc.) |
//! | 3 | no QR code found in the image |
//! | 4 | QR decoded but the bytes are not a valid digest payload |

use shakenfist_visual_digest::{decode, decode_qr_png, QrError};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        let prog = args.first().map(|s| s.as_str()).unwrap_or("digest-decode");
        eprintln!("usage: {prog} <path-to-png>");
        std::process::exit(1);
    }
    let path = std::path::Path::new(&args[1]);

    let bytes = match decode_qr_png(path) {
        Ok(b) => b,
        Err(QrError::NoQrFound) => {
            eprintln!("error: no QR code found in {}", path.display());
            std::process::exit(3);
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };

    let digest = match decode(&bytes) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: decoding digest payload: {e}");
            std::process::exit(4);
        }
    };

    match serde_json::to_string_pretty(&digest) {
        Ok(json) => println!("{json}"),
        Err(e) => {
            eprintln!("error: serialising digest to JSON: {e}");
            std::process::exit(2);
        }
    }
}
