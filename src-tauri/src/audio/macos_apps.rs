//! The running applications a user could plausibly want the audio of, from NSWorkspace.

use super::CapturableApplication;
use objc2_app_kit::{NSApplicationActivationPolicy, NSRunningApplication, NSWorkspace};

pub fn running() -> Vec<CapturableApplication> {
    let current_process = std::process::id() as i32;
    let mut applications = each(|application| {
        let bundle_id = application.bundleIdentifier()?.to_string();
        let name = application.localizedName()?.to_string();
        // Accessory and background processes have no window the user could point at.
        let listable = application.activationPolicy() == NSApplicationActivationPolicy::Regular
            && !bundle_id.is_empty()
            && !name.is_empty()
            && application.processIdentifier() != current_process;
        listable.then_some(CapturableApplication { bundle_id, name })
    });
    applications.sort_by_key(|application| application.name.to_lowercase());
    applications.dedup_by(|left, right| left.bundle_id == right.bundle_id);
    applications
}

/// Every process of one application: a browser answers as one bundle id but many processes,
/// and the audio can come out of any of them.
pub fn pids_for_bundle(bundle_id: &str) -> Vec<i32> {
    each(|application| {
        (application.bundleIdentifier()?.to_string() == bundle_id)
            .then(|| application.processIdentifier())
    })
}

fn each<T>(mut pick: impl FnMut(&NSRunningApplication) -> Option<T>) -> Vec<T> {
    NSWorkspace::sharedWorkspace()
        .runningApplications()
        .iter()
        .filter_map(|application| pick(&application))
        .collect()
}
