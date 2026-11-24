//! Webview permissions Shuttle answers for itself.
//!
//! The voice-note recorder calls `navigator.mediaDevices.getUserMedia`. Every webview engine
//! treats that as a permission request and, left alone, denies it: WebKitGTK waits for the
//! `permission-request` signal and denies when nothing handles it, and WebView2 leaves the
//! answer to a browser prompt a desktop app cannot draw. Shuttle grants its own UI the
//! microphone so recording starts without a settings trip, and keeps every other request on
//! the engine's default.

/// A capability a webview can request from the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    Microphone,
    Camera,
    Other,
}

/// Whether a request may be granted.
///
/// Only audio capture, and only from a URL Shuttle itself serves. Camera, location,
/// notifications and everything else keep the webview's default answer.
pub fn allows(capability: Capability, request_url: Option<&str>) -> bool {
    capability == Capability::Microphone && request_url.is_some_and(is_app_url)
}

/// One of the URLs Shuttle serves its own UI from: the `tauri` protocol on Linux and macOS,
/// the `tauri.localhost` workaround wry uses on Windows, and the Vite dev server.
pub fn is_app_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let (host, port) = match host_port.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (host_port, None),
    };
    match scheme.to_ascii_lowercase().as_str() {
        "tauri" => host.eq_ignore_ascii_case("localhost"),
        "http" | "https" => {
            host.eq_ignore_ascii_case("tauri.localhost")
                || (host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1")
                    && port == Some("1420")
        }
        _ => false,
    }
}

