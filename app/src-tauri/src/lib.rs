#![deny(unsafe_code)]

pub mod assets_bridge;
pub mod bridge;
pub mod demo;
mod exit;
mod gui_smoke;
use exit::{ExitControl, ExitHost};
pub mod interactive_rail;
pub mod recording;
pub mod runtime;
pub mod settings;
pub mod setup;
mod startup_cache;
mod storage_usage;
pub mod updater;
pub mod window_policy;

use maxed_types::{
    CaptureStatus, CharacterCalculation, CharacterCalculationRequest, LiveSnapshot, StoreStatus,
};
use runtime::CaptureRuntime;
use setup::SetupService;
use std::sync::{atomic::Ordering, Arc};
use tauri::{Emitter, Manager};
use updater::service::UpdateService;

struct NativeExit {
    app: tauri::AppHandle,
    runtime: Arc<CaptureRuntime>,
    setup: Arc<SetupService>,
}
impl ExitHost for NativeExit {
    fn installing(&self) -> bool {
        self.setup.updates.installing()
    }
    fn stop_updates(&self) -> Result<(), String> {
        self.setup.updates.shutdown()
    }
    fn prepare(&self) {
        self.setup.cancel_pending(true);
        self.setup.join_pending();
        tauri::async_runtime::block_on(self.runtime.wait_history_preparation());
    }
    fn resume(&self) {
        self.setup.resume_after_cancelled_exit();
    }
    fn unsaved(&self) -> bool {
        self.runtime.has_unsaved_summaries()
    }
    fn delivery_failed(&self) -> bool {
        self.runtime.presentation_failed()
    }
    fn retry_save(&self) -> Result<(), String> {
        self.runtime.retry_store()
    }
    fn shutdown(&self) -> Result<(), String> {
        self.runtime.shutdown()
    }
    fn discard(&self) {
        self.runtime.exit_without_saving();
        let _ = self.runtime.flush_bridge();
    }
    fn warn(&self, native_fallback: bool) {
        let _ = self.app.emit("unsaved-close", ());
        if let Some(window) = self.app.get_webview_window("main") {
            let title = if native_fallback {
                exit::native_warning_title()
            } else {
                exit::UNSAVED_TITLE.to_string()
            };
            let _ = window.set_title(&title);
        }
    }
    fn exit(&self, code: i32) {
        self.app.exit(code);
    }
}
fn exit_host(app: &tauri::AppHandle) -> NativeExit {
    NativeExit {
        app: app.clone(),
        runtime: app.state::<Arc<CaptureRuntime>>().inner().clone(),
        setup: app.state::<Arc<SetupService>>().inner().clone(),
    }
}
#[tauri::command]
async fn exit_without_saving(app: tauri::AppHandle) -> Result<(), String> {
    let control = app.state::<Arc<ExitControl>>().inner().clone();
    let host = exit_host(&app);
    tauri::async_runtime::spawn_blocking(move || exit::run(&control, &host, true))
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
async fn get_database_size(setup: tauri::State<'_, Arc<SetupService>>) -> Result<String, String> {
    let setup = setup.inner().clone();
    tauri::async_runtime::spawn_blocking(move || setup.database_size().map(|size| size.to_string()))
        .await
        .map_err(|_| "Database size unavailable.".to_string())?
}
#[tauri::command]
async fn get_history_upgrade_status(
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<maxed_types::HistoryUpgradeStatus, String> {
    let setup = setup.inner().clone();
    tauri::async_runtime::spawn_blocking(move || setup.history_upgrade_status())
        .await
        .map_err(|_| "History upgrade status is unavailable.".to_string())?
}
#[tauri::command]
async fn backup_and_upgrade_history(
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<maxed_types::HistoryUpgradeStatus, String> {
    let setup = setup.inner().clone();
    tauri::async_runtime::spawn_blocking(move || setup.backup_and_upgrade_history())
        .await
        .map_err(|_| "History upgrade is unavailable.".to_string())?
}
#[tauri::command]
async fn get_storage_status(
    estimate: Option<bool>,
    setup: tauri::State<'_, Arc<SetupService>>,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::StorageStatus, String> {
    let setup = setup.inner().clone();
    let status = tauri::async_runtime::spawn_blocking(move || setup.storage_status())
        .await
        .map_err(|_| "Storage status is unavailable.".to_string())??;
    Ok(runtime
        .inner()
        .storage_status(status, estimate.unwrap_or(false))
        .await)
}
#[tauri::command]
async fn compact_storage(
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::StorageCommandOutcome, String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.compact_storage())
        .await
        .map_err(|_| "Storage maintenance is unavailable.".to_string())?
}
#[tauri::command]
async fn cancel_storage_maintenance(
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::StorageCommandOutcome, String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.cancel_storage_maintenance())
        .await
        .map_err(|_| "Storage maintenance is unavailable.".to_string())?
}
#[tauri::command]
async fn resume_storage_maintenance(
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::StorageCommandOutcome, String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.resume_storage_maintenance())
        .await
        .map_err(|_| "Storage maintenance is unavailable.".to_string())?
}
#[tauri::command]
async fn list_storage_backups(
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<Vec<maxed_types::ManagedStorageBackup>, String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.list_storage_backups())
        .await
        .map_err(|_| "Storage backups are unavailable.".to_string())?
}
#[tauri::command]
async fn delete_storage_backup(
    id: String,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<(), String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.delete_storage_backup(&id))
        .await
        .map_err(|_| "Storage backup could not be deleted.".to_string())?
}
#[tauri::command]
async fn get_storage_backup_location(
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<String, String> {
    setup.storage_backup_location()
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
async fn get_capture_status(app: tauri::AppHandle) -> CaptureStatus {
    let runtime = app.state::<Arc<CaptureRuntime>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.status())
        .await
        .unwrap_or_else(|_| CaptureStatus {
            state: maxed_types::CaptureState::Failed,
            message: "Capture status worker failed; restart Maxed".into(),
            ..Default::default()
        })
}

#[tauri::command]
async fn get_live_snapshot(
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<LiveSnapshot, String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.snapshot())
        .await
        .map_err(|_| "Live snapshot worker failed".into())
}

