//! In-app updates: the check, the throttle around it, and the one state the settings
//! footer and the tray badge both read. Kept clear of the controller, which owns
//! translation and has nothing to say about releases.

use crate::persistence::{read_config, write_config};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

const PREFERENCES_FILE: &str = "updates.json";

/// A result stands this long before another round trip is worth it.
const CHECK_INTERVAL_SECONDS: i64 = 24 * 60 * 60;
/// Long enough after launch for the windows, audio devices and provider sockets to settle
/// before the check competes with them for the network.
const STARTUP_DELAY: Duration = Duration::from_secs(5);
const STATUS_EVENT: &str = "updates://status";
/// The only surface that shows any of this; the subtitle overlay is a hot path during
/// translation and has no business decoding update progress.
const SETTINGS_WINDOW: &str = "main";
/// The tray asks the settings window to show what it just announced.
const FOCUS_EVENT: &str = "updates://focus";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    /// Nothing known yet: no check has finished this run.
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    /// Downloaded and verified; the swap happens on restart.
    Ready,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub stage: Stage,
    pub current_version: String,
    pub version: Option<String>,
    /// Percent, while downloading and only once the response declared a length.
    pub progress: Option<u8>,
    pub error: Option<String>,
    pub auto_check: bool,
    /// False when the running copy came from a package the updater cannot replace in
    /// place, which leaves the download page as the only route.
    pub installable: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Preferences {
    auto_check: bool,
    /// Unix seconds, persisted so a relaunch does not start the interval over.
    last_checked_at: Option<i64>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            auto_check: true,
            last_checked_at: None,
        }
    }
}

#[derive(Default)]
struct Inner {
    stage: Stage,
    progress: Option<u8>,
    error: Option<String>,
    /// What the last check found; its version is what the footer announces.
    pending: Option<Update>,
    /// The verified payload, held between the download and the restart that applies it.
    payload: Option<Vec<u8>>,
    preferences: Preferences,
}

/// Work in flight, which nothing may restart.
fn busy(stage: Stage) -> bool {
    matches!(stage, Stage::Checking | Stage::Downloading)
}

/// An update in hand: what the badge shows, and what a background check leaves alone.
fn found(stage: Stage) -> bool {
    matches!(stage, Stage::Available | Stage::Downloading | Stage::Ready)
}

pub struct UpdateState {
    inner: Mutex<Inner>,
}

impl UpdateState {
    fn new(preferences: Preferences) -> Self {
        Self {
            inner: Mutex::new(Inner {
                preferences,
                ..Inner::default()
            }),
        }
    }
}

/// Managed state, the preferences behind it, and the check that runs a moment after launch.
pub fn install(app: &AppHandle) {
    app.manage(UpdateState::new(read_config(app, PREFERENCES_FILE)));
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        check(app, false).await;
    });
}

/// The tray's entry: the page that shows the answer, and the check the label promises.
pub fn open_from_tray(app: &AppHandle) {
    let _ = crate::shell::show_settings(app);
    let _ = app.emit_to(SETTINGS_WINDOW, FOCUS_EVENT, ());
    let app = app.clone();
    tauri::async_runtime::spawn(async move { check(app, true).await });
}

#[tauri::command]
pub fn update_status(app: AppHandle) -> UpdateStatus {
    status(&app)
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle) {
    check(app, true).await;
}

#[tauri::command]
pub fn set_auto_check_updates(app: AppHandle, enabled: bool) -> Result<(), String> {
    let preferences = {
        let mut inner = lock(&app)?;
        inner.preferences.auto_check = enabled;
        inner.preferences.clone()
    };
    write_config(&app, PREFERENCES_FILE, &preferences)?;
    publish(&app);
    Ok(())
}

