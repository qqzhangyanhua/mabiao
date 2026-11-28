//! 展示身份与品牌图形静态文件。生成逻辑只在本模块，不进发布产物。
//!
//! 同一批生成内容双写到桌面端 `src/generated/` 与远程服务管理网页
//! `server/web/src/generated/`，保持管理网页自包含且不引入跨包导入。

mod marks;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;

use crate::conversation::conversation_adapter;
use crate::domain::{Brand, OfficialQuotaProvider, Source};
use marks::{
    brand_chart_color, brand_mark_spec, unknown_mark_spec, BrandMarkSpec, SvgPathSpec,
    DEFAULT_BRAND_CHART_COLOR,
};

const UPDATE_ENV: &str = "MABIAO_UPDATE_DISPLAY_IDENTITY";
const REGEN_COMMAND: &str = "MABIAO_UPDATE_DISPLAY_IDENTITY=1 cargo test --manifest-path src-tauri/Cargo.toml --lib tests::display_identity::display_identity_file_matches_committed_snapshot -- --exact";
const ADMIN_WEB_MAX_CODE_LINES: usize = 400;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has a parent")
}

fn canonical_icon_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/source-icons")
}

fn generated_targets() -> [PathBuf; 2] {
    let root = repo_root();
    [
        root.join("src/generated"),
        root.join("server/web/src/generated"),
    ]
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

fn resume_templates() -> Vec<(&'static str, Option<&'static str>)> {
    Source::ALL
        .iter()
        .copied()
        .map(|source| (source.as_str(), source.resume_template()))
        .collect()
}

fn bodyless_conversation_sources() -> Vec<&'static str> {
    Source::ALL
        .iter()
        .copied()
        .filter(|&source| {
            !conversation_adapter(source)
                .expect("every Source::ALL entry has a conversation adapter")
                .has_conversation_body
        })
        .map(Source::as_str)
        .collect()
}