#[tauri::command]
async fn get_character_calculation(
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
    request: CharacterCalculationRequest,
) -> Result<CharacterCalculation, String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.get_character_calculation(request))
        .await
        .map_err(|_| "character_calculation_worker_failed".to_string())?
}

#[tauri::command]
fn set_document_visible(window: tauri::Window, visible: bool) -> bool {
    if window.label() != "main" {
        return true;
    }
    window
        .state::<Arc<bridge::PresentationVisibility>>()
        .document_visible
        .store(visible, Ordering::Release);
    update_presentation_visibility(&window)
}

fn update_presentation_visibility(window: &tauri::Window) -> bool {
    if window.label() != "main" {
        return true;
    }
    let state = window.state::<Arc<bridge::PresentationVisibility>>();
    let visible = state.document_visible.load(Ordering::Acquire)
        && window.is_visible().unwrap_or(true)
        && !window.is_minimized().unwrap_or(false);
    let app = window.app_handle();
    if !state.set_visible(visible) {
        return visible;
    }
    let epoch = state.epoch();
    let _ = app.emit("presentation-visible", visible);
    // The window callback must not wait for the bounded publisher queue.
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if app.state::<Arc<bridge::PresentationVisibility>>().epoch() != epoch {
            return;
        }
        if visible {
            let runtime = app.state::<Arc<CaptureRuntime>>();
            if let Ok(publisher) = runtime.publisher(app.clone()) {
                let snapshot = runtime.snapshot();
                // A read can wait behind capture/storage work. Do not publish
                // its restore baseline after another hide/restore transition.
                if app.state::<Arc<bridge::PresentationVisibility>>().epoch() != epoch {
                    return;
                }
                publisher.publish(bridge::Publication::Snapshot(Box::new(snapshot)));
                publisher.publish(bridge::Publication::PartyChanged(bridge::PartyChanged {
                    scope_id: None,
                }));
            }
        }
    });
    visible
}

#[tauri::command]
async fn subscribe_live_payload(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
    payloads: tauri::State<'_, bridge::channels::LivePayloads>,
    event: String,
    channel: tauri::ipc::Channel,
) -> Result<u32, String> {
    if event == "party-changed" {
        // Preset picks and annotations can change before capture ever starts.
        runtime.publisher(app.clone())?;
    }
    let id = payloads.subscribe(webview.label(), event, channel)?;
    let runtime = runtime.inner().clone();
    let reconnected =
        tauri::async_runtime::spawn_blocking(move || runtime.reconnect_presentation())
            .await
            .map_err(|_| "Presentation reconnect worker failed".to_string())
            .and_then(|result| result);
    if let Err(error) = reconnected {
        let _ = payloads.unsubscribe(webview.label(), id);
        return Err(error);
    }
    let title = app.state::<Arc<ExitControl>>().presentation_reconnected();
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_title(title);
    }
    Ok(id)
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
async fn get_store_status(app: tauri::AppHandle) -> StoreStatus {
    let runtime = app.state::<Arc<CaptureRuntime>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.store_status())
        .await
        .unwrap_or_else(|_| StoreStatus {
            state: maxed_types::StoreState::Failed,
            queued: 0,
            durable: None,
            stream_id: None,
            message: "Store status worker failed; restart Maxed".into(),
        })
}

