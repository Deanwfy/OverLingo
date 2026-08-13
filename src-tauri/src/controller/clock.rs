use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct SessionClock {
    elapsed: Duration,
    running_since: Option<Instant>,
}

impl SessionClock {
    pub(super) fn start(&mut self) {
        self.start_at(Instant::now());
    }

    pub(super) fn pause(&mut self) {
        self.pause_at(Instant::now());
    }

    pub(super) fn resume(&mut self) {
        self.resume_at(Instant::now());
    }

    pub(super) fn reset(&mut self) {
        self.elapsed = Duration::ZERO;
        self.running_since = None;
    }

    pub(super) fn elapsed(&self) -> Duration {
        self.elapsed_at(Instant::now())
    }

    fn start_at(&mut self, now: Instant) {
        self.elapsed = Duration::ZERO;
        self.running_since = Some(now);
    }

    fn pause_at(&mut self, now: Instant) {
        if let Some(started) = self.running_since.take() {
            self.elapsed += now.saturating_duration_since(started);
        }
    }

    fn resume_at(&mut self, now: Instant) {
        if self.running_since.is_none() {
            self.running_since = Some(now);
        }
    }

    fn elapsed_at(&self, now: Instant) -> Duration {
        self.elapsed
            + self.running_since.map_or(Duration::ZERO, |started| {
                now.saturating_duration_since(started)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_paused_time() {
        let start = Instant::now();
        let mut clock = SessionClock::default();
        clock.start_at(start);
        clock.pause_at(start + Duration::from_secs(8));

        assert_eq!(
            clock.elapsed_at(start + Duration::from_secs(30)).as_secs(),
            8
        );

        clock.resume_at(start + Duration::from_secs(30));
        assert_eq!(
            clock.elapsed_at(start + Duration::from_secs(35)).as_secs(),
            13
        );
    }
}
