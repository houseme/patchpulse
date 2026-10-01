use std::collections::BTreeMap;

use jiff::{
    Timestamp,
    civil::{Date, DateTime},
    tz::Offset,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchStatus {
    Installed,
    Pending,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchSource {
    Wmi,
    Wua,
    PowerShell,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchRecord {
    pub kb_id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub severity: Option<String>,
    pub installed_on: Option<Timestamp>,
    #[serde(default)]
    pub installed_date: Option<Date>,
    #[serde(default)]
    pub installed_on_raw: Option<String>,
    pub status: PatchStatus,
    pub reboot_required: bool,
    pub source: PatchSource,
    #[serde(default)]
    pub sources: Vec<PatchSource>,
    #[serde(default)]
    pub update_id: Option<String>,
}

impl PatchRecord {
    pub fn new(kb_id: String, status: PatchStatus, source: PatchSource) -> Self {
        Self {
            kb_id,
            title: None,
            description: None,
            category: None,
            severity: None,
            installed_on: None,
            installed_date: None,
            installed_on_raw: None,
            status,
            reboot_required: false,
            source,
            sources: vec![source],
            update_id: None,
        }
    }

    /// Combine complementary metadata without depending on completion order.
    pub fn merge(&mut self, other: &Self) {
        fn richer(left: &mut Option<String>, right: &Option<String>) {
            if let Some(right) = right
                && left
                    .as_ref()
                    .is_none_or(|left| (right.len(), right) > (left.len(), left))
            {
                *left = Some(right.clone());
            }
        }
        richer(&mut self.title, &other.title);
        richer(&mut self.description, &other.description);
        richer(&mut self.update_id, &other.update_id);
        if self.category != other.category {
            let categories: std::collections::BTreeSet<_> = self
                .category
                .iter()
                .chain(other.category.iter())
                .flat_map(|value| value.split(", "))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect();
            self.category = if categories.is_empty() {
                None
            } else {
                Some(categories.into_iter().collect::<Vec<_>>().join(", "))
            };
        }
        fn severity_rank(value: &str) -> u8 {
            ["Low", "Moderate", "Important", "Critical"]
                .iter()
                .position(|level| level.eq_ignore_ascii_case(value.trim()))
                .map_or(0, |index| index as u8 + 1)
        }
        if other.severity.as_ref().is_some_and(|value| {
            self.severity.as_ref().is_none_or(|current| {
                (severity_rank(value), value) > (severity_rank(current), current)
            })
        }) {
            self.severity.clone_from(&other.severity);
        }
        // Installation fields describe one observation; never mix different dates and raw values.
        if (other.installation_key(), other.installed_on_raw.as_deref())
            > (self.installation_key(), self.installed_on_raw.as_deref())
        {
            self.installed_on = other.installed_on;
            self.installed_date = other.installed_date;
            self.installed_on_raw.clone_from(&other.installed_on_raw);
        }
        self.reboot_required |= other.reboot_required;
        self.sources.extend_from_slice(&other.sources);
        self.sources.extend([self.source, other.source]);
        self.sources.sort_unstable();
        self.sources.dedup();
        self.source = self.source.min(other.source);
    }

    pub(crate) fn installation_key(&self) -> (Option<Date>, Option<Timestamp>) {
        (
            self.installed_on
                .map(|t| t.to_zoned(jiff::tz::TimeZone::UTC).date())
                .or(self.installed_date),
            self.installed_on,
        )
    }
}

pub fn normalize_kb(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let digits = if raw.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("kb")) {
        raw.get(2..)?
    } else {
        raw
    };
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!("KB{digits}"))
}

pub fn dedupe<'a>(records: impl IntoIterator<Item = &'a PatchRecord>) -> Vec<PatchRecord> {
    let mut map: BTreeMap<&str, PatchRecord> = BTreeMap::new();
    for record in records {
        match map.entry(&record.kb_id) {
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.get_mut().merge(record)
            }
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(record.clone());
            }
        }
    }
    map.into_values().collect()
}

/// Parse an offset-bearing CIM value without treating local clock time as UTC.
pub fn parse_cim_timestamp(value: &str) -> Option<Timestamp> {
    let value = value.trim();
    if value.len() != 25
        || !value.is_ascii()
        || value.as_bytes()[14] != b'.'
        || !value[..14].bytes().all(|c| c.is_ascii_digit())
        || !value[15..21].bytes().all(|c| c.is_ascii_digit())
        || !value[22..].bytes().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let datetime = DateTime::strptime("%Y%m%d%H%M%S", &value[..14]).ok()?;
    let microseconds: i32 = value[15..21].parse().ok()?;
    if !(0..1_000_000).contains(&microseconds) {
        return None;
    }
    let sign = match value.as_bytes()[21] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let minutes: i32 = value[22..].parse().ok()?;
    let timestamp = Offset::from_seconds(sign * minutes * 60)
        .ok()?
        .to_timestamp(datetime)
        .ok()?;
    timestamp
        .checked_add(jiff::SignedDuration::from_micros(i64::from(microseconds)))
        .ok()
}

