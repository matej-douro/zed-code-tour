use std::fmt;

use lsp_types::{Range, Uri};

use crate::{
    tour::{ParseError, Tour, TourStep, parse_tour},
    uri::{file_path, file_uri},
};

/// The currently open tour and its resolved active step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveTour {
    pub tour_uri: Uri,
    pub tour: Tour,
    pub current_step: usize,
    pub source_uri: Option<Uri>,
    pub source_range: Option<Range>,
    pub running: bool,
}

impl ActiveTour {
    /// Create active state for a parsed tour. The first step is selected and
    /// the tour is not running until the start flow in #5 explicitly starts it.
    pub fn new(tour_uri: Uri, tour: Tour) -> Self {
        let mut active = Self {
            tour_uri,
            tour,
            current_step: 0,
            source_uri: None,
            source_range: None,
            running: false,
        };
        active.resolve_current_step();
        active
    }

    pub fn current_step(&self) -> Option<&TourStep> {
        self.tour.steps.get(self.current_step)
    }

    pub fn step_count(&self) -> usize {
        self.tour.steps.len()
    }

    /// Select a step by its zero-based position. Returns false when the index is
    /// out of range, leaving the current step unchanged.
    pub fn set_current_step(&mut self, index: usize) -> bool {
        if index >= self.tour.steps.len() {
            return false;
        }

        self.current_step = index;
        self.resolve_current_step();
        true
    }

    fn resolve_current_step(&mut self) {
        self.source_uri = self
            .current_step()
            .and_then(|step| file_uri(&step.target.path));
        // The LSP range depends on source text (UTF-16 columns), so it is
        // resolved by the server in navigation.rs and is never fabricated here.
        self.source_range = None;
    }
}

/// Errors returned when a tour document cannot be made active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenTourError {
    InvalidTourUri,
    Parse(Vec<ParseError>),
}

