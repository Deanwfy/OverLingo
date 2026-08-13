use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum TranslationState {
    Stopped,
    Starting,
    Running,
    Paused,
    Failed,
}

impl TranslationState {
    /// A session the user considers open, whether or not audio is flowing yet.
    pub(super) fn is_active(self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::Paused)
    }

    /// Whether audio is being translated right now. Narrower than [`is_active`]: a paused
    /// session is still a session, but nothing is being captured or billed.
    pub(super) fn is_translating(self) -> bool {
        matches!(self, Self::Running)
    }

    /// Whether the elapsed-time readout still has to be refreshed. Deliberately wider than
    /// [`is_active`]: a session whose routes have all failed can still come back, and a
    /// clock that stopped being republished never starts again on its own.
    pub(super) fn is_timed(self) -> bool {
        !matches!(self, Self::Stopped | Self::Paused)
    }

    /// Time with every route down is not translation time, but the session is not over
    /// either, so the clock is held rather than cleared and a recovery carries on from
    /// where it stopped.
    pub(super) fn clock_change(self, next: Self) -> ClockChange {
        match (self, next) {
            (from, to) if from == to => ClockChange::None,
            (_, Self::Failed) => ClockChange::Hold,
            (Self::Failed, to) if to.is_timed() => ClockChange::Continue,
            _ => ClockChange::None,
        }
    }

    /// Folds the open routes into the session state. Pausing is the user's own decision, so
    /// no route may override it, and a session with no routes yet keeps the state it has.
    pub(super) fn aggregated(self, routes: &[RouteState]) -> Self {
        if self == Self::Paused || routes.is_empty() {
            return self;
        }
        let states = || routes.iter().copied();
        if states().any(|state| matches!(state, RouteState::Live | RouteState::Reconfiguring)) {
            Self::Running
        } else if states()
            .any(|state| matches!(state, RouteState::Connecting | RouteState::Reconnecting))
        {
            Self::Starting
        } else if states().all(|state| state == RouteState::Failed) {
            Self::Failed
        } else {
            self
        }
    }
}

/// What a session-state transition asks of the elapsed-time clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClockChange {
    None,
    Hold,
    Continue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum RouteState {
    Stopped,
    Connecting,
    /// Rebuilding after a settings change while the previous session still runs.
    Reconfiguring,
    Reconnecting,
    Live,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_working_route_carries_the_session() {
        assert_eq!(
            TranslationState::Starting.aggregated(&[RouteState::Live, RouteState::Failed]),
            TranslationState::Running
        );
        assert_eq!(
            TranslationState::Running.aggregated(&[RouteState::Reconnecting, RouteState::Failed]),
            TranslationState::Starting
        );
    }

    #[test]
    fn only_an_entirely_failed_set_fails_the_session() {
        assert_eq!(
            TranslationState::Running.aggregated(&[RouteState::Failed, RouteState::Failed]),
            TranslationState::Failed
        );
        assert_eq!(
            TranslationState::Running.aggregated(&[RouteState::Failed, RouteState::Stopped]),
            TranslationState::Running
        );
    }

    /// Clearing the clock on failure loses a recovering session's elapsed time, and leaves
    /// it stopped at zero because nothing ever starts it again.
    #[test]
    fn a_recovering_session_carries_on_from_where_it_stopped() {
        use TranslationState::{Failed, Running, Starting, Stopped};
        assert_eq!(Running.clock_change(Failed), ClockChange::Hold);
        assert_eq!(Failed.clock_change(Starting), ClockChange::Continue);
        assert_eq!(Failed.clock_change(Running), ClockChange::Continue);
        assert_eq!(Failed.clock_change(Failed), ClockChange::None);
        assert_eq!(Failed.clock_change(Stopped), ClockChange::None);
    }

    /// The readout is driven by a chain that reschedules itself, so a state that stops the
    /// chain strands the clock even after the routes recover.
    #[test]
    fn a_failed_session_keeps_its_clock_running() {
        assert!(TranslationState::Failed.is_timed());
        assert!(TranslationState::Starting.is_timed());
        assert!(TranslationState::Running.is_timed());
        assert!(!TranslationState::Paused.is_timed());
        assert!(!TranslationState::Stopped.is_timed());
    }

    #[test]
    fn a_paused_session_ignores_its_routes() {
        assert_eq!(
            TranslationState::Paused.aggregated(&[RouteState::Live]),
            TranslationState::Paused
        );
    }

    #[test]
    fn no_open_routes_leaves_the_state_alone() {
        assert_eq!(
            TranslationState::Starting.aggregated(&[]),
            TranslationState::Starting
        );
    }
}
