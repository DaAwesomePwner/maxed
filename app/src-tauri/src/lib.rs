#![forbid(unsafe_code)]

pub mod assets_bridge;
pub mod bridge;
pub mod demo;
pub mod interactive_rail;
pub mod recording;
pub mod runtime;
pub mod settings;
pub mod setup;
mod startup_cache;
pub mod updater;
pub mod window_policy;

use maxed_types::{CaptureStatus, LiveSnapshot, StoreStatus};
use runtime::CaptureRuntime;
use setup::SetupService;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::Manager;
use updater::service::UpdateService;

#[derive(Default)]
struct ExitControl {
    exiting: AtomicBool,
    finished: AtomicBool,
    warned: AtomicBool,
    startup_failed: AtomicBool,
}

impl ExitControl {
    fn exit_code(&self) -> i32 {
        i32::from(self.startup_failed.load(Ordering::Acquire))
    }

    fn discard_confirmed(&self, unsaved: bool) -> bool {
        unsaved && self.warned.load(Ordering::Acquire)
    }
}

#[tauri::command]
async fn exit_without_saving(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<(), String> {
    let runtime = runtime.inner().clone();
    runtime.wait_history_preparation().await;
    let setup = app.state::<Arc<SetupService>>().inner().clone();
    if setup.updates.installing() {
        return Err("Update operation is busy.".into());
    }
    let exit = app.state::<Arc<ExitControl>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        setup.updates.shutdown()?;
        setup.cancel_pending(true);
        setup.join_pending();
        runtime.exit_without_saving();
        let _ = runtime.flush_bridge();
        exit.finished.store(true, Ordering::Release);
        app.exit(exit.exit_code());
        Ok(())
    })
    .await
    .map_err(|_| "Exit worker failed".to_string())?
}