/// Date-only and locale-ambiguous inputs never manufacture a UTC timestamp.
pub fn parse_installed_date(value: &str) -> (Option<Timestamp>, Option<Date>) {
    let value = value.trim();
    if let Ok(timestamp) = value.parse::<Timestamp>() {
        return (Some(timestamp), None);
    }
    if let Some(timestamp) = parse_cim_timestamp(value) {
        return (Some(timestamp), None);
    }
    if let Ok(date) = value.parse::<Date>() {
        return (None, Some(date));
    }
    // Accept locale dates only when both date orders agree or exactly one is valid.
    for (mdy_format, dmy_format) in [("%m/%d/%Y", "%d/%m/%Y"), ("%m-%d-%Y", "%d-%m-%Y")] {
        let mdy = Date::strptime(mdy_format, value).ok();
        let dmy = Date::strptime(dmy_format, value).ok();
        match (mdy, dmy) {
            (Some(a), Some(b)) if a == b => return (None, Some(a)),
            (Some(date), None) | (None, Some(date)) => return (None, Some(date)),
            _ => {}
        }
    }
    // Legacy QuickFixEngineering can return a hexadecimal Windows FILETIME.
    if value.len() == 16
        && value.bytes().all(|c| c.is_ascii_hexdigit())
        && let Ok(ticks) = u64::from_str_radix(value, 16)
    {
        let seconds = (ticks / 10_000_000) as i64 - 11_644_473_600;
        let nanos = ((ticks % 10_000_000) * 100) as i32;
        if let Ok(timestamp) = Timestamp::new(seconds, nanos) {
            return (Some(timestamp), None);
        }
    }
    if value.len() == 8
        && value.bytes().all(|c| c.is_ascii_digit())
        && let Ok(date) = Date::strptime("%Y%m%d", value)
    {
        return (None, Some(date));
    }
    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kb_normalization_rejects_invalid_identifiers() {
        assert_eq!(normalize_kb(" kb123 ").as_deref(), Some("KB123"));
        assert_eq!(normalize_kb("456").as_deref(), Some("KB456"));
        for value in ["KB", "abc123", "💡1", "Kß12", "", "1 OR 1"] {
            assert!(normalize_kb(value).is_none());
        }
    }

    #[test]
    fn cim_offset_and_fraction_are_preserved() {
        assert_eq!(
            parse_cim_timestamp("20250312000000.123456+480")
                .unwrap()
                .to_string(),
            "2025-03-11T16:00:00.123456Z"
        );
        assert!(parse_cim_timestamp("💡💡💡💡💡💡💡").is_none());
        assert!(parse_cim_timestamp("20251312000000.000000+480").is_none());
    }

    #[test]
    fn civil_dates_do_not_invent_an_instant() {
        let (instant, date) = parse_installed_date("3/23/2025");
        assert!(instant.is_none());
        assert_eq!(date.unwrap().to_string(), "2025-03-23");
        assert_eq!(parse_installed_date("3/12/2025"), (None, None));
        assert_eq!(
            parse_installed_date("23-10-2013").1.unwrap().to_string(),
            "2013-10-23"
        );
        assert_eq!(
            parse_installed_date("019DB1DED53E8000")
                .0
                .unwrap()
                .to_string(),
            "1970-01-01T00:00:00Z"
        );
    }

    #[test]
    fn merging_preserves_complementary_fields_and_is_deterministic() {
        let mut a = PatchRecord::new("KB1".into(), PatchStatus::Installed, PatchSource::Wmi);
        a.description = Some("Security update".into());
        let mut b = PatchRecord::new("KB1".into(), PatchStatus::Installed, PatchSource::Wua);
        b.title = Some("Cumulative update".into());
        b.reboot_required = true;
        let forward = dedupe([&a, &b]);
        let backward = dedupe([&b, &a]);
        assert_eq!(forward, backward);
        assert!(forward[0].title.is_some() && forward[0].description.is_some());
        assert!(forward[0].reboot_required);
        assert_eq!(forward[0].sources.len(), 2);
    }

    #[test]
    fn severity_uses_risk_order_and_categories_are_combined() {
        let mut important = PatchRecord::new("KB1".into(), PatchStatus::Pending, PatchSource::Wmi);
        important.severity = Some("Important".into());
        important.category = Some("QuickFixEngineering".into());
        let mut critical = PatchRecord::new("KB1".into(), PatchStatus::Pending, PatchSource::Wua);
        critical.severity = Some("Critical".into());
        critical.category = Some("Security Updates".into());
        let forward = dedupe([&important, &critical]);
        assert_eq!(forward, dedupe([&critical, &important]));
        assert_eq!(forward[0].severity.as_deref(), Some("Critical"));
        assert_eq!(
            forward[0].category.as_deref(),
            Some("QuickFixEngineering, Security Updates")
        );
    }

    #[test]
    fn installation_fields_remain_a_coherent_observation() {
        let mut old = PatchRecord::new("KB1".into(), PatchStatus::Installed, PatchSource::Wmi);
        old.installed_on = Some("2026-08-01T00:00:00Z".parse().unwrap());
        old.installed_on_raw = Some("20260801000000.000000+000".into());
        let mut recent = old.clone();
        recent.installed_on = None;
        recent.installed_date = Some("2026-09-01".parse().unwrap());
        recent.installed_on_raw = Some("2026-09-01".into());
        let merged = dedupe([&old, &recent]);
        assert_eq!(merged, dedupe([&recent, &old]));
        assert_eq!(merged[0].installed_on, None);
        assert_eq!(merged[0].installed_on_raw, recent.installed_on_raw);
        assert_eq!(merged[0].installed_date, recent.installed_date);
    }

    #[test]
    fn cim_numeric_fields_do_not_accept_embedded_signs() {
        assert!(parse_cim_timestamp("20250312000000.+00001+480").is_none());
        assert!(parse_cim_timestamp("20250312000000.000000+-48").is_none());
    }
}
