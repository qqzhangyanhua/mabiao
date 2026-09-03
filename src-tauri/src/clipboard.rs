//! 把 PNG 解码成 RGBA 并写入系统剪贴板。不落盘。
//!
//! 写剪贴板要的是未预乘 RGBA，不是 PNG 字节。Linux/X11 在 `Clipboard` drop
//! 之后会丢掉选区，所以这里把实例留到进程退出。

use std::borrow::Cow;
use std::sync::Mutex;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

#[derive(Debug)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

const PNG_MAGIC: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

pub fn png_to_rgba(png: &[u8]) -> Result<RgbaImage, String> {
    if png.is_empty() {
        return Err("海报图片是空的".into());
    }
    if !png.starts_with(PNG_MAGIC) {
        return Err("无法解码海报 PNG：不是 PNG".into());
    }
    let image =
        image::load_from_memory(png).map_err(|error| format!("无法解码海报 PNG：{error}"))?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 {
        return Err("海报图片是空的".into());
    }
    Ok(RgbaImage {
        width,
        height,
        rgba: rgba.into_raw(),
    })
}

pub fn copy_png_base64(base64: &str) -> Result<(), String> {
    let png = BASE64
        .decode(base64.as_bytes())
        .map_err(|error| format!("无法解码海报 PNG：{error}"))?;
    copy_rgba(png_to_rgba(&png)?)
}

fn copy_rgba(image: RgbaImage) -> Result<(), String> {
    let mut guard = CLIPBOARD
        .lock()
        .map_err(|_| "无法打开系统剪贴板：锁已损坏".to_string())?;
    if guard.is_none() {
        *guard = Some(
            arboard::Clipboard::new().map_err(|error| format!("无法打开系统剪贴板：{error}"))?,
        );
    }
    let clipboard = guard
        .as_mut()
        .ok_or_else(|| "无法打开系统剪贴板".to_string())?;
    clipboard
        .set_image(arboard::ImageData {
            width: image.width as usize,
            height: image.height as usize,
            bytes: Cow::Owned(image.rgba),
        })
        .map_err(|error| format!("写入系统剪贴板失败：{error}"))
}
