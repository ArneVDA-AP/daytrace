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
    /// What the category's time counts as in the focus and distraction numbers (Daily, Weekly,
    /// Goals, Your day so far, daytrace-mcp). Absent in settings written before it existed: then
    /// it follows the name (see `counts()`); `Settings::sanitized` fills it in on load and save.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counts_as: Option<CountsAs>,
}

/// Dayflow lets each day plan pick its focus and distraction categories
/// (Views/Components/DayCategorySelectionEditor.swift); Daytrace keeps one choice per category.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CountsAs {
    Focus,
    Distraction,
    Neither,
}

impl CountsAs {
    /// The value for a category without a stored choice, as Daytrace counted before the choice
    /// existed: a category named Distraction or Distractions is distraction, the idle category is
    /// neither, every other category is focus.
    pub fn derived(name: &str, is_idle: bool) -> CountsAs {
        if is_idle {
            return CountsAs::Neither;
        }
        match name.trim().to_lowercase().as_str() {
            "distraction" | "distractions" => CountsAs::Distraction,
            _ => CountsAs::Focus,
        }
    }
}

impl Category {
    pub fn new(name: &str, color: &str, description: &str, is_idle: bool) -> Category {
        Category { name: name.into(), color: color.into(), description: description.into(), is_idle, counts_as: None }
    }

    /// The stored choice, or the one derived from the name. The idle category always counts as
    /// neither: idle time is left out of focus, distraction and active time.
    pub fn counts(&self) -> CountsAs {
        if self.is_idle {
            return CountsAs::Neither;
        }
        self.counts_as.unwrap_or_else(|| CountsAs::derived(&self.name, false))
    }
}

/// Defaults for new installs. Work, Personal and Distraction use the validated palette of the UI
/// (src/design/tokens.css: blue, green, plum, at least 3:1 on every surface and distinct for
/// colour-blind readers); the UI maps these hex values to theme-aware tokens. Idle is drawn as a
/// hatch from its own token, so its stored colour is only a fallback.
pub fn default_categories() -> Vec<Category> {
    let with = |c: Category, counts: CountsAs| Category { counts_as: Some(counts), ..c };
    vec![
        with(
            Category::new(
                "Work",
                "#407BC5",
                "Career, school, or productivity-focused activities (projects, emails, assignments, video calls, learning skills, etc.)",
                false,
            ),
            CountsAs::Focus,
        ),
        with(
            Category::new(
                "Personal",
                "#287C42",
                "Purposeful non-work activities or life tasks (paying bills, fitness tracking, meal planning, personal research, creative hobbies, etc.)",
                false,
            ),
            CountsAs::Focus,
        ),
        with(
            Category::new(
                "Distraction",
                "#73176F",
                "Passive consumption or aimless browsing (scrolling feeds, watching random videos, clicking through news, mindless games, etc.)",
                false,
            ),
            CountsAs::Distraction,
        ),
        with(Category::new("Idle", "#A0AEC0", "", true), CountsAs::Neither),
    ]
}

/// Categories as fed to prompts (Dayflow's descriptorsForLLM): the idle category always carries
/// a fixed description.
pub fn categories_for_llm(cats: &[Category]) -> Vec<Category> {
    let mut out: Vec<Category> = cats.iter().cloned().collect();
    if !out.iter().any(|c| c.is_idle) {
        out.push(Category::new("Idle", "#A0AEC0", "", true));
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

    #[test]
    fn counts_as_follows_the_name_until_chosen() {
        // Settings written before the field existed.
        let old: Vec<Category> = serde_json::from_str(
            r##"[{"name":"Work","color":"#000000"},{"name":"Distractions","color":"#000000"},{"name":"Idle","color":"#000000","isIdle":true}]"##,
        )
        .unwrap();
        let counts: Vec<CountsAs> = old.iter().map(Category::counts).collect();
        assert_eq!(counts, vec![CountsAs::Focus, CountsAs::Distraction, CountsAs::Neither]);
        // A stored choice wins over the name, except for the idle category.
        let mut gaming = Category::new("Distraction", "#000000", "", false);
        gaming.counts_as = Some(CountsAs::Neither);
        assert_eq!(gaming.counts(), CountsAs::Neither);
        let mut idle = Category::new("Away", "#000000", "", true);
        idle.counts_as = Some(CountsAs::Focus);
        assert_eq!(idle.counts(), CountsAs::Neither);
        // Serialized as camelCase "countsAs" with lowercase values; absent when not chosen.
        let json = serde_json::to_string(&default_categories()[2]).unwrap();
        assert!(json.contains(r#""countsAs":"distraction""#), "{json}");
        assert!(!serde_json::to_string(&Category::new("X", "#000000", "", false)).unwrap().contains("countsAs"));
    }
}
