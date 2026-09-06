pub struct SessionSummary {
    pub project: String,
    pub title: String,
    pub summary: String,
}

pub fn map_prompt(compressed: &str) -> String {
    format!(
        "请用一句中文概括下面这个会话做了什么。只输出 JSON：{{\"summary\":\"...\"}}，不要其它文字。\n\n{compressed}"
    )
}

pub fn reduce_prompt(summaries: &[SessionSummary]) -> String {
    let mut body = String::from(
        "下面是一段时间内各会话的一句话摘要。请汇总成中文工作纪要，只输出 JSON，不要其它文字。\n\
条目数量 3 到 6，按内容多少自己决定，不要硬凑。\n\
结构：{\"headline\":\"一句话概括这段时间的主线\",\"entries\":[{\"title\":\"小标题\",\"detail\":\"一句话说明\",\"project\":\"目录名\"}],\"closing\":\"一句收尾\"}\n\n\
摘要：\n",
    );
    for (index, item) in summaries.iter().enumerate() {
        body.push_str(&format!(
            "{}. [{}] {}：{}\n",
            index + 1,
            item.project,
            item.title,
            item.summary
        ));
    }
    body
}
