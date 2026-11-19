#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use fastdict::api::{RunningServer, ServeConfig};
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

fn local_import_roots() -> Vec<PathBuf> {
    #[cfg(unix)]
    {
        vec![PathBuf::from("/")]
    }
    #[cfg(windows)]
    {
        ('A'..='Z')
            .map(|letter| PathBuf::from(format!("{letter}:\\")))
            .filter(|root| root.exists())
            .collect()
    }
}

#[tauri::command]
async fn pick_import_folders(app: tauri::AppHandle) -> Vec<String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("选择一个或多个词典文件夹")
        .pick_folders(move |folders| {
            let paths = folders
                .unwrap_or_default()
                .into_iter()
                .filter_map(|p| p.into_path().ok())
                .map(|p| p.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let _ = tx.send(paths);
        });
    rx.await.unwrap_or_default()
}

#[tauri::command]
async fn pick_import_files(app: tauri::AppHandle) -> Vec<String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("选择词典与伴随资源文件")
        .pick_files(move |files| {
            let paths = files
                .unwrap_or_default()
                .into_iter()
                .filter_map(|p| p.into_path().ok())
                .map(|p| p.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let _ = tx.send(paths);
        });
    rx.await.unwrap_or_default()
}

struct EmbeddedBackend {
    runtime: tokio::runtime::Runtime,
    server: Option<RunningServer>,
}

impl EmbeddedBackend {
    fn shutdown(&mut self) {
        if let Some(server) = self.server.take() {
            let _ = self.runtime.block_on(server.shutdown());
        }
    }
}

fn resolve_data_dir(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(raw) = std::env::var("FASTDICT_DATA_DIR") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    if let Ok(dir) = app.path().local_data_dir().or_else(|_| app.path().data_dir()) {
        return dir.join("fastdict");
    }
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from(".fastdict"))
}

fn main() {
    #[cfg(target_os = "linux")]
    {
        // Prevent WebKitGTK blank/dark-gray surface caused by DMA-BUF / EGL conflicts
        // on Wayland compositors (Niri, Hyprland, Sway), NVIDIA, and XWayland AppImages.
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            // SAFETY: Called single-threaded at process start before spawning threads or GTK.
            unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
        }
        if std::env::var_os("GDK_BACKEND").as_deref() == Some(std::ffi::OsStr::new("x11"))
            && std::env::var_os("EGL_PLATFORM").as_deref() == Some(std::ffi::OsStr::new("wayland"))
        {
            // SAFETY: Called single-threaded at process start before spawning threads or GTK.
            unsafe { std::env::remove_var("EGL_PLATFORM") };
        }
    }

    let backend_holder: Arc<Mutex<Option<EmbeddedBackend>>> = Arc::new(Mutex::new(None));
    let shutdown_holder = backend_holder.clone();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            pick_import_folders,
            pick_import_files
        ])
        .setup(move |app| {
            let data_dir = resolve_data_dir(app.handle());
            let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());

            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|e| std::io::Error::other(format!("failed to create tokio runtime: {e}")))?;

            let config = ServeConfig {
                listen: "127.0.0.1:0".parse().expect("valid loopback addr"),
                data_dir: data_dir.clone(),
                token: Some(token.clone()),
                import_roots: local_import_roots(),
                static_dir: PathBuf::new(),
            };

            let server = runtime
                .block_on(fastdict::api::start(config))
                .map_err(|e| std::io::Error::other(format!("failed to start local fastdict engine: {e}")))?;

            let addr = server.addr();
            let base_url = format!("http://{addr}");
            let canonical_data_dir = data_dir
                .canonicalize()
                .unwrap_or(data_dir)
                .to_string_lossy()
                .into_owned();

            let bridge_payload = json!({
                "base_url": base_url,
                "token": token,
                "data_dir": canonical_data_dir,
                "local_only": true,
            });
            let init_script = format!("window.__FASTDICT_DESKTOP__ = {bridge_payload};");

            *backend_holder.lock().unwrap() = Some(EmbeddedBackend {
                runtime,
                server: Some(server),
            });

            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("FastDict")
                .inner_size(1200.0, 820.0)
                .min_inner_size(760.0, 560.0)
                .center()
                .initialization_script(&init_script)
                .build()?;

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building FastDict desktop application");

    app.run(move |_handle, event| {
        if matches!(event, RunEvent::Exit) {
            if let Some(mut backend) = shutdown_holder.lock().unwrap().take() {
                backend.shutdown();
            }
        }
    });
}
