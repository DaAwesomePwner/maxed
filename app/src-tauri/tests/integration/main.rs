// Explicit target: keep fixture paths beside their original test modules.
#[path = "../../../../crates/maxed-assets/tests/common/mod.rs"]
mod fixture;

#[path = "../alerts.rs"]
mod alerts;
#[path = "../alerts_acceptance.rs"]
mod alerts_acceptance;
#[path = "../assets_bridge.rs"]
mod assets_bridge;
#[path = "../bridge.rs"]
mod bridge;
#[path = "../demo.rs"]
mod demo;
#[path = "../integration.rs"]
mod integration;
#[path = "../p7_owners.rs"]
mod p7_owners;
#[path = "../recording.rs"]
mod recording;
#[path = "../settings.rs"]
mod settings;
#[path = "../setup.rs"]
mod setup;
#[path = "../status.rs"]
mod status;
#[path = "../updater_package.rs"]
mod updater_package;
#[path = "../window_policy.rs"]
mod window_policy;
