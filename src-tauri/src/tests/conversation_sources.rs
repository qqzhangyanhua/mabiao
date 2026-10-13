use crate::conversation::CONVERSATION_SOURCES;
use crate::domain::Source;

#[test]
fn conversation_sources_cover_every_registered_source_once() {
    assert_eq!(
        CONVERSATION_SOURCES.len(),
        Source::ALL.len(),
        "推导出的对话记录来源名单不能多也不能少：适配器表多一行或少一行必须使本测试失败"
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