#[tauri::command]
pub async fn download_update(app: AppHandle) -> Result<(), String> {
    let update = {
        let mut inner = lock(&app)?;
        let Some(update) = inner
            .pending
            .clone()
            .filter(|_| inner.stage == Stage::Available)
        else {
            return Ok(());
        };
        inner.stage = Stage::Downloading;
        inner.progress = None;
        inner.error = None;
        update
    };
    publish(&app);

    // Chunks arrive far faster than a person can read them, so the state is republished
    // only when the rounded percentage actually moves.
    let mut received = 0u64;
    let mut announced = None;
    let downloaded = update
        .download(
            |chunk, total| {
                received += chunk as u64;
                let Some(total) = total.filter(|total| *total > 0) else {
                    return;
                };
                let percent = ((received * 100) / total).min(100) as u8;
                if announced == Some(percent) {
                    return;
                }
                announced = Some(percent);
                if let Ok(mut inner) = lock(&app) {
                    inner.progress = Some(percent);
                }
                publish(&app);
            },
            || {},
        )
        .await;

    let mut inner = lock(&app)?;
    inner.progress = None;
    match downloaded {
        Ok(bytes) => {
            inner.payload = Some(bytes);
            inner.stage = Stage::Ready;
        }
        Err(error) => {
            inner.stage = Stage::Failed;
            inner.error = Some(error.to_string());
        }
    }
    drop(inner);
    publish(&app);
    Ok(())
}

/// Swaps the app in, then relaunches through the controller so the session is stopped
/// and the config saved as on quit. On Windows the installer ends this process itself.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    // Taken, not cloned: tens of megabytes, and the process is about to be replaced.
    let taken = {
        let mut inner = lock(&app)?;
        match inner.stage {
            Stage::Ready => inner.pending.clone().zip(inner.payload.take()),
            _ => None,
        }
    };
    let Some((update, payload)) = taken else {
        return Ok(());
    };

    // Unpacking the archive takes seconds; not on a worker the provider streams share.
    let (installed, payload) =
        tauri::async_runtime::spawn_blocking(move || (update.install(&payload), payload))
            .await
            .map_err(|error| error.to_string())?;
    if let Err(error) = installed {
        let mut inner = lock(&app)?;
        // Still ready: the payload is verified and back in hand, so a retry is one click.
        inner.payload = Some(payload);
        inner.error = Some(error.to_string());
        drop(inner);
        publish(&app);
        return Err(error.to_string());
    }
    app.state::<crate::controller::AppController>()
        .request(crate::controller::ControllerRequest::Restart)
}

async fn check(app: AppHandle, forced: bool) {
    {
        let Ok(mut inner) = lock(&app) else {
            return;
        };
        if !may_check(inner.stage, &inner.preferences, forced) {
            return;
        }
        inner.stage = Stage::Checking;
        inner.error = None;
    }
    publish(&app);

    let outcome = match app.updater() {
        Ok(updater) => updater.check().await.map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    };

    // The guard must be gone before `publish`, which takes the lock again.
    let Ok(mut inner) = lock(&app) else {
        return;
    };
    let held = inner.pending.as_ref().map(|update| update.version.clone());
    let completed = settle(
        &mut inner,
        held.as_deref(),
        outcome
            .as_ref()
            .map(|latest| latest.as_ref().map(|update| update.version.as_str())),
        forced,
    );
    if let Ok(latest) = outcome {
        inner.pending = latest;
    }
    let preferences = completed.then(|| inner.preferences.clone());
    drop(inner);
    if let Some(preferences) = preferences {
        let _ = write_config(&app, PREFERENCES_FILE, &preferences);
    }
    publish(&app);
}

