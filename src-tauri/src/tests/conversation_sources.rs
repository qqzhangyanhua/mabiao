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
