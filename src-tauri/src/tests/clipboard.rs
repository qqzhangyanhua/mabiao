//! PNG → RGBA 是写系统剪贴板之前的纯转换。真正的剪贴板 I/O 在 CI 里不测。

#[cfg(target_os = "macos")]
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

/// 手写 1×1 不透明红 PNG（RGBA 8-bit），不是本仓库 encoder 的产物。
const RED_1X1_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0xF8, 0xCF, 0xC0, 0xF0,
    0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x56, 0xC7, 0x2F, 0x0D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

#[test]
fn png_to_rgba_keeps_one_red_pixel() {
    let image = crate::clipboard::png_to_rgba(RED_1X1_PNG).expect("valid png");
    assert_eq!(image.width, 1);
    assert_eq!(image.height, 1);
    assert_eq!(image.rgba, vec![255, 0, 0, 255]);
}

#[test]
fn png_to_rgba_rejects_empty_bytes() {
    let err = crate::clipboard::png_to_rgba(&[]).expect_err("empty");
    assert!(err.contains("空"), "{err}");
}

#[test]
fn png_to_rgba_rejects_invalid_bytes() {
    let err = crate::clipboard::png_to_rgba(&[0, 1, 2, 3]).expect_err("invalid");
    assert!(err.contains("无法解码"), "{err}");
}

#[test]
fn copy_png_base64_rejects_invalid_base64() {
    let err = crate::clipboard::copy_png_base64("@@@").expect_err("invalid base64");
    assert!(err.contains("无法解码"), "{err}");
}

#[cfg(target_os = "macos")]
#[ignore = "writes the system clipboard"]
#[test]
fn macos_clipboard_roundtrip_one_red_pixel() {
    crate::clipboard::copy_png_base64(&BASE64.encode(RED_1X1_PNG)).expect("write clipboard");
    let mut clipboard = arboard::Clipboard::new().expect("open clipboard");
    let image = clipboard.get_image().expect("read clipboard");
    assert_eq!(image.width, 1);
    assert_eq!(image.height, 1);
    assert_eq!(&image.bytes[..4], &[255, 0, 0, 255]);
}
