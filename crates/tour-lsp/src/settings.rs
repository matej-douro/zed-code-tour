use serde_json::Value;

/// Where the tour body is rendered relative to the selected source range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Placement {
    #[default]
    Below,
    Above,
}

impl Placement {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Below => "below",
            Self::Above => "above",
        }
    }

    fn from_json(value: &Value) -> Option<Self> {
        match value.as_str()? {
            "below" => Some(Self::Below),
            "above" => Some(Self::Above),
            _ => None,
        }
    }
}

/// Whether a tour starts automatically when its file opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AutoStart {
    Always,
    #[default]
    Prompt,
    Never,
}

impl AutoStart {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::Prompt => "prompt",
            Self::Never => "never",
        }
    }

    fn from_json(value: &Value) -> Option<Self> {
        match value.as_str()? {
            "always" => Some(Self::Always),
            "prompt" => Some(Self::Prompt),
            "never" => Some(Self::Never),
            _ => None,
        }
    }
}

/// Validated settings passed through `lsp.tour.settings` as initialization
/// options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TourSettings {
    pub placement: Placement,
    pub auto_start: AutoStart,
}

impl TourSettings {
    /// Parse settings, falling back to the documented default for every absent
    /// or invalid field.
    ///
    /// A missing or non-object options value produces the full default. Unknown
    /// fields are ignored.
    pub fn from_initialization_options(options: Option<&Value>) -> Self {
        let Some(options) = options.and_then(Value::as_object) else {
            return Self::default();
        };

        Self {
            placement: options
                .get("placement")
                .and_then(Placement::from_json)
                .unwrap_or_default(),
            auto_start: options
                .get("auto_start")
                .and_then(AutoStart::from_json)
                .unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{AutoStart, Placement, TourSettings};

    #[test]
    fn absent_options_use_defaults() {
        let settings = TourSettings::from_initialization_options(None);
        assert_eq!(settings.placement, Placement::Below);
        assert_eq!(settings.auto_start, AutoStart::Prompt);
        assert_eq!(settings, TourSettings::default());
    }

    #[test]
    fn empty_and_unrelated_objects_use_defaults() {
        for options in [
            json!({}),
            json!({ "unknown": true }),
            json!({ "placement": null }),
        ] {
            let settings = TourSettings::from_initialization_options(Some(&options));
            assert_eq!(settings, TourSettings::default(), "options: {options}");
        }
    }

    #[test]
    fn parses_valid_values() {
        for (placement, auto_start) in [
            (Placement::Below, AutoStart::Always),
            (Placement::Below, AutoStart::Prompt),
            (Placement::Below, AutoStart::Never),
            (Placement::Above, AutoStart::Always),
            (Placement::Above, AutoStart::Prompt),
            (Placement::Above, AutoStart::Never),
        ] {
            let options = json!({
                "placement": placement.as_str(),
                "auto_start": auto_start.as_str(),
            });
            let settings = TourSettings::from_initialization_options(Some(&options));
            assert_eq!(
                settings,
                TourSettings {
                    placement,
                    auto_start,
                }
            );
        }
    }

    #[test]
    fn invalid_values_fall_back_field_by_field() {
        let options = json!({
            "placement": "sideways",
            "auto_start": "sometimes",
        });
        assert_eq!(
            TourSettings::from_initialization_options(Some(&options)),
            TourSettings::default()
        );

        let options = json!({
            "placement": "above",
            "auto_start": 3,
        });
        assert_eq!(
            TourSettings::from_initialization_options(Some(&options)),
            TourSettings {
                placement: Placement::Above,
                auto_start: AutoStart::Prompt,
            }
        );

        let options = json!({
            "placement": ["below"],
            "auto_start": "never",
        });
        assert_eq!(
            TourSettings::from_initialization_options(Some(&options)),
            TourSettings {
                placement: Placement::Below,
                auto_start: AutoStart::Never,
            }
        );
    }

    #[test]
    fn non_object_options_use_defaults() {
        for options in [json!(null), json!("below"), json!([1, 2, 3]), json!(42)] {
            assert_eq!(
                TourSettings::from_initialization_options(Some(&options)),
                TourSettings::default(),
                "options: {options}"
            );
        }
    }
}