/// What a check's result does to the state, given the version of the update already held.
/// Says whether the round trip completed, which is what restarts the interval.
fn settle(
    inner: &mut Inner,
    held: Option<&str>,
    outcome: Result<Option<&str>, &String>,
    forced: bool,
) -> bool {
    match outcome {
        Ok(latest) => {
            // A download in hand stays good while its release is still the one on offer.
            if latest != held {
                inner.payload = None;
            }
            inner.stage = match (latest, &inner.payload) {
                (None, _) => Stage::UpToDate,
                (Some(_), Some(_)) => Stage::Ready,
                (Some(_), None) => Stage::Available,
            };
            inner.preferences.last_checked_at = Some(now_seconds());
        }
        Err(error) => {
            inner.stage = Stage::Failed;
            inner.error = Some(error.clone());
        }
    }
    // "Up to date" and "failed" are answers; a background check with nothing new says
    // nothing, and a failed one just tries again next time.
    if !forced && matches!(inner.stage, Stage::UpToDate | Stage::Failed) {
        inner.stage = Stage::Idle;
        inner.error = None;
    }
    outcome.is_ok()
}

/// Forced skips the switch and the interval; in-flight work and an update in hand are
/// left alone either way.
fn may_check(stage: Stage, preferences: &Preferences, forced: bool) -> bool {
    !busy(stage)
        && (forced || (preferences.auto_check && !found(stage) && due(preferences.last_checked_at)))
}

fn status(app: &AppHandle) -> UpdateStatus {
    // A poisoned lock leaves the footer on a plain version line rather than no line.
    let guard = lock(app).ok();
    let fallback = Inner::default();
    let inner = guard.as_deref().unwrap_or(&fallback);
    UpdateStatus {
        stage: inner.stage,
        current_version: app.package_info().version.to_string(),
        version: inner.pending.as_ref().map(|update| update.version.clone()),
        progress: inner.progress,
        error: inner.error.clone(),
        auto_check: inner.preferences.auto_check,
        installable: installable(),
    }
}

fn publish(app: &AppHandle) {
    let status = status(app);
    crate::shell::set_update_badge(app, found(status.stage));
    let _ = app.emit_to(SETTINGS_WINDOW, STATUS_EVENT, status);
}

fn lock(app: &AppHandle) -> Result<std::sync::MutexGuard<'_, Inner>, String> {
    app.state::<UpdateState>()
        .inner()
        .inner
        .lock()
        .map_err(|error| error.to_string())
}

fn due(last_checked_at: Option<i64>) -> bool {
    // A clock that moved backwards would otherwise park the check until it catches up.
    last_checked_at
        .is_none_or(|last| !(0..CHECK_INTERVAL_SECONDS).contains(&(now_seconds() - last)))
}

fn now_seconds() -> i64 {
    chrono::Utc::now().timestamp()
}

/// NSIS leaves its uninstaller beside the binary; an MSI install has none, and the setup
/// the updater fetches would land beside it rather than replace it. Anything unreadable
/// counts as installable: a wrong "no" would strand a user who could have updated.
#[cfg(target_os = "windows")]
fn installable() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("uninstall.exe").exists()))
        .unwrap_or(true)
}

