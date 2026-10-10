use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PriceOrigin {
    #[default]
    User,
    Snapshot,
}

impl PriceOrigin {
    pub fn is_user(&self) -> bool {
        matches!(self, PriceOrigin::User)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PriceOrigin::User => "user",
            PriceOrigin::Snapshot => "snapshot",
        }
    }
}

/// 单条消耗记录的费用来源，给界面展示用。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CostSource {
    Native,
    User,
    Snapshot,
    #[default]
    None,
}

impl CostSource {
    pub fn as_str(self) -> &'static str {
        match self {
            CostSource::Native => "native",
            CostSource::User => "user",
            CostSource::Snapshot => "snapshot",
            CostSource::None => "none",
        }
    }

    pub fn from_sql(value: &str) -> Self {
        match value {
            "native" => CostSource::Native,
            "user" => CostSource::User,
            "snapshot" => CostSource::Snapshot,
            _ => CostSource::None,
        }
    }

    pub fn note(self) -> &'static str {
        match self {
            CostSource::Native => "来源自带",
            CostSource::User => "用户单价",
            CostSource::Snapshot => "LiteLLM 快照",
            CostSource::None => "单价未配置",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PriceEntry {
    pub model: String,
    pub provider: Option<String>,
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_creation: f64,
    /// 旧文件没有该字段时视为用户单价。
    #[serde(default, skip_serializing_if = "PriceOrigin::is_user")]
    pub origin: PriceOrigin,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PriceTable {
    pub prices: Vec<PriceEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DerivedCost {
    pub amount: Option<f64>,
    pub unpriced: bool,
    pub source_native: bool,
    pub cost_source: CostSource,
}

impl DerivedCost {
    pub fn cost_note(&self) -> String {
        self.cost_source.note().to_string()
    }
}
