use tauri::Runtime;
use tauri::plugin::TauriPlugin;

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::<R>::new("ads")
        .invoke_handler(tauri::generate_handler![
            init_ads_window,
            hide_ads_window,
            update_ads_window_hold,
            show_ads_consent_ui,
            expand_ads_consent_webview,
            open_ads_consent_preferences,
            finish_ads_consent_flow,
            should_show_ads_consent_popup,
            perform_ads_consent_action,
            record_ads_click,
            open_link,
            get_ads_personalization,
        ])
        .build()
}

#[tauri::command]
pub async fn init_ads_window(
    _dpr: Option<f32>,
    _override_shown: Option<bool>,
) -> crate::api::Result<()> {
    Ok(())
}

#[tauri::command]
pub async fn hide_ads_window(
    _reset: Option<bool>,
) -> crate::api::Result<()> {
    Ok(())
}

#[tauri::command]
pub async fn update_ads_window_hold(
    _acquire: Option<bool>,
    _dpr: Option<f32>,
) -> crate::api::Result<()> {
    Ok(())
}

#[tauri::command]
pub async fn show_ads_consent_ui(
    _notification_enabled: Option<bool>,
) -> crate::api::Result<()> {
    Ok(())
}

#[tauri::command]
pub async fn expand_ads_consent_webview() -> crate::api::Result<()> {
    Ok(())
}

#[tauri::command]
pub async fn open_ads_consent_preferences() -> crate::api::Result<()> {
    Ok(())
}

#[tauri::command]
pub async fn finish_ads_consent_flow(
    _dpr: Option<f32>,
) -> crate::api::Result<()> {
    Ok(())
}

#[tauri::command]
pub async fn should_show_ads_consent_popup() -> crate::api::Result<bool> {
    Ok(false)
}

#[tauri::command]
pub async fn perform_ads_consent_action(
    _action: Option<String>,
) -> crate::api::Result<()> {
    Ok(())
}

#[tauri::command]
pub async fn record_ads_click() -> crate::api::Result<()> {
    Ok(())
}

#[tauri::command]
pub async fn open_link(
    _path: Option<String>,
    _origin: Option<String>,
) -> crate::api::Result<()> {
    Ok(())
}

#[tauri::command]
pub async fn get_ads_personalization() -> crate::api::Result<bool> {
    Ok(false)
}
