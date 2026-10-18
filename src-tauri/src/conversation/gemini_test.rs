use base64::prelude::*;
use serde_json::json;

use super::parse;
use crate::domain::{
    ConversationAttachmentKind, ConversationAttachmentStatus, ConversationEventKind as EventKind,
};

fn test_png_bytes() -> Vec<u8> {
    let pixels = image::RgbaImage::from_pixel(2, 2, image::Rgba([24, 160, 200, 255]));
    let mut output = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(pixels)
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    output.into_inner()
}

fn write_session(contents: serde_json::Value) -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp
        .path()
        .join(".gemini/tmp/proj/chats/session-gemini-media.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, contents.to_string()).unwrap();
    (temp, path)
}

#[test]
fn image_only_inline_data_message_is_kept_as_embedded_attachment() {
    let png = test_png_bytes();
    let (_temp, path) = write_session(json!({
        "sessionId": "gemini-media",
        "messages": [{
            "id": "u1",
            "timestamp": "2026-09-03T08:00:00Z",
            "type": "user",
            "content": [{
                "inlineData": {
                    "mimeType": "image/png",
                    "data": BASE64_STANDARD.encode(&png)
                }
            }]
        }]
    }));

    let parsed = parse(&path, true).unwrap();
    assert_eq!(parsed.messages.len(), 1);
    assert!(parsed.messages[0].text.is_empty());
    let event = parsed
        .events
        .iter()
        .find(|event| event.kind == EventKind::Message)
        .unwrap();
    assert_eq!(event.attachments.len(), 1);
    assert_eq!(event.attachments[0].kind, ConversationAttachmentKind::Image);
    assert_eq!(
        event.attachments[0].status,
        ConversationAttachmentStatus::Embedded
    );
    assert_eq!(event.attachments[0].media_type, "image/png");
    assert_eq!(event.attachments[0].original_path, "内嵌图片数据");
    assert!(
        event.details.get("content").is_none(),
        "message details must not keep inline image bodies"
    );
}

#[test]
fn remote_http_image_is_unsupported() {
    let (_temp, path) = write_session(json!({
        "sessionId": "gemini-remote",
        "messages": [{
            "id": "u1",
            "timestamp": "2026-09-03T08:00:00Z",
            "type": "user",
            "content": [{
                "type": "image_url",
                "image_url": "https://example.com/shot.png"
            }]
        }]
    }));

    let parsed = parse(&path, true).unwrap();
    let event = parsed
        .events
        .iter()
        .find(|event| event.kind == EventKind::Message)
        .unwrap();
    assert_eq!(event.attachments.len(), 1);
    assert_eq!(
        event.attachments[0].status,
        ConversationAttachmentStatus::Unsupported
    );
}

#[test]
fn compression_history_item_becomes_system_status_boundary() {
    let (_temp, path) = write_session(json!({
        "sessionId": "gemini-compact",
        "messages": [
            {
                "id": "u1",
                "timestamp": "2026-09-03T08:00:00Z",
                "type": "user",
                "content": "Keep going"
            },
            {
                "id": "c1",
                "timestamp": "2026-09-03T08:00:10Z",
                "type": "compression",
                "compression": {
                    "isPending": false,
                    "originalTokenCount": 120000,
                    "newTokenCount": 18000,
                    "compressionStatus": 1
                }
            },
            {
                "id": "a1",
                "timestamp": "2026-09-03T08:00:11Z",
                "type": "gemini",
                "model": "gemini-flash",
                "content": "Compressed context, continuing."
            }
        ]
    }));

    let parsed = parse(&path, true).unwrap();
    let boundary = parsed
        .events
        .iter()
        .find(|event| event.name.as_deref() == Some("compression"))
        .unwrap();
    assert_eq!(boundary.kind, EventKind::SystemStatus);
    assert_eq!(
        boundary.text.as_deref(),
        Some("上下文已压缩：120000 → 18000 tokens")
    );
    assert_eq!(parsed.messages.len(), 2);
}