fn brand_chart_colors() -> Vec<(&'static str, &'static str)> {
    Brand::ALL
        .iter()
        .copied()
        .map(|brand| (brand.as_str(), brand_chart_color(brand)))
        .collect()
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

fn push_header(out: &mut String) {
    out.push_str("// 由展示身份快照测试生成。不要手改。\n");
    out.push_str("// 重新生成：\n// ");
    out.push_str(REGEN_COMMAND);
    out.push_str("\n\n");
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
    push_header(&mut out);

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

    out.push_str(
        "/** 本机不存对话正文、目录按设计为空的来源 slug，由 `CONVERSATION_ADAPTERS` 推导。 */\n",
    );
    push_string_array(
        &mut out,
        "BODYLESS_CONVERSATION_SOURCES",
        &bodyless_conversation_sources(),
    );
    out.push('\n');

    out.push_str("/** 按 slug 索引的显示名。重名 slug 以来源 `application_name` 为准。 */\n");
    push_string_record(&mut out, "DISPLAY_NAMES", &merged_display_names());
    out.push('\n');

    out.push_str("/** 按 slug 索引的品牌。重名 slug 以来源为准；没有品牌时为 null。 */\n");
    push_optional_string_record(&mut out, "BRAND_BY_SLUG", &merged_brands());
    out.push('\n');

    out.push_str(
        "/** 按来源 slug 索引的恢复命令模板，`{id}` 占位会话 ID；没有公开恢复命令时为 null。只有来源才有模板，只在额度里出现的 slug 没有这个字段。 */\n",
    );
    push_optional_string_record(&mut out, "RESUME_TEMPLATES", &resume_templates());
    out.push('\n');

    out.push_str("/** 按品牌 ID 索引的确定性图表色（`#rrggbb`）。 */\n");
    push_string_record(&mut out, "BRAND_CHART_COLORS", &brand_chart_colors());
    out.push('\n');

    out.push_str("/** 未知或无品牌 slug 的默认图表色。 */\n");
    out.push_str("export const DEFAULT_BRAND_CHART_COLOR = ");
    out.push_str(&js_string(DEFAULT_BRAND_CHART_COLOR));
    out.push_str(";\n\n");

    out.push_str("export type UsageSource = (typeof SOURCE_SLUGS)[number];\n");
    out.push_str("export type BrandId = (typeof BRAND_IDS)[number];\n");
    out
}

fn format_svg_path(path: &SvgPathSpec) -> String {
    let mut parts = vec![format!("d: {}", js_string(path.d))];
    if let Some(fill) = path.fill {
        parts.push(format!("fill: {}", js_string(fill)));
    }
    if let Some(fill_rule) = path.fill_rule {
        parts.push(format!("fillRule: {}", js_string(fill_rule)));
    }
    if let Some(clip_rule) = path.clip_rule {
        parts.push(format!("clipRule: {}", js_string(clip_rule)));
    }
    if let Some(opacity) = path.opacity {
        parts.push(format!("opacity: {opacity}"));
    }
    if let Some(stroke) = path.stroke {
        parts.push(format!("stroke: {}", js_string(stroke)));
    }
    if let Some(stroke_width) = path.stroke_width {
        parts.push(format!("strokeWidth: {}", js_string(stroke_width)));
    }
    if let Some(stroke_linecap) = path.stroke_linecap {
        parts.push(format!("strokeLinecap: {}", js_string(stroke_linecap)));
    }
    if let Some(stroke_linejoin) = path.stroke_linejoin {
        parts.push(format!("strokeLinejoin: {}", js_string(stroke_linejoin)));
    }
    format!("{{ {} }}", parts.join(", "))
}

fn encode_asset_data_uri(file_name: &str, mime: &str) -> String {
    let path = canonical_icon_dir().join(file_name);
    let bytes = fs::read(&path).unwrap_or_else(|error| {
        panic!("读不到品牌图标源资源 {}：{error}", path.display());
    });
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:{mime};base64,{encoded}")
}

fn format_mark_value(spec: &BrandMarkSpec, indent: &str) -> String {
    match spec {
        BrandMarkSpec::Svg { view_box, paths } if paths.len() == 1 => {
            format!(
                "{{ kind: \"svg\", viewBox: {}, paths: [{}] }}",
                js_string(view_box),
                format_svg_path(&paths[0])
            )
        }
        BrandMarkSpec::Svg { view_box, paths } => {
            let mut out = format!(
                "{{ kind: \"svg\", viewBox: {}, paths: [\n",
                js_string(view_box)
            );
            for path in paths {
                out.push_str(indent);
                out.push_str("  ");
                out.push_str(&format_svg_path(path));
                out.push_str(",\n");
            }
            out.push_str(indent);
            out.push_str("] }");
            out
        }
        BrandMarkSpec::ImageAsset {
            file_name,
            mime,
            plate,
        } => {
            let src = encode_asset_data_uri(file_name, mime);
            if *plate {
                format!(
                    "{{ kind: \"image\", src: {}, plate: true }}",
                    js_string(&src)
                )
            } else {
                format!("{{ kind: \"image\", src: {} }}", js_string(&src))
            }
        }
    }
}

fn render_brand_marks() -> String {
    let mut out = String::new();
    push_header(&mut out);

    out.push_str("import type { BrandId } from \"./displayIdentity\";\n\n");
    out.push_str("export type BrandSvgPath = {\n");
    out.push_str("  readonly d: string;\n");
    out.push_str("  readonly fill?: string;\n");
    out.push_str("  readonly fillRule?: \"evenodd\";\n");
    out.push_str("  readonly clipRule?: \"evenodd\";\n");
    out.push_str("  readonly opacity?: number;\n");
    out.push_str("  readonly stroke?: string;\n");
    out.push_str("  readonly strokeWidth?: string;\n");
    out.push_str("  readonly strokeLinecap?: \"round\";\n");
    out.push_str("  readonly strokeLinejoin?: \"round\";\n");
    out.push_str("};\n\n");

    out.push_str("export type BrandMark =\n");
    out.push_str(
        "  | { readonly kind: \"svg\"; readonly viewBox: string; readonly paths: readonly BrandSvgPath[] }\n",
    );
    out.push_str(
        "  | { readonly kind: \"image\"; readonly src: string; readonly plate?: true };\n\n",
    );

    out.push_str("/** 未知来源或无品牌时的通用占位图形。 */\n");
    out.push_str("export const UNKNOWN_BRAND_MARK: BrandMark = ");
    out.push_str(&format_mark_value(&unknown_mark_spec(), ""));
    out.push_str(";\n\n");

    out.push_str("/** 按品牌 ID 索引的矢量路径或内联位图图形，桌面端与管理网页共享。 */\n");
    out.push_str("export const BRAND_MARKS: Record<BrandId, BrandMark> = {\n");
    for brand in Brand::ALL {
        out.push_str("  ");
        out.push_str(&js_key(brand.as_str()));
        out.push_str(": ");
        out.push_str(&format_mark_value(&brand_mark_spec(brand), "  "));
        out.push_str(",\n");
    }
    out.push_str("};\n");
    out
}

fn count_non_blank_non_comment_lines(source: &str) -> usize {
    source
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty()
                && !trimmed.starts_with("//")
                && !trimmed.starts_with("/*")
                && !trimmed.starts_with('*')
        })
        .count()
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
    let files = [
        ("displayIdentity.ts", render_display_identity()),
        ("brandMarks.ts", render_brand_marks()),
    ];
    let update = std::env::var(UPDATE_ENV).ok().as_deref() == Some("1");
    for dir in generated_targets() {
        for (file_name, rendered) in &files {
            let path = dir.join(file_name);
            if update {
                write_generated(&path, rendered);
                continue;
            }
            let committed = fs::read_to_string(&path).unwrap_or_else(|error| {
                panic!(
                    "读不到展示身份生成文件 {}：{error}\n重新生成：\n  {REGEN_COMMAND}",
                    path.display()
                );
            });
            assert_eq!(
                committed,
                *rendered,
                "展示身份生成文件与 Rust 登记不一致：{}\n重新生成：\n  {}",
                path.display(),
                REGEN_COMMAND
            );
        }
    }
}

