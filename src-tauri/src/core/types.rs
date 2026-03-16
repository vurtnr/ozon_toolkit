use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub title: String,
    pub price: String,
    #[serde(rename = "itemUrl")]
    pub item_url: String,
    #[serde(rename = "imageUrl")]
    pub image_url: String,
    #[serde(
        rename = "cosScore",
        default,
        deserialize_with = "deserialize_cos_score_permille",
        serialize_with = "serialize_cos_score_permille"
    )]
    pub cos_score_permille: u16,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawCosScore {
    Number(f64),
    Text(String),
}

fn deserialize_cos_score_permille<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<RawCosScore>::deserialize(deserializer)?;
    let value = match raw {
        Some(RawCosScore::Number(value)) => value,
        Some(RawCosScore::Text(value)) => value.parse::<f64>().unwrap_or(0.0),
        None => 0.0,
    };

    if !value.is_finite() {
        return Ok(0);
    }

    Ok((value.clamp(0.0, 1.0) * 1000.0).round() as u16)
}

fn serialize_cos_score_permille<S>(value: &u16, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_f64((*value as f64) / 1000.0)
}

impl Candidate {
    pub fn cos_score(&self) -> f32 {
        self.cos_score_permille as f32 / 1000.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchSummary {
    NoMatch,
    Cheapest(Candidate),
    MatchedButPriceUnavailable { total_matches: usize },
}