impl fmt::Display for OpenTourError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTourUri => f.write_str("tour document URI is not a local file"),
            Self::Parse(errors) => {
                for error in errors {
                    writeln!(f, "{error}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for OpenTourError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOutcome {
    /// The changed document is not the active tour.
    Ignored,
    /// The active tour was replaced with the new parsed content.
    Updated,
}

/// Holds at most one active tour. Opening a tour always replaces any previous
/// tour, so the last opened `.tour.md` document wins. There is one state per
/// language server instance, which assumes one workspace per server.
#[derive(Debug, Default)]
pub struct TourState {
    active: Option<ActiveTour>,
}

impl TourState {
    pub fn active(&self) -> Option<&ActiveTour> {
        self.active.as_ref()
    }

    pub fn active_mut(&mut self) -> Option<&mut ActiveTour> {
        self.active.as_mut()
    }

    /// Open a tour document, replacing any active tour.
    ///
    /// The previous tour is cleared before parsing, so an invalid document also
    /// deterministically leaves no active tour instead of reviving stale state.
    pub fn open_tour(&mut self, tour_uri: Uri, source: &str) -> Result<(), OpenTourError> {
        self.active = None;
        let path = file_path(&tour_uri).ok_or(OpenTourError::InvalidTourUri)?;
        let tour = parse_tour(&path, source).map_err(OpenTourError::Parse)?;
        self.active = Some(ActiveTour::new(tour_uri, tour));
        Ok(())
    }

    /// Update the active tour after a `didChange`.
    ///
    /// Changes to other open tour files are ignored: only opening a document
    /// replaces the active tour. When the active tour changes, its running flag
    /// and current step are preserved when possible, and the step index is
    /// clamped to the new step count.
    ///
    /// If the changed content cannot be parsed, the last valid active tour is
    /// kept. This lets an in-progress edit fail transiently and recover on the
    /// next valid change instead of stranding the tour until it is reopened.
    pub fn update_active_tour(
        &mut self,
        tour_uri: &Uri,
        source: &str,
    ) -> Result<UpdateOutcome, OpenTourError> {
        let Some(active) = self.active.as_ref() else {
            return Ok(UpdateOutcome::Ignored);
        };
        if active.tour_uri != *tour_uri {
            return Ok(UpdateOutcome::Ignored);
        }

        let current_step = active.current_step;
        let running = active.running;

        let path = file_path(tour_uri).ok_or(OpenTourError::InvalidTourUri)?;
        let tour = parse_tour(&path, source).map_err(OpenTourError::Parse)?;
        let mut active = ActiveTour::new(tour_uri.clone(), tour);
        active.running = running;
        if active.step_count() > 0 {
            active.set_current_step(current_step.min(active.step_count() - 1));
        }
        self.active = Some(active);
        Ok(UpdateOutcome::Updated)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use lsp_types::Uri;

    use super::{ActiveTour, OpenTourError, TourState, UpdateOutcome};

    const FIRST_TOUR: &str = r#"# First tour

## 1. One
[one](src/one.rs#L1:1-L1:4)
"#;
    const SECOND_TOUR: &str = r#"# Second tour

## 1. Two
[two](src/two.rs#L2:3-L4:5)

## 2. Three
[three](src/three.rs#L7:1-L7:9)
"#;
    const INVALID_TOUR: &str = "# Invalid tour\n\n## 1. Missing link\n\nNo target link.\n";

    fn uri(value: &str) -> Uri {
        Uri::from_str(value).expect("valid URI")
    }

    fn file_uri(value: &str) -> Uri {
        uri(value)
    }

    #[test]
    fn opening_a_second_tour_replaces_the_first() {
        let mut state = TourState::default();
        state
            .open_tour(file_uri("file:///tours/first.tour.md"), FIRST_TOUR)
            .expect("first tour should open");

        let active = state.active().expect("first tour is active");
        assert_eq!(active.tour_uri.as_str(), "file:///tours/first.tour.md");
        assert_eq!(active.current_step, 0);
        assert_eq!(
            active.source_uri.as_ref().map(|uri| uri.as_str()),
            Some("file:///tours/src/one.rs")
        );
        assert_eq!(active.source_range, None);
        assert!(!active.running);

        state
            .open_tour(file_uri("file:///tours/second.tour.md"), SECOND_TOUR)
            .expect("second tour should open");

        let active = state.active().expect("second tour is active");
        assert_eq!(active.tour_uri.as_str(), "file:///tours/second.tour.md");
        assert_eq!(active.tour.title, "Second tour");
        assert_eq!(active.tour.steps.len(), 2);
        assert_eq!(active.current_step, 0);
        assert_eq!(
            active.source_uri.as_ref().map(|uri| uri.as_str()),
            Some("file:///tours/src/two.rs")
        );
        assert_eq!(active.source_range, None);
        assert!(!active.running);
    }

    #[test]
    fn invalid_tour_clears_the_active_tour() {
        let mut state = TourState::default();
        state
            .open_tour(file_uri("file:///tours/first.tour.md"), FIRST_TOUR)
            .expect("first tour should open");

        let error = state
            .open_tour(file_uri("file:///tours/invalid.tour.md"), INVALID_TOUR)
            .expect_err("invalid tour should fail");

        assert!(matches!(error, OpenTourError::Parse(_)));
        assert!(state.active().is_none());
    }

    #[test]
    fn changes_to_other_tours_are_ignored() {
        let mut state = TourState::default();
        state
            .open_tour(file_uri("file:///tours/first.tour.md"), FIRST_TOUR)
            .expect("first tour should open");

        let outcome = state
            .update_active_tour(&file_uri("file:///tours/second.tour.md"), SECOND_TOUR)
            .expect("inactive change should not fail");

        assert_eq!(outcome, UpdateOutcome::Ignored);
        assert_eq!(
            state.active().map(|active| active.tour_uri.as_str()),
            Some("file:///tours/first.tour.md")
        );
    }

    #[test]
    fn invalid_active_change_keeps_the_last_valid_tour() {
        let mut state = TourState::default();
        state
            .open_tour(file_uri("file:///tours/first.tour.md"), FIRST_TOUR)
            .expect("first tour should open");

        let error = state
            .update_active_tour(&file_uri("file:///tours/first.tour.md"), INVALID_TOUR)
            .expect_err("invalid change should fail");

        assert!(matches!(error, OpenTourError::Parse(_)));
        let active = state.active().expect("last valid tour should be kept");
        assert_eq!(active.tour.title, "First tour");
        assert_eq!(active.current_step, 0);

        let outcome = state
            .update_active_tour(&file_uri("file:///tours/first.tour.md"), SECOND_TOUR)
            .expect("the next valid change should recover");
        assert_eq!(outcome, UpdateOutcome::Updated);
        assert_eq!(
            state.active().map(|active| active.tour.title.as_str()),
            Some("Second tour")
        );
    }

    #[test]
    fn changing_the_active_tour_preserves_step_and_running_state() {
        let mut state = TourState::default();
        state
            .open_tour(file_uri("file:///tours/second.tour.md"), SECOND_TOUR)
            .expect("second tour should open");

        let active = state.active_mut().expect("second tour is active");
        assert!(active.set_current_step(1));
        active.running = true;

        let outcome = state
            .update_active_tour(&file_uri("file:///tours/second.tour.md"), SECOND_TOUR)
            .expect("active change should parse");

        assert_eq!(outcome, UpdateOutcome::Updated);
        let active = state.active().expect("tour is still active");
        assert_eq!(active.current_step, 1);
        assert!(active.running);
        assert_eq!(
            active.source_uri.as_ref().map(|uri| uri.as_str()),
            Some("file:///tours/src/three.rs")
        );
        assert_eq!(active.source_range, None);
    }

    #[test]
    fn changing_the_active_tour_clamps_the_step_index() {
        let mut state = TourState::default();
        state
            .open_tour(file_uri("file:///tours/second.tour.md"), SECOND_TOUR)
            .expect("second tour should open");
        state
            .active_mut()
            .expect("second tour is active")
            .set_current_step(1);

        let shorter_tour = r#"# Shorter

## 1. Only step
[only](src/only.rs#L1:1-L1:2)
"#;
        state
            .update_active_tour(&file_uri("file:///tours/second.tour.md"), shorter_tour)
            .expect("active change should parse");

        assert_eq!(state.active().map(|active| active.current_step), Some(0));
    }

    #[test]
    fn empty_tours_have_no_resolved_source() {
        let active = ActiveTour::new(
            file_uri("file:///tours/empty.tour.md"),
            crate::tour::parse_tour("/tours/empty.tour.md", "# Empty\n")
                .expect("empty tour should parse"),
        );

        assert_eq!(active.step_count(), 0);
        assert_eq!(active.source_uri, None);
        assert_eq!(active.source_range, None);
    }
}