#[test]
fn generated_identity_files_stay_under_admin_web_line_limit() {
    for (name, rendered) in [
        ("displayIdentity.ts", render_display_identity()),
        ("brandMarks.ts", render_brand_marks()),
    ] {
        let lines = count_non_blank_non_comment_lines(&rendered);
        assert!(
            lines <= ADMIN_WEB_MAX_CODE_LINES,
            "{name} 有 {lines} 行有效代码，超过管理网页 ESLint max-lines={ADMIN_WEB_MAX_CODE_LINES} 上限"
        );
    }
}

#[test]
fn brand_chart_colors_are_deterministic_hex_and_preserve_signature_brands() {
    for brand in Brand::ALL {
        let hex = brand_chart_color(brand);
        assert!(
            hex.len() == 7
                && hex.starts_with('#')
                && hex[1..].bytes().all(|b| b.is_ascii_hexdigit()),
            "品牌 {} 的图表色不是 #rrggbb：{hex}",
            brand.as_str()
        );
    }
    assert_eq!(
        brand_chart_color(Brand::Agy),
        brand_chart_color(Brand::Gemini)
    );
    assert_eq!(brand_chart_color(Brand::Claude), "#ea580c");
    assert_eq!(brand_chart_color(Brand::Cursor), "#64748b");
}

#[test]
fn canonical_source_icon_assets_have_no_orphans_or_duplicates() {
    let referenced: BTreeSet<&str> = Brand::ALL
        .iter()
        .copied()
        .filter_map(|brand| match brand_mark_spec(brand) {
            BrandMarkSpec::ImageAsset { file_name, .. } => Some(file_name),
            BrandMarkSpec::Svg { .. } => None,
        })
        .collect();

    let dir = canonical_icon_dir();
    let on_disk: BTreeSet<String> = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("读不到 {}：{error}", dir.display()))
        .map(|entry| {
            entry
                .expect("read source-icons entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    let expected: BTreeSet<String> = referenced.into_iter().map(str::to_string).collect();
    assert_eq!(
        on_disk, expected,
        "src-tauri/assets/source-icons/ 与 marks.rs 引用的位图清单必须一一对应"
    );
}