#[cfg(not(target_os = "windows"))]
fn installable() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_install_checks_and_then_waits_out_the_interval() {
        assert!(due(None));
        assert!(!due(Some(now_seconds())));
        assert!(due(Some(now_seconds() - CHECK_INTERVAL_SECONDS)));
    }

    /// A clock set forward and back again must not park the check until it catches up.
    #[test]
    fn a_timestamp_in_the_future_does_not_block_the_next_check() {
        assert!(due(Some(now_seconds() + CHECK_INTERVAL_SECONDS)));
    }

    /// The startup check can fail for the most ordinary reasons — no network yet, no
    /// manifest published yet — and the next automatic one has to be allowed to try again.
    #[test]
    fn a_failed_check_does_not_block_the_next_automatic_one() {
        let preferences = Preferences::default();
        assert!(may_check(Stage::Failed, &preferences, false));
        assert!(may_check(Stage::Idle, &preferences, false));
        assert!(may_check(Stage::UpToDate, &preferences, false));
    }

    #[test]
    fn a_found_update_and_work_in_flight_are_left_alone() {
        let preferences = Preferences::default();
        assert!(!found(Stage::Idle) && !found(Stage::UpToDate) && !found(Stage::Failed));
        for stage in [Stage::Available, Stage::Ready] {
            assert!(found(stage), "{stage:?}");
            assert!(!may_check(stage, &preferences, false), "{stage:?}");
            assert!(may_check(stage, &preferences, true), "{stage:?}");
        }
        for stage in [Stage::Checking, Stage::Downloading] {
            assert!(!may_check(stage, &preferences, false), "{stage:?}");
            assert!(!may_check(stage, &preferences, true), "{stage:?}");
        }
    }

    #[test]
    fn the_switch_and_the_interval_only_gate_the_automatic_path() {
        let off = Preferences {
            auto_check: false,
            last_checked_at: None,
        };
        assert!(!may_check(Stage::Idle, &off, false));
        assert!(may_check(Stage::Idle, &off, true));

        let recent = Preferences {
            auto_check: true,
            last_checked_at: Some(now_seconds()),
        };
        assert!(!may_check(Stage::Idle, &recent, false));
        assert!(may_check(Stage::Idle, &recent, true));
    }

    fn holding(stage: Stage, payload: bool) -> Inner {
        Inner {
            stage,
            payload: payload.then(Vec::new),
            ..Inner::default()
        }
    }

    /// Only a check the user asked for gets an answer; a background one stays quiet, and
    /// a background failure does not even count as a round trip.
    #[test]
    fn a_background_check_only_speaks_when_it_finds_something() {
        let mut inner = holding(Stage::Checking, false);
        assert!(settle(&mut inner, None, Ok(None), false));
        assert_eq!(inner.stage, Stage::Idle);

        let mut inner = holding(Stage::Checking, false);
        assert!(!settle(
            &mut inner,
            None,
            Err(&"offline".to_string()),
            false
        ));
        assert_eq!((inner.stage, inner.error), (Stage::Idle, None));

        let mut inner = holding(Stage::Checking, false);
        assert!(settle(&mut inner, None, Ok(Some("1.2.0")), false));
        assert_eq!(inner.stage, Stage::Available);
    }

    #[test]
    fn a_forced_check_answers_either_way() {
        let mut inner = holding(Stage::Checking, false);
        settle(&mut inner, None, Ok(None), true);
        assert_eq!(inner.stage, Stage::UpToDate);

        let mut inner = holding(Stage::Checking, false);
        settle(&mut inner, None, Err(&"offline".to_string()), true);
        assert_eq!(inner.stage, Stage::Failed);
        assert_eq!(inner.error.as_deref(), Some("offline"));
    }

    /// A finished download survives a re-check of the same release and nothing else.
    #[test]
    fn a_download_in_hand_is_kept_only_for_the_same_release() {
        let mut inner = holding(Stage::Checking, true);
        settle(&mut inner, Some("1.2.0"), Ok(Some("1.2.0")), true);
        assert_eq!(inner.stage, Stage::Ready);
        assert!(inner.payload.is_some());

        let mut inner = holding(Stage::Checking, true);
        settle(&mut inner, Some("1.2.0"), Ok(Some("1.3.0")), true);
        assert_eq!(inner.stage, Stage::Available);
        assert!(inner.payload.is_none());

        let mut inner = holding(Stage::Checking, true);
        settle(&mut inner, Some("1.2.0"), Ok(None), true);
        assert_eq!(inner.stage, Stage::UpToDate);
        assert!(inner.payload.is_none());
    }

    #[test]
    fn preferences_default_to_checking_and_survive_a_round_trip() {
        let preferences = Preferences::default();
        assert!(preferences.auto_check);
        assert_eq!(preferences.last_checked_at, None);

        let stored: Preferences =
            serde_json::from_str(&serde_json::to_string(&preferences).unwrap()).unwrap();
        assert!(stored.auto_check);

        // A file written before a field existed still loads.
        let partial: Preferences = serde_json::from_str("{\"autoCheck\":false}").unwrap();
        assert!(!partial.auto_check);
        assert_eq!(partial.last_checked_at, None);
    }
}
