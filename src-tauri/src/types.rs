use serde::{Deserialize, Deserializer, Serialize};

/// Card exchange format with the LLM (Dayflow's ActivityCardData).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityCard {
    pub start_time: String,
    pub end_time: String,
    #[serde(default)]
    pub category: String,
    #[serde(default, deserialize_with = "string_or_default")]
    pub subcategory: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default, deserialize_with = "string_or_default")]
    pub detailed_summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distractions: Option<Vec<Distraction>>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient_app_sites")]
    pub app_sites: Option<AppSites>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Distraction {
    pub start_time: String,
    pub end_time: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AppSites {
    pub primary: Option<String>,
    pub secondary: Option<String>,
}

fn string_or_default<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v: Option<serde_json::Value> = Option::deserialize(d)?;
    Ok(match v {
        Some(serde_json::Value::String(s)) => s,
        Some(serde_json::Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    })
}

/// Accepts `{primary: "a", secondary: "b"}` where either value may be a string, an array (first
/// element is used) or null. Empty strings become None; both None → None.
fn lenient_app_sites<'de, D: Deserializer<'de>>(d: D) -> Result<Option<AppSites>, D::Error> {
    let v: Option<serde_json::Value> = Option::deserialize(d)?;
    Ok(v.and_then(|v| app_sites_from_value(&v)))
}

pub fn app_sites_from_value(v: &serde_json::Value) -> Option<AppSites> {
    fn pick(v: Option<&serde_json::Value>) -> Option<String> {
        let s = match v? {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(a) => a.first()?.as_str()?.to_string(),
            _ => return None,
        };
        let s = s.trim().to_string();
        if s.is_empty() { None } else { Some(s) }
    }
    let obj = v.as_object()?;
    let sites = AppSites { primary: pick(obj.get("primary")), secondary: pick(obj.get("secondary")) };
    if sites.primary.is_none() && sites.secondary.is_none() { None } else { Some(sites) }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Observation {
    pub start_ts: i64,
    pub end_ts: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub name: String,
    pub color: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub is_idle: bool,
}

pub fn default_categories() -> Vec<Category> {
    vec![
        Category {
            name: "Work".into(),
            color: "#B984FF".into(),
            description: "Career, school, or productivity-focused activities (projects, emails, assignments, video calls, learning skills, etc.)".into(),
            is_idle: false,
        },
        Category {
            name: "Personal".into(),
            color: "#6AADFF".into(),
            description: "Purposeful non-work activities or life tasks (paying bills, fitness tracking, meal planning, personal research, creative hobbies, etc.)".into(),
            is_idle: false,
        },
        Category {
            name: "Distraction".into(),
            color: "#FF5950".into(),
            description: "Passive consumption or aimless browsing (scrolling feeds, watching random videos, clicking through news, mindless games, etc.)".into(),
            is_idle: false,
        },
        Category { name: "Idle".into(), color: "#A0AEC0".into(), description: String::new(), is_idle: true },
    ]
}

/// Categories as fed to prompts (Dayflow's descriptorsForLLM): the idle category always carries
/// a fixed description.
pub fn categories_for_llm(cats: &[Category]) -> Vec<Category> {
    let mut out: Vec<Category> = cats.iter().cloned().collect();
    if !out.iter().any(|c| c.is_idle) {
        out.push(Category { name: "Idle".into(), color: "#A0AEC0".into(), description: String::new(), is_idle: true });
    }
    for c in out.iter_mut() {
        c.description = c.description.trim().to_string();
        if c.is_idle {
            c.description = "Use when the user is idle for more than half of this period.".into();
        }
    }
    out
}

/// Exact (case-insensitive) match → that name; idle aliases → idle category; else first category.
pub fn normalize_category(raw: &str, cats: &[Category]) -> String {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return cats.first().map(|c| c.name.clone()).unwrap_or_default();
    }
    let lower = cleaned.to_lowercase();
    if let Some(c) = cats.iter().find(|c| c.name.trim().to_lowercase() == lower) {
        return c.name.clone();
    }
    if let Some(idle) = cats.iter().find(|c| c.is_idle) {
        if lower == "idle" || lower == "idle time" || lower == idle.name.to_lowercase() {
            return idle.name.clone();
        }
    }
    cats.first().map(|c| c.name.clone()).unwrap_or_else(|| cleaned.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_lenient_cards() {
        let json = r#"[{"startTime":"2:30 PM","endTime":"2:45 PM","category":"work","subcategory":null,
            "title":"T","summary":"S","detailedSummary":"D","titleEvidence":{"x":1},
            "appSites":{"primary":["github.com","x"],"secondary":""}}]"#;
        let cards: Vec<ActivityCard> = serde_json::from_str(json).unwrap();
        assert_eq!(cards[0].subcategory, "");
        assert_eq!(cards[0].app_sites, Some(AppSites { primary: Some("github.com".into()), secondary: None }));
        let cats = default_categories();
        assert_eq!(normalize_category(&cards[0].category, &cats), "Work");
        assert_eq!(normalize_category("idle time", &cats), "Idle");
        assert_eq!(normalize_category("Gaming", &cats), "Work");
    }
}