#[tauri::command]
async fn get_update_status(
    service: tauri::State<'_, Arc<UpdateService>>,
) -> Result<maxed_types::UpdateStatus, String> {
    Ok(service.status())
}
#[tauri::command]
async fn check_for_updates(
    service: tauri::State<'_, Arc<UpdateService>>,
) -> Result<maxed_types::UpdateStatus, String> {
    Ok(service.check())
}
#[tauri::command]
async fn download_update(
    request_id: String,
    service: tauri::State<'_, Arc<UpdateService>>,
) -> Result<maxed_types::UpdateStatus, String> {
    service.download(&request_id)
}
#[tauri::command]
async fn install_update(
    request_id: String,
    service: tauri::State<'_, Arc<UpdateService>>,
) -> Result<maxed_types::UpdateStatus, String> {
    service.install(&request_id)
}
#[tauri::command]
async fn cancel_update(
    service: tauri::State<'_, Arc<UpdateService>>,
) -> Result<maxed_types::UpdateStatus, String> {
    Ok(service.cancel())
}
#[tauri::command]
async fn get_recording_status(
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<maxed_types::RecordingStatus, String> {
    let setup = setup.inner().clone();
    tauri::async_runtime::spawn_blocking(move || setup.recording_status())
        .await
        .map_err(|_| "Recording status unavailable.".to_string())?
}
#[tauri::command]
async fn report_interactive_rail(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<(), String> {
    app.state::<interactive_rail::InteractiveRail>()
        .report(window.label() == "main", std::io::stdout().lock())
}

#[tauri::command]
fn get_capture_status(runtime: tauri::State<'_, Arc<CaptureRuntime>>) -> CaptureStatus {
    runtime.status()
}

#[tauri::command]
fn get_live_snapshot(runtime: tauri::State<'_, Arc<CaptureRuntime>>) -> LiveSnapshot {
    runtime.snapshot()
}

#[tauri::command]
fn subscribe_live_payload(
    webview: tauri::Webview,
    payloads: tauri::State<'_, bridge::channels::LivePayloads>,
    event: String,
    channel: tauri::ipc::Channel,
) -> Result<u32, String> {
    payloads.subscribe(webview.label(), event, channel)
}

#[tauri::command]
fn unsubscribe_live_payload(
    webview: tauri::Webview,
    payloads: tauri::State<'_, bridge::channels::LivePayloads>,
    id: u32,
) -> Result<(), String> {
    payloads.unsubscribe(webview.label(), id)
}

#[tauri::command]
fn get_store_status(runtime: tauri::State<'_, Arc<CaptureRuntime>>) -> StoreStatus {
    runtime.store_status()
}

#[tauri::command]
async fn get_home_summary(
    scope: maxed_types::HomeScope,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::HomeSummary>, String> {
    runtime.inner().home_summary(scope).await
}

#[tauri::command]
async fn get_scope_loot(
    scope: maxed_types::HomeScope,
    cursor: Option<String>,
    limit: u16,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<maxed_types::HistoryReply<maxed_types::ScopeLootPage>, String> {
    let tiers = setup.scope_loot_tiers();
    runtime
        .inner()
        .scope_loot(scope, cursor, limit, tiers)
        .await
}

#[tauri::command]
async fn get_dungeon_bests(
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<Vec<maxed_types::DungeonBest>>, String> {
    runtime.inner().dungeon_bests().await
}

#[tauri::command]
async fn get_alert_summary(
    request: maxed_types::AlertSummaryRequest,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::AlertSummary>, String> {
    runtime.inner().alert_summary(request).await
}
#[tauri::command]
async fn list_alerts(
    filter: maxed_types::AlertFilter,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::AlertPage>, String> {
    runtime.inner().list_alerts(filter).await
}
#[tauri::command]
async fn get_alert_delivery_status(
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::AlertDeliveryStatus, String> {
    Ok(runtime.alert_delivery_status())
}
#[tauri::command]
async fn update_chat_settings(
    settings: maxed_types::ChatSettings,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<(), String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.update_chat_settings(settings))
        .await
        .map_err(|_| "Chat settings worker failed".to_string())?
}
#[tauri::command]
async fn update_alert_preferences(
    preferences: maxed_types::AlertPreferences,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<(), String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.update_alert_preferences(preferences))
        .await
        .map_err(|_| "Alert settings worker failed".to_string())?
}
#[tauri::command]
async fn request_notification_permission(
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::NotificationPermission, String> {
    runtime.request_notification_permission()
}
#[tauri::command]
async fn test_alert_delivery(
    kind: maxed_types::AlertKind,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<maxed_types::AlertDeliveryStatus, String> {
    let runtime = runtime.inner().clone();
    let setup = setup.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cache = setup.assets.cache();
        runtime.test_alert_delivery_with_catalog(kind, cache.as_ref().map(|c| c.catalog.as_ref()))
    })
    .await
    .map_err(|_| "Test notification worker failed".to_string())?
}

#[tauri::command]
async fn list_runs(
    filter: maxed_types::RunFilter,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::RunPage>, String> {
    runtime.inner().list_runs(filter).await
}

#[tauri::command]
async fn list_drops(
    filter: maxed_types::DropFilter,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::DropPage>, String> {
    runtime.inner().list_drops(filter).await
}

#[tauri::command]
async fn get_chat_overview(
    request: maxed_types::ChatOverviewRequest,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::ChatOverview>, String> {
    runtime.inner().get_chat_overview(request).await
}

#[tauri::command]
async fn get_chat_sender(
    request: maxed_types::ChatSenderRequest,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<Option<maxed_types::ChatSenderSummary>>, String> {
    runtime.inner().get_chat_sender(request).await
}

#[tauri::command]
async fn list_whisper_partners(
    filter: maxed_types::WhisperPartnerFilter,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::WhisperPartners>, String> {
    runtime.inner().list_whisper_partners(filter).await
}

#[tauri::command]
async fn list_chat(
    filter: maxed_types::ChatFilter,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::ChatPage>, String> {
    runtime.inner().list_chat(filter).await
}

#[tauri::command]
async fn get_event_summary(
    runtime: tauri::State<'_, std::sync::Arc<CaptureRuntime>>,
    request: maxed_types::EventSummaryRequest,
) -> Result<maxed_types::HistoryReply<maxed_types::EventSummary>, String> {
    runtime.inner().get_event_summary(request).await
}

#[tauri::command]
async fn list_instance_events(
    filter: maxed_types::EventFilter,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::EventPage>, String> {
    runtime.inner().list_instance_events(filter).await
}

#[tauri::command]
async fn clear_chat_history(runtime: tauri::State<'_, Arc<CaptureRuntime>>) -> Result<(), String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.clear_chat_history())
        .await
        .map_err(|_| "Chat worker failed".to_string())?
}

#[tauri::command]
async fn get_item_history(
    request: maxed_types::ItemHistoryRequest,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::ItemHistory>, String> {
    runtime.inner().get_item_history(request).await
}

#[tauri::command]
async fn get_stats_maps(
    filter: maxed_types::StatsFilter,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::StatsMaps>, String> {
    runtime.inner().stats_maps(filter).await
}

#[tauri::command]
async fn list_stats_sessions(
    filter: maxed_types::StatsSessionFilter,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::StatsSessionPage>, String> {
    runtime.inner().stats_sessions(filter).await
}

#[tauri::command]
async fn get_stats_session(
    request: maxed_types::StatsSessionRequest,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<Option<maxed_types::StatsSessionDetail>>, String> {
    runtime.inner().stats_session(request).await
}

#[tauri::command]
async fn get_stats_dungeons(
    filter: maxed_types::StatsFilter,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::StatsDungeons>, String> {
    runtime.inner().stats_dungeons(filter).await
}

#[tauri::command]
async fn export_stats_csv(
    app: tauri::AppHandle,
    request: maxed_types::StatsExportRequest,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<maxed_types::StatsExportResult, String> {
    let setup = setup.inner().clone();
    let dialog = runtime::TauriSaveDialog(app);
    runtime
        .inner()
        .export_stats_csv(&dialog, request, move |ids| {
            setup
                .visuals(ids)
                .map(|page| {
                    page.objects
                        .into_iter()
                        .filter_map(|o| o.name.map(|name| (o.object_type, name)))
                        .collect()
                })
                .unwrap_or_default()
        })
        .await
}

#[tauri::command]
async fn get_loot_stats(
    filter: maxed_types::LootStatsFilter,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::LootStats>, String> {
    runtime.inner().get_loot_stats(filter).await
}

#[tauri::command]
async fn get_drop_summary(
    request: maxed_types::DropSummaryRequest,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<maxed_types::HistoryReply<maxed_types::DropSummary>, String> {
    let tiers = setup.scope_loot_tiers();
    runtime.inner().get_drop_summary(request, tiers).await
}

#[tauri::command]
async fn get_collection(
    request: maxed_types::CollectionRequest,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<maxed_types::HistoryReply<maxed_types::Collection>, String> {
    let tiers = setup.scope_loot_tiers();
    runtime.inner().get_collection(request, tiers).await
}

#[tauri::command]
async fn get_run(
    id: String,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<Option<maxed_types::RunStory>>, String> {
    runtime.inner().get_run(id).await
}

#[tauri::command]
async fn get_run_fight_strip(
    run_id: String,
    player_id: Option<String>,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<Vec<maxed_types::RunFightStripRow>>, String> {
    runtime.inner().get_run_fight_strip(run_id, player_id).await
}

#[tauri::command]
async fn get_fight(
    id: String,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<Option<maxed_types::FightView>>, String> {
    runtime.inner().get_fight(id).await
}

#[tauri::command]
async fn get_player_breakdown(
    fight_id: String,
    object_id: i32,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<Option<maxed_types::PlayerDamage>>, String> {
    runtime
        .inner()
        .get_player_breakdown(fight_id, object_id)
        .await
}

#[tauri::command]
async fn retry_store(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<(), String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        runtime.retry_store()?;
        clear_exit_warning(&app);
        Ok(())
    })
    .await
    .map_err(|_| "Summary worker failed".to_string())?
}

#[tauri::command]
async fn set_demo(
    app: tauri::AppHandle,
    enabled: bool,
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<(), String> {
    app.state::<Arc<CaptureRuntime>>()
        .wait_history_preparation()
        .await;
    let setup = setup.inner().clone();
    setup.cancel_pending(false);
    tauri::async_runtime::spawn_blocking(move || {
        setup.set_demo(enabled)?;
        clear_exit_warning(&app);
        Ok(())
    })
    .await
    .map_err(|_| "Demo worker failed".to_string())?
}

#[tauri::command]
async fn start_capture(
    app: tauri::AppHandle,
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<(), String> {
    app.state::<Arc<CaptureRuntime>>()
        .wait_history_preparation()
        .await;
    let setup = setup.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        setup.start()?;
        clear_exit_warning(&app);
        Ok(())
    })
    .await
    .map_err(|_| "Capture worker failed".to_string())?
}

#[tauri::command]
async fn stop_capture(setup: tauri::State<'_, Arc<SetupService>>) -> Result<(), String> {
    let setup = setup.inner().clone();
    setup.cancel_pending(false);
    tauri::async_runtime::spawn_blocking(move || setup.stop())
        .await
        .map_err(|_| "Capture worker failed".to_string())?
}

#[tauri::command]
async fn get_startup_setup_required(
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<bool, String> {
    #[cfg(feature = "perf-live")]
    if runtime::perf::enabled() {
        return Ok(false);
    }
    let setup = setup.inner().clone();
    tauri::async_runtime::spawn_blocking(move || setup.startup_setup_required())
        .await
        .map_err(|_| "Startup checks failed. Open Setup to retry.".into())
}
#[tauri::command]
fn get_catalog_available(setup: tauri::State<'_, Arc<SetupService>>) -> bool {
    setup.catalog_available()
}
#[tauri::command]
async fn get_setup_status(app: tauri::AppHandle, retry: Option<bool>) -> maxed_types::SetupStatus {
    let setup = app.state::<Arc<SetupService>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || setup.query_with_retry(retry.unwrap_or(false)))
        .await
        .unwrap_or_else(|_| maxed_types::SetupStatus {
            npcap: maxed_types::NpcapCheck::Failed,
            assets: maxed_types::AssetCheck::Failed,
            message: "Setup worker failed. Retry setup.".into(),
            ..Default::default()
        })
}
#[tauri::command]
async fn list_adapters(
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<Vec<maxed_types::AdapterInfo>, String> {
    let setup = setup.inner().clone();
    tauri::async_runtime::spawn_blocking(move || setup.adapters())
        .await
        .map_err(|_| "Adapter worker failed.")?
}
#[tauri::command]
async fn get_settings(
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<maxed_types::AppSettings, String> {
    let setup = setup.inner().clone();
    tauri::async_runtime::spawn_blocking(move || setup.settings.get())
        .await
        .map_err(|_| "Settings worker failed.")?
}
#[tauri::command]
async fn update_settings(
    settings: maxed_types::AppSettings,
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<(), String> {
    let setup = setup.inner().clone();
    tauri::async_runtime::spawn_blocking(move || setup.update(settings))
        .await
        .map_err(|_| "Settings worker failed.")?
}
#[tauri::command]
async fn search_items(
    query: String,
    limit: u8,
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<Vec<maxed_types::ItemMatch>, String> {
    let setup = setup.inner().clone();
    tauri::async_runtime::spawn_blocking(move || setup.search_items(&query, limit))
        .await
        .map_err(|_| "Asset worker failed.")?
}
#[tauri::command]
async fn set_chase_list(
    items: Vec<maxed_types::ChaseItem>,
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<(), String> {
    let setup = setup.inner().clone();
    tauri::async_runtime::spawn_blocking(move || setup.settings.put_chase(items))
        .await
        .map_err(|_| "Settings worker failed.")?
}
#[tauri::command]
async fn set_adapter(
    adapter_id: Option<String>,
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<(), String> {
    let setup = setup.inner().clone();
    tauri::async_runtime::spawn_blocking(move || setup.set_adapter(adapter_id))
        .await
        .map_err(|_| "Adapter worker failed.")?
}
#[tauri::command]
async fn set_assets_path(
    path: String,
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<(), String> {
    let setup = setup.inner().clone();
    let operation = setup.reserve_assets()?;
    tauri::async_runtime::spawn_blocking(move || setup.set_assets_reserved(path, operation))
        .await
        .map_err(|_| "Asset worker failed.")?
}
#[tauri::command]
async fn get_object_visuals(
    object_types: Vec<u16>,
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<maxed_types::VisualPage, String> {
    let setup = setup.inner().clone();
    tauri::async_runtime::spawn_blocking(move || setup.visuals(&object_types))
        .await
        .map_err(|_| "Visual worker failed.")?
}

#[tauri::command]
async fn list_catalog_dungeons(
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<Vec<String>, String> {
    let setup = setup.inner().clone();
    tauri::async_runtime::spawn_blocking(move || setup.catalog_dungeons())
        .await
        .map_err(|_| "Catalog worker failed.")?
}

#[tauri::command]
async fn list_catalog_dungeon_options(
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<Vec<maxed_types::DungeonDef>, String> {
    let setup = setup.inner().clone();
    tauri::async_runtime::spawn_blocking(move || setup.catalog_dungeon_options())
        .await
        .map_err(|_| "Catalog worker failed.")?
}

#[tauri::command]
async fn get_dungeon_portal_type(
    dungeon: String,
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<Option<u16>, String> {
    if dungeon.len() > 4096 {
        return Err("Dungeon name is too long.".into());
    }
    let setup = setup.inner().clone();
    tauri::async_runtime::spawn_blocking(move || setup.dungeon_portal_type(&dungeon))
        .await
        .map_err(|_| "Visual worker failed.")?
}

fn clear_exit_warning(app: &tauri::AppHandle) {
    app.state::<Arc<ExitControl>>()
        .warned
        .store(false, Ordering::Release);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_title("Maxed");
    }
}

// Runs only on the startup blocking worker, never the UI thread. No settings writes.
fn run_measurement_loops(handle: &tauri::AppHandle, service: &SetupService, loops: u16) {
    use std::time::{Duration, Instant};
    let runtime = handle.state::<Arc<CaptureRuntime>>();
    let exit = handle.state::<Arc<ExitControl>>();
    let cancelled = || exit.exiting.load(Ordering::Acquire);
    let pause = |duration: Duration| {
        let until = Instant::now() + duration;
        while Instant::now() < until {
            if cancelled() {
                return false;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        !cancelled()
    };
    let fail = || {
        eprintln!("Measurement demo loop failed");
        exit.startup_failed.store(true, Ordering::Release);
        handle.exit(1);
    };
    // Also await the final replay so its failures cannot silently pass a loop run.
    for cycle in 0..=loops {
        while runtime.is_running() {
            if !pause(Duration::from_millis(100)) {
                return;
            }
        }
        if cancelled() {
            return;
        }
        if runtime.status().state != maxed_types::CaptureState::Stopped {
            fail();
            return;
        }
        if cycle == loops {
            return;
        }
        if !pause(Duration::from_secs(2)) {
            return;
        }
        if service.set_demo(false).is_err() {
            if !cancelled() {
                fail();
            }
            return;
        }
        if !pause(Duration::from_secs(2)) {
            return;
        }
        if service.set_demo(true).is_err() {
            if !cancelled() {
                fail();
            }
            return;
        }
    }
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(CaptureRuntime::default()))
        .manage(bridge::channels::LivePayloads::default())
        .on_page_load(|webview, payload| {
            let _ = webview
                .state::<bridge::channels::LivePayloads>()
                .page_load(webview.label(), payload.event());
        })
        .on_window_event(|window, event| {
            // Each app window is a WebviewWindow with the same webview label.
            // Tauri exposes its destruction through the owning window event.
            if matches!(event, tauri::WindowEvent::Destroyed) {
                let _ = window
                    .state::<bridge::channels::LivePayloads>()
                    .purge(window.label());
            }
        })
        .manage(Arc::new(ExitControl::default()))
        .manage(interactive_rail::InteractiveRail::new(
            std::env::var("MAXED_MEASURE_STARTUP").ok().as_deref(),
        ))
        .setup(|app| {
            let root = app
                .path()
                .app_data_dir()
                .map_err(|_| std::io::Error::other("Local app data is unavailable"))?;
            let lifecycle = Arc::new(setup::NativeLifecycle {
                runtime: app.state::<Arc<CaptureRuntime>>().inner().clone(),
                app: app.handle().clone(),
                app_data: root.clone(),
            });
            let service = SetupService::new(
                root.clone(),
                Arc::new(setup::NativeCapture),
                lifecycle,
                Arc::new(assets_bridge::AssetBridge::default()),
            )
            .map_err(std::io::Error::other)?
            .with_launcher_root(app.path().local_data_dir().ok());
            let service = Arc::new(service);
            let weak_setup = Arc::downgrade(&service);
            service
                .updates
                .configure_installer(Arc::new(move |updater| {
                    let setup = weak_setup.upgrade().ok_or(updater::Error::Shutdown)?;
                    let installer = setup.install_update(updater)?;
                    installer.wait()
                }));
            app.manage(service.updates.clone());
            app.state::<Arc<CaptureRuntime>>()
                .configure_alert_settings(service.settings.clone())?;
            app.state::<Arc<CaptureRuntime>>()
                .initialize_home(root)
                .map_err(std::io::Error::other)?;
            app.state::<Arc<CaptureRuntime>>()
                .inner()
                .start_alert_delivery(app.handle().clone())?;
            app.manage(service.clone());
            let handle = app.handle().clone();
            let window = app
                .config()
                .app
                .windows
                .iter()
                .find(|window| window.label == "main")
                .cloned()
                .ok_or_else(|| std::io::Error::other("Main window configuration missing"))?;
            // Read local settings off the UI thread before creating the first WebView.
            // Invalid settings still open the repair UI, using the low-memory default.
            tauri::async_runtime::spawn_blocking(move || {
                let _ = service.initialize_updates();
                let acceleration = service
                    .settings
                    .get()
                    .unwrap_or_default()
                    .hardware_acceleration;
                let result = tauri::WebviewWindowBuilder::from_config(&handle, &window)
                    .map(|builder| {
                        builder.additional_browser_args(&window_policy::browser_args(acceleration))
                    })
                    .and_then(|builder| builder.build());
                if result.is_err() {
                    eprintln!("Maxed window could not start");
                    handle
                        .state::<Arc<ExitControl>>()
                        .startup_failed
                        .store(true, Ordering::Release);
                    handle.exit(1);
                    return;
                }
                #[cfg(feature = "perf-live")]
                if runtime::perf::enabled() {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    if handle
                        .state::<Arc<CaptureRuntime>>()
                        .inner()
                        .performance_realm(handle.clone())
                        .is_err()
                    {
                        eprintln!("PERF_LIVE failed");
                    }
                    return;
                }
                // Release-enabled measurement seam; same lifecycle as the Settings command.
                let demo = std::env::var("MAXED_MEASURE_DEMO").ok();
                let loops = window_policy::measurement_demo_loops(
                    demo.as_deref(),
                    std::env::var("MAXED_MEASURE_DEMO_LOOPS").ok().as_deref(),
                );
                if !window_policy::measurement_demo(demo.as_deref()) {
                    return;
                }
                if service.set_demo(true).is_err() {
                    eprintln!("Measurement demo could not start");
                    handle
                        .state::<Arc<ExitControl>>()
                        .startup_failed
                        .store(true, Ordering::Release);
                    handle.exit(1);
                    return;
                }
                if let Some(loops) = loops {
                    run_measurement_loops(&handle, &service, loops);
                }
            });
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("maxed-sprite", |context, request, responder| {
            let main_window = context.webview_label() == "main";
            let assets = context
                .app_handle()
                .state::<Arc<SetupService>>()
                .assets
                .clone();
            tauri::async_runtime::spawn_blocking(move || {
                let response = assets.sprite_response(
                    main_window,
                    request.method(),
                    &request.uri().to_string(),
                );
                responder.respond(response);
            });
        })
        .invoke_handler(tauri::generate_handler![
            subscribe_live_payload,
            unsubscribe_live_payload,
            get_update_status,
            check_for_updates,
            download_update,
            install_update,
            cancel_update,
            get_recording_status,
            report_interactive_rail,
            get_capture_status,
            get_live_snapshot,
            get_store_status,
            get_home_summary,
            get_scope_loot,
            get_dungeon_bests,
            list_runs,
            list_drops,
            get_chat_overview,
            get_chat_sender,
            list_whisper_partners,
            list_chat,
            list_instance_events,
            get_event_summary,
            clear_chat_history,
            get_item_history,
            get_loot_stats,
            get_collection,
            get_drop_summary,
            get_stats_maps,
            list_stats_sessions,
            get_stats_session,
            get_stats_dungeons,
            export_stats_csv,
            list_alerts,
            get_alert_summary,
            get_alert_delivery_status,
            update_alert_preferences,
            update_chat_settings,
            request_notification_permission,
            test_alert_delivery,
            get_run,
            get_fight,
            get_run_fight_strip,
            get_player_breakdown,
            retry_store,
            exit_without_saving,
            set_demo,
            start_capture,
            stop_capture,
            get_setup_status,
            get_startup_setup_required,
            get_catalog_available,
            list_adapters,
            set_adapter,
            set_assets_path,
            get_settings,
            update_settings,
            search_items,
            set_chase_list,
            get_object_visuals,
            get_dungeon_portal_type,
            list_catalog_dungeon_options,
            list_catalog_dungeons
        ])
        .build(tauri::generate_context!());
    let Ok(app) = app else {
        eprintln!("Maxed window could not start");
        return;
    };
    app.run(move |app, event| {
        let exit = app.state::<Arc<ExitControl>>().inner().clone();
        let request_exit = match event {
            tauri::RunEvent::WindowEvent {
                event: tauri::WindowEvent::CloseRequested { api, .. },
                ..
            } if !exit.finished.load(Ordering::Acquire) => {
                api.prevent_close();
                true
            }
            tauri::RunEvent::ExitRequested { api, .. }
                if !exit.finished.load(Ordering::Acquire) =>
            {
                api.prevent_exit();
                true
            }
            _ => false,
        };
        if request_exit && !exit.exiting.swap(true, Ordering::AcqRel) {
            let runtime = app.state::<Arc<CaptureRuntime>>().inner().clone();
            let setup = app.state::<Arc<SetupService>>().inner().clone();
            if setup.updates.installing() {
                exit.exiting.store(false, Ordering::Release);
                return;
            }
            setup.cancel_pending(true);
            let app = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                if setup.updates.shutdown().is_err() {
                    setup.resume_after_cancelled_exit();
                    exit.exiting.store(false, Ordering::Release);
                    return;
                }
                setup.join_pending();
                tauri::async_runtime::block_on(runtime.wait_history_preparation());
                if exit.discard_confirmed(runtime.has_unsaved_summaries()) {
                    runtime.exit_without_saving();
                    let _ = runtime.flush_bridge();
                    exit.finished.store(true, Ordering::Release);
                    app.exit(exit.exit_code());
                    return;
                }
                if runtime.shutdown().is_ok() {
                    exit.finished.store(true, Ordering::Release);
                    app.exit(exit.exit_code());
                } else if !runtime.has_unsaved_summaries() {
                    // Durable summaries permit exit, but failed final IPC is
                    // recorded by shutdown and must not report a clean close.
                    exit.finished.store(true, Ordering::Release);
                    app.exit(1);
                } else {
                    setup.resume_after_cancelled_exit();
                    // No UI response defaults to cancelling exit and preserving
                    // data. Native window chrome provides a fallback confirmation:
                    // another close confirms discard.
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.set_title(
                            "Maxed — Unsaved changes. Close again to exit without saving",
                        );
                    }
                    exit.warned.store(true, Ordering::Release);
                    exit.exiting.store(false, Ordering::Release);
                }
            });
        }
    });
}

#[cfg(test)]
mod exit_tests {
    use super::*;
    #[test]
    fn startup_failure_survives_shutdown_and_discard_confirmation() {
        let exit = ExitControl::default();
        assert_eq!(exit.exit_code(), 0);
        exit.startup_failed.store(true, Ordering::Release);
        exit.exiting.store(true, Ordering::Release);
        exit.finished.store(true, Ordering::Release);
        assert_eq!(exit.exit_code(), 1);
        exit.warned.store(true, Ordering::Release);
        assert!(exit.discard_confirmed(true));
        assert_eq!(exit.exit_code(), 1);
    }

    #[test]
    fn unanswered_exit_preserves_changes_and_second_close_confirms_discard() {
        let exit = ExitControl::default();
        assert!(!exit.discard_confirmed(true));
        exit.warned.store(true, Ordering::Release);
        assert!(exit.discard_confirmed(true));
        assert!(!exit.discard_confirmed(false));
        assert!(!exit.finished.load(Ordering::Acquire));
    }
}