/// Grants the microphone to the main window's webview. Call once, when the window exists.
pub fn allow_microphone<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> tauri::Result<()> {
    #[cfg(target_os = "linux")]
    {
        window.with_webview(|platform| install_on(&platform.inner()))
    }

    #[cfg(windows)]
    {
        window.with_webview(|platform| install_on(&platform.controller()))
    }

    #[cfg(target_os = "macos")]
    {
        // wry answers `requestMediaCapturePermissionForOrigin` with Grant already, so macOS
        // needs the bundle's NSMicrophoneUsageDescription and nothing from this module.
        let _ = window;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn install_on(webview: &webkit2gtk::WebView) {
    use webkit2gtk::glib::{object::Cast, Object};
    use webkit2gtk::{
        PermissionRequestExt, UserMediaPermissionRequest, UserMediaPermissionRequestExt, WebViewExt,
    };

    webview.connect_permission_request(|webview, request| {
        let object: &Object = request.upcast_ref();
        let Some(media) = object.downcast_ref::<UserMediaPermissionRequest>() else {
            // Location, notifications and the rest keep WebKit's own answer, which is deny.
            return false;
        };
        let capability = if media.is_for_video_device() {
            Capability::Camera
        } else if media.is_for_audio_device() {
            Capability::Microphone
        } else {
            Capability::Other
        };
        if allows(capability, webview.uri().as_deref()) {
            request.allow();
        } else {
            request.deny();
        }
        true
    });
}

#[cfg(windows)]
fn install_on(controller: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PERMISSION_KIND, COREWEBVIEW2_PERMISSION_KIND_CAMERA,
        COREWEBVIEW2_PERMISSION_KIND_MICROPHONE, COREWEBVIEW2_PERMISSION_STATE_ALLOW,
        COREWEBVIEW2_PERMISSION_STATE_DENY,
    };
    use webview2_com::{take_pwstr, PermissionRequestedEventHandler};
    use windows_core::PWSTR;

    let Ok(core) = (unsafe { controller.CoreWebView2() }) else {
        return;
    };
    let mut token = 0i64;
    unsafe {
        let _ = core.add_PermissionRequested(
            &PermissionRequestedEventHandler::create(Box::new(|_, args| {
                let Some(args) = args else {
                    return Ok(());
                };
                let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
                args.PermissionKind(&mut kind)?;
                let capability = match kind {
                    COREWEBVIEW2_PERMISSION_KIND_MICROPHONE => Capability::Microphone,
                    COREWEBVIEW2_PERMISSION_KIND_CAMERA => Capability::Camera,
                    // Location, notifications and the rest keep WebView2's own answer.
                    _ => return Ok(()),
                };
                let mut raw = PWSTR::null();
                args.Uri(&mut raw)?;
                let url = take_pwstr(raw);
                let state = if allows(capability, Some(&url)) {
                    COREWEBVIEW2_PERMISSION_STATE_ALLOW
                } else {
                    COREWEBVIEW2_PERMISSION_STATE_DENY
                };
                args.SetState(state)?;
                Ok(())
            })),
            &mut token,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{allows, is_app_url, Capability};

    #[test]
    fn the_app_ui_is_recognised_on_every_platform() {
        assert!(is_app_url("tauri://localhost"));
        assert!(is_app_url("tauri://localhost/index.html"));
        assert!(is_app_url("http://tauri.localhost/"));
        assert!(is_app_url("https://tauri.localhost/index.html"));
        assert!(is_app_url("http://localhost:1420/"));
        assert!(is_app_url("http://127.0.0.1:1420/?target=main"));
    }

    #[test]
    fn another_site_is_not_the_app_ui() {
        assert!(!is_app_url("https://web.whatsapp.com/"));
        assert!(!is_app_url("http://localhost:8080/"));
        assert!(!is_app_url("https://tauri.localhost.evil.test/"));
        assert!(!is_app_url("about:blank"));
        assert!(!is_app_url(""));
    }

    #[test]
    fn audio_capture_is_allowed_for_the_app_ui() {
        assert!(allows(Capability::Microphone, Some("tauri://localhost/")));
        assert!(allows(Capability::Microphone, Some("http://localhost:1420/")));
    }

    #[test]
    fn the_microphone_is_denied_to_any_other_page() {
        assert!(!allows(
            Capability::Microphone,
            Some("https://web.whatsapp.com/")
        ));
        assert!(!allows(Capability::Microphone, None));
    }

    #[test]
    fn video_and_unrelated_requests_are_denied_even_from_the_app_ui() {
        assert!(!allows(Capability::Camera, Some("tauri://localhost/")));
        assert!(!allows(Capability::Other, Some("tauri://localhost/")));
    }
}

/// Runs a real WebKitGTK webview so the signal wiring is proven, not just the policy.
///
/// Phase one leaves the handler out and expects the browser-style denial the app used to
/// show. Phase two installs it and expects no denial.
#[cfg(all(test, target_os = "linux"))]
mod live {
    use gtk::prelude::{ContainerExt, GtkWindowExt, WidgetExt};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use webkit2gtk::WebViewExt;

    const ORIGIN: &str = "http://127.0.0.1:1420/";

    const PAGE: &str = r#"<html><body><script>
      const report = (text) => { fetch('/r/' + text.replace(/ /g, '_')); };
      window.onerror = (message) => report('error ' + message);
      if (!navigator.mediaDevices) {
        report('no-mediaDevices');
      } else {
        navigator.mediaDevices.getUserMedia({ audio: true }).then(
          (stream) => {
            stream.getTracks().forEach((t) => t.stop());
            navigator.mediaDevices.getUserMedia({ video: true }).then(
              () => report('audio=allowed video=allowed'),
              (error) => report('audio=allowed video=' + error.name)
            );
          },
          (error) => report('audio=' + error.name)
        );
      }
    </script></body></html>"#;

    /// Serves the page and records the result it reports, on an origin the policy allows.
    fn serve_page() -> Option<(std::thread::JoinHandle<()>, Arc<Mutex<Option<String>>>)> {
        let listener = bind();
        let answer = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&answer);
        let server = std::thread::spawn(move || {
            let Ok(listener) = listener else { return };
            for stream in listener.incoming() {
                let Ok(mut socket) = stream else { return };
                let mut request = [0u8; 2048];
                let read = socket.read(&mut request).unwrap_or(0);
                let head = String::from_utf8_lossy(&request[..read]);
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                if let Some(reported) = path.strip_prefix("/r/") {
                    *slot.lock().unwrap() = Some(reported.replace('_', " "));
                } else if path == "/stop" {
                    break;
                }
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    PAGE.len(),
                    PAGE
                );
                let _ = socket.write_all(response.as_bytes());
            }
        });
        Some((server, answer))
    }

    /// The previous phase's listener may still hold the port for a moment.
    fn bind() -> std::io::Result<TcpListener> {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match TcpListener::bind("127.0.0.1:1420") {
                Ok(listener) => return Ok(listener),
                Err(error) if Instant::now() < deadline => {
                    let _ = error;
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Loads the page in a fresh webview and returns what `getUserMedia` reported, if anything.
    fn ask_for_audio(grant: bool) -> Option<String> {
        let (server, reported) = serve_page()?;
        gtk::init().expect("gtk init");
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        let webview = webkit2gtk::WebView::new();
        window.add(&webview);
        // WebKit only asks for capture once the view is mapped; keep the window out of sight.
        window.set_default_size(1, 1);
        window.move_(-100, -100);
        window.show_all();
        if grant {
            super::install_on(&webview);
        }
        webview.connect_load_failed(|_, _, url, error| {
            eprintln!("load failed: {url} {error}");
            false
        });
        webview.load_uri(ORIGIN);

        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline && reported.lock().unwrap().is_none() {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        // The server thread waits for one more connection; this releases it.
        if let Ok(mut socket) = std::net::TcpStream::connect("127.0.0.1:1420") {
            let _ = socket.write_all(b"GET /stop HTTP/1.1\r\n\r\n");
        }
        let _ = server.join();
        let answer = reported.lock().unwrap().clone();
        answer
    }

    #[test]
    #[ignore = "needs a display: cargo test --lib live_ -- --ignored --nocapture"]
    fn live_webkit_turns_the_denial_into_an_allow() {
        let without = ask_for_audio(false);
        eprintln!("without the handler: {without:?}");
        assert_eq!(
            without.as_deref(),
            Some("audio=NotAllowedError"),
            "WebKit was expected to deny audio capture when nothing handles the signal"
        );

        let with = ask_for_audio(true).expect("the page did not report a getUserMedia result");
        eprintln!("with the handler: {with}");
        assert!(
            with.starts_with("audio=allowed"),
            "audio capture was not granted: {with}"
        );
        assert!(
            with.contains("video=NotAllowedError"),
            "video capture was not denied: {with}"
        );
    }
}