#[tauri::command]
async fn get_home_summary(
    scope: maxed_types::HomeScope,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::HomeSummary>, String> {
    runtime.inner().home_summary(scope).await
}

#[tauri::command]
async fn get_home_dungeon_streak(
    request: maxed_types::HomeDungeonStreakRequest,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::HomeDungeonStreak>, String> {
    runtime.inner().home_dungeon_streak(request).await
}

#[tauri::command]
async fn get_home_hud(
    scope: maxed_types::HomeScope,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<maxed_types::HistoryReply<maxed_types::HomeHud>, String> {
    runtime
        .inner()
        .home_hud(scope, setup.catalog_query_data())
        .await
}

#[tauri::command]
async fn get_run_recap(
    request: maxed_types::RunRecapRequest,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<maxed_types::HistoryReply<Option<maxed_types::RunRecap>>, String> {
    runtime
        .inner()
        .run_recap(request, setup.catalog_query_data())
        .await
}

#[tauri::command]
async fn get_visit_recap(
    request: maxed_types::VisitRecapRequest,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<Option<maxed_types::VisitRecap>>, String> {
    runtime.inner().visit_recap(request).await
}

#[tauri::command]
async fn get_scope_loot(
    scope: maxed_types::HomeScope,
    cursor: Option<String>,
    limit: u16,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<maxed_types::HistoryReply<maxed_types::ScopeLootPage>, String> {
    let tiers = setup.catalog_query_data();
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
async fn update_party_settings(
    settings: maxed_types::PartySettings,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<(), String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.update_party_settings(settings))
        .await
        .map_err(|_| "Party settings worker failed".to_string())?
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
async fn get_boss_history(
    request: maxed_types::BossHistoryRequest,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::BossHistory>, String> {
    runtime.inner().boss_history(request).await
}

#[tauri::command]
async fn get_home_boss_baselines(
    request: maxed_types::BossHistoryRequest,
    runtime: tauri::State<'_, std::sync::Arc<runtime::CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::HomeBossBaselines>, String> {
    runtime.inner().home_boss_baselines(request).await
}

#[tauri::command]
async fn get_dungeon_profile(
    request: maxed_types::DungeonProfileRequest,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::DungeonProfile>, String> {
    runtime.inner().dungeon_profile(request).await
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
    let tiers = setup.catalog_query_data();
    runtime.inner().get_drop_summary(request, tiers).await
}

#[tauri::command]
async fn get_collection(
    request: maxed_types::CollectionRequest,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<maxed_types::HistoryReply<maxed_types::Collection>, String> {
    let tiers = setup.catalog_query_data();
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
async fn get_run_player(
    run_id: String,
    player_id: String,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<Option<maxed_types::RunPlayerView>>, String> {
    runtime.inner().get_run_player(run_id, player_id).await
}

#[tauri::command]
async fn get_party_live(
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::PartyView>, String> {
    runtime.inner().get_party_live().await
}

#[tauri::command]
async fn get_party_run(
    run_id: String,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<Option<maxed_types::PartyView>>, String> {
    runtime.inner().get_party_run(run_id).await
}

#[tauri::command]
async fn get_party_players(
    query: String,
    filter: maxed_types::PartyPlayerFilter,
    sort: maxed_types::PartyPlayerSort,
    cursor: Option<maxed_types::PartyPlayersCursor>,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<maxed_types::PartyPlayersPage>, String> {
    runtime
        .inner()
        .get_party_players(query, filter, sort, cursor)
        .await
}

#[tauri::command]
async fn get_party_player(
    key: String,
    cursor: Option<maxed_types::PartyRunsCursor>,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<Option<maxed_types::PartyPlayerView>>, String> {
    runtime.inner().get_party_player(key, cursor).await
}

#[tauri::command]
async fn set_party_mark(
    key: String,
    name: String,
    mark: Option<maxed_types::PartyMark>,
    note: Option<String>,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<(), String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.set_party_mark(key, name, mark, note))
        .await
        .map_err(|_| "Party worker failed".to_string())?
}

#[tauri::command]
async fn clear_party_history(
    options: maxed_types::ClearPartyHistory,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<(), String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.clear_party_history(options))
        .await
        .map_err(|_| "Party worker failed".to_string())?
}

#[tauri::command]
async fn set_party_preset_pick(
    preset_id: Option<String>,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<(), String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.set_party_preset_pick(preset_id))
        .await
        .map_err(|_| "Party worker failed".to_string())?
}

#[tauri::command]
async fn open_realmeye(name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || runtime::open_realmeye(&name))
        .await
        .map_err(|_| "Browser handoff failed".to_string())?
}

#[tauri::command]
async fn get_player_context(
    scope_id: String,
    player_id: String,
    from: Option<maxed_types::Timestamp>,
    until: Option<maxed_types::Timestamp>,
    runtime: tauri::State<'_, Arc<CaptureRuntime>>,
) -> Result<maxed_types::HistoryReply<Option<maxed_types::PlayerContextView>>, String> {
    runtime
        .inner()
        .get_player_context(scope_id, player_id, from, until)
        .await
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
    if gui_smoke::enabled() {
        return Ok(true);
    }
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
async fn get_item_details(
    object_types: Vec<u16>,
    enchant_ids: Vec<i32>,
    setup: tauri::State<'_, Arc<SetupService>>,
) -> Result<maxed_types::ItemDetailPage, String> {
    let setup = setup.inner().clone();
    tauri::async_runtime::spawn_blocking(move || setup.item_details(&object_types, &enchant_ids))
        .await
        .map_err(|_| "Item detail worker failed.")?
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
    app.state::<Arc<ExitControl>>().clear_native_warning();
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

fn report_build_failure() -> ! {
    const MESSAGE: &str =
        "Maxed could not start. Please restart Maxed. If the problem continues, reinstall Maxed.";
    let failure = exit::startup_failure(|name| std::env::var_os(name).is_some());
    eprintln!("{MESSAGE}");
    #[cfg(windows)]
    if failure.show_dialog {
        show_startup_error(MESSAGE);
    }
    std::process::exit(failure.exit_code);
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn show_startup_error(message: &str) {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    let message: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    // SAFETY: Both strings are NUL-terminated and live throughout this blocking
    // call. No owner window exists yet; no pointer is retained by MessageBoxW.
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            w!("Maxed"),
            MB_OK | MB_ICONERROR,
        );
    }
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(CaptureRuntime::default()))
        .manage(bridge::channels::LivePayloads::default())
        .manage(Arc::new(bridge::PresentationVisibility::default()))
        .on_page_load(|webview, payload| {
            let _ = webview
                .state::<bridge::channels::LivePayloads>()
                .page_load(webview.label(), payload.event());
        })
        .on_window_event(|window, event| {
            if matches!(
                event,
                tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Focused(_)
            ) {
                update_presentation_visibility(window);
            }
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
            .with_launcher_root(if gui_smoke::enabled() {
                None
            } else {
                app.path().local_data_dir().ok()
            });
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
            let storage_notices = app.handle().clone();
            app.state::<Arc<CaptureRuntime>>()
                .set_storage_notice_sink(Arc::new(move |notice| {
                    let _ = storage_notices.emit("storage-maintenance-changed", notice);
                }));
            // Account-history notices also apply before capture has ever started.
            app.state::<Arc<CaptureRuntime>>()
                .publisher(app.handle().clone())
                .map_err(std::io::Error::other)?;
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
                // Smoke always wins over inherited measurement/demo flags.
                if gui_smoke::enabled() {
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
                    request
                        .headers()
                        .get("Origin")
                        .and_then(|v| v.to_str().ok()),
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
            get_database_size,
            get_history_upgrade_status,
            backup_and_upgrade_history,
            get_storage_status,
            compact_storage,
            cancel_storage_maintenance,
            resume_storage_maintenance,
            list_storage_backups,
            delete_storage_backup,
            get_storage_backup_location,
            report_interactive_rail,
            get_capture_status,
            get_live_snapshot,
            get_character_calculation,
            set_document_visible,
            get_store_status,
            get_home_summary,
            get_home_hud,
            get_home_dungeon_streak,
            get_run_recap,
            get_visit_recap,
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
            get_boss_history,
            get_home_boss_baselines,
            get_dungeon_profile,
            export_stats_csv,
            list_alerts,
            get_alert_summary,
            get_alert_delivery_status,
            update_alert_preferences,
            update_chat_settings,
            update_party_settings,
            request_notification_permission,
            test_alert_delivery,
            get_run,
            get_fight,
            get_run_fight_strip,
            get_player_breakdown,
            get_run_player,
            get_party_live,
            get_party_run,
            get_party_players,
            get_party_player,
            set_party_mark,
            clear_party_history,
            set_party_preset_pick,
            open_realmeye,
            get_player_context,
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
            get_item_details,
            get_dungeon_portal_type,
            list_catalog_dungeon_options,
            list_catalog_dungeons
        ])
        .build(tauri::generate_context!());
    let Ok(app) = app else {
        report_build_failure();
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
        if request_exit {
            let host = exit_host(app);
            tauri::async_runtime::spawn_blocking(move || {
                let _ = exit::run(&exit, &host, false);
            });
        }
    });
}
