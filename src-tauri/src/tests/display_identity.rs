//! 展示身份静态文件。生成逻辑只在本模块，不进发布产物。

use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::{Brand, OfficialQuotaProvider, Source};

const UPDATE_ENV: &str = "MABIAO_UPDATE_DISPLAY_IDENTITY";
const REGEN_COMMAND: &str = "MABIAO_UPDATE_DISPLAY_IDENTITY=1 cargo test --manifest-path src-tauri/Cargo.toml --lib tests::display_identity::display_identity_file_matches_committed_snapshot -- --exact";

fn generated_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has a parent")
        .join("src/generated/displayIdentity.ts")
}

fn merged_display_names() -> Vec<(&'static str, &'static str)> {
    let mut names: Vec<(&str, &str)> = Source::ALL
        .iter()
        .copied()
        .map(|source| (source.as_str(), source.application_name()))
        .collect();
    for provider in OfficialQuotaProvider::ALL {
        let slug = provider.as_str();
        if names.iter().any(|(existing, _)| *existing == slug) {
            continue;
        }
        names.push((slug, provider.display_name()));
    }
    names
}

fn js_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn js_key(slug: &str) -> String {
    if is_identifier(slug) {
        slug.to_string()
    } else {
        js_string(slug)
    }
}

fn is_identifier(slug: &str) -> bool {
    let mut chars = slug.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' || first == '$' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
}

fn push_string_array(out: &mut String, name: &str, values: &[&str]) {
    out.push_str("export const ");
    out.push_str(name);
    out.push_str(" = [\n");
    for value in values {
        out.push_str("  ");
        out.push_str(&js_string(value));
        out.push_str(",\n");
    }
    out.push_str("] as const;\n");
}

fn push_string_record(out: &mut String, name: &str, entries: &[(&str, &str)]) {
    out.push_str("export const ");
    out.push_str(name);
    out.push_str(" = {\n");
    for (key, value) in entries {
        out.push_str("  ");
        out.push_str(&js_key(key));
        out.push_str(": ");
        out.push_str(&js_string(value));
        out.push_str(",\n");
    }
    out.push_str("} as const;\n");
}

fn merged_brands() -> Vec<(&'static str, Option<&'static str>)> {
    let mut brands: Vec<(&str, Option<&str>)> = Source::ALL
        .iter()
        .copied()
        .map(|source| (source.as_str(), Some(source.brand().as_str())))
        .collect();
    for provider in OfficialQuotaProvider::ALL {
        let slug = provider.as_str();
        if brands.iter().any(|(existing, _)| *existing == slug) {
            continue;
        }
        brands.push((slug, provider.brand().map(Brand::as_str)));
    }
    brands
}

fn push_optional_string_record(out: &mut String, name: &str, entries: &[(&str, Option<&str>)]) {
    out.push_str("export const ");
    out.push_str(name);
    out.push_str(" = {\n");
    for (key, value) in entries {
        out.push_str("  ");
        out.push_str(&js_key(key));
        out.push_str(": ");
        match value {
            Some(text) => out.push_str(&js_string(text)),
            None => out.push_str("null"),
        }
        out.push_str(",\n");
    }
    out.push_str("} as const;\n");
}

fn render_display_identity() -> String {
    let mut out = String::new();
    out.push_str("// 由展示身份快照测试生成。不要手改。\n");
    out.push_str("// 重新生成：\n// ");
    out.push_str(REGEN_COMMAND);
    out.push_str("\n\n");

    let source_slugs: Vec<&str> = Source::ALL.iter().copied().map(Source::as_str).collect();
    out.push_str("/** 来源 slug，顺序与 `Source::ALL` 一致。 */\n");
    push_string_array(&mut out, "SOURCE_SLUGS", &source_slugs);
    out.push('\n');

    let quota_slugs: Vec<&str> = OfficialQuotaProvider::ALL
        .iter()
        .copied()
        .map(OfficialQuotaProvider::as_str)
        .collect();
    out.push_str("/** 官方额度提供方 slug，顺序与 `OfficialQuotaProvider::ALL` 一致。 */\n");
    push_string_array(&mut out, "OFFICIAL_QUOTA_PROVIDER_SLUGS", &quota_slugs);
    out.push('\n');

    let brand_ids: Vec<&str> = Brand::ALL.iter().copied().map(Brand::as_str).collect();
    out.push_str("/** 品牌 ID，顺序与 `Brand::ALL` 一致。 */\n");
    push_string_array(&mut out, "BRAND_IDS", &brand_ids);
    out.push('\n');

    out.push_str("/** 按 slug 索引的显示名。重名 slug 以来源 `application_name` 为准。 */\n");
    push_string_record(&mut out, "DISPLAY_NAMES", &merged_display_names());
    out.push('\n');

    out.push_str("/** 按 slug 索引的品牌。重名 slug 以来源为准；没有品牌时为 null。 */\n");
    push_optional_string_record(&mut out, "BRAND_BY_SLUG", &merged_brands());
    out.push('\n');
    out.push_str("export type UsageSource = (typeof SOURCE_SLUGS)[number];\n");
    out.push_str("export type BrandId = (typeof BRAND_IDS)[number];\n");
    out
}

fn write_generated(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create display identity dir");
    }
    let tmp = path.with_extension("ts.tmp");
    fs::write(&tmp, contents).expect("write display identity tmp");
    fs::rename(&tmp, path).expect("replace display identity file");
}

#[test]
fn display_identity_file_matches_committed_snapshot() {
    let rendered = render_display_identity();
    let path = generated_path();
    if std::env::var(UPDATE_ENV).ok().as_deref() == Some("1") {
        write_generated(&path, &rendered);
        return;
    }
    let committed = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "读不到展示身份生成文件 {}：{error}\n重新生成：\n  {REGEN_COMMAND}",
            path.display()
        );
    });
    assert_eq!(
        committed,
        rendered,
        "展示身份生成文件与 Rust 登记不一致：{}\n重新生成：\n  {}",
        path.display(),
        REGEN_COMMAND
    );
}
