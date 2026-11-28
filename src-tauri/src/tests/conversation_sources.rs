use std::path::Path;

use crate::conversation::{conversation_adapter, CONVERSATION_ADAPTERS, CONVERSATION_SOURCES};
use crate::domain::Source;

#[test]
fn conversation_sources_cover_every_registered_source_once() {
    let adapters = CONVERSATION_ADAPTERS;
    assert_eq!(
        adapters.len(),
        Source::ALL.len(),
        "对话记录适配器表不能多行也不能少行：人为删掉一行必须使本测试失败"
    );
    for source in Source::ALL {
        let matches = adapters
            .iter()
            .filter(|adapter| adapter.source == source)
            .count();
        assert_eq!(
            matches,
            1,
            "{} 应在对话记录适配器表中恰好一行",
            source.as_str()
        );
        assert_eq!(
            conversation_adapter(source)
                .expect("已登记来源必须能取到对话记录适配器")
                .source,
            source
        );
    }

    let derived: Vec<Source> = adapters.iter().map(|adapter| adapter.source).collect();
    assert_eq!(
        CONVERSATION_SOURCES,
        derived.as_slice(),
        "对话记录来源名单必须与对话记录适配器表的来源一致，顺序也一致"
    );
    for source in Source::ALL {
        let matches = CONVERSATION_SOURCES
            .iter()
            .filter(|listed| **listed == source)
            .count();
        assert_eq!(
            matches,
            1,
            "{} 应在推导出的对话记录来源名单里恰好一次",
            source.as_str()
        );
    }
}

#[test]
fn conversation_adapters_declare_bodyless_sources_accurately() {
    let bodyless: Vec<Source> = Source::ALL
        .iter()
        .copied()
        .filter(|&source| {
            !conversation_adapter(source)
                .expect("已登记来源必须能取到对话记录适配器")
                .has_conversation_body
        })
        .collect();
    assert_eq!(
        bodyless,
        vec![Source::Aider, Source::Warp, Source::Windsurf],
        "本机不存对话正文的来源必须通过 CONVERSATION_ADAPTERS::has_conversation_body 显式声明"
    );

    for source in bodyless {
        let adapter = conversation_adapter(source).expect("bodyless source adapter");
        let batch =
            (adapter.index)(Path::new("unused")).expect("无正文来源的 index 应无错返回空目录");
        assert!(
            batch.conversations.is_empty(),
            "{} 声明无本机正文，但 index 返回了会话",
            source.as_str()
        );
    }
}
