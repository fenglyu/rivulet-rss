//! Run explicitly with `cargo test --test browser_smoke -- --ignored` on a machine with Chrome.
use rivulet::scraper::{ChromeScraper, ScraperConfig};
use rivulet_desktop::sources::{Access, ArticleCapture, Publisher, capture_script};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[tokio::test]
#[ignore = "requires Chrome and permission to launch a local browser"]
async fn headless_capture_reads_visible_paragraphs_and_rejects_paywalls() {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = server.local_addr().unwrap();
    server.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = stop.clone();
    let thread = std::thread::spawn(move || {
        while !server_stop.load(Ordering::Relaxed) {
            let Ok((mut connection, _)) = server.accept() else {
                std::thread::sleep(std::time::Duration::from_millis(10));
                continue;
            };
            connection
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut buffer = [0; 4096];
            let count = connection.read(&mut buffer).unwrap_or(0);
            if count == 0 {
                // Chrome may open idle preconnect sockets; do not send an article
                // before receiving an HTTP request for the intended fixture.
                continue;
            }
            let request = String::from_utf8_lossy(&buffer[..count]);
            let paragraphs = format!(
                "<p>{}</p>",
                "A visible news paragraph with useful research details. ".repeat(5)
            )
            .repeat(4);
            let gate = if request.contains("/paywall ") {
                "<div class='paywall'>Subscribe to continue</div>"
            } else {
                ""
            };
            let mut html = format!(
                "<!doctype html><html><head><title>Test story</title></head><body><h1>Test story</h1>{gate}<article>{paragraphs}<p style='display:none'>HIDDEN_SECRET_BODY</p></article></body></html>"
            );
            if request.contains("/robot ") {
                html = "<!doctype html><title>Bloomberg - Are you a robot?</title><h1>Are you a robot?</h1><p>Verification is required.</p>".into();
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                html.len(),
                html
            );
            let _ = connection.write_all(response.as_bytes());
        }
    });
    let profile = tempfile::tempdir().unwrap();
    let browser = ChromeScraper::new(ScraperConfig {
        user_data_dir: Some(profile.path().into()),
        restore_session: true,
        preserve_browser_defaults: true,
        user_agent: None,
        headless: true,
        wait_after_load_ms: 500,
        ..ScraperConfig::default()
    })
    .await
    .unwrap();
    let readable: ArticleCapture = serde_json::from_value(
        browser
            .evaluate_page(
                &format!("http://{address}/readable"),
                &capture_script(Publisher::Caixin),
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(readable.access(), Access::Extracted);
    assert!(!readable.text.contains("HIDDEN_SECRET_BODY"));
    let paywall: ArticleCapture = serde_json::from_value(
        browser
            .evaluate_page(
                &format!("http://{address}/paywall"),
                &capture_script(Publisher::Caixin),
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(paywall.access(), Access::Restricted, "{paywall:?}");
    let robot: ArticleCapture = serde_json::from_value(
        browser
            .evaluate_page(
                &format!("http://{address}/robot"),
                &capture_script(Publisher::Bloomberg),
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(robot.access(), Access::Restricted, "{robot:?}");
    assert!(robot.text.is_empty());
    browser.evaluate_page(&format!("http://{address}/readable"), "(() => { document.cookie = 'rivulet_session=active; path=/'; localStorage.setItem('rivulet_login', 'active'); return true; })()").await.unwrap();
    browser.close().await.unwrap();
    let reopened = ChromeScraper::new(ScraperConfig {
        user_data_dir: Some(profile.path().into()),
        restore_session: true,
        preserve_browser_defaults: true,
        user_agent: None,
        headless: true,
        wait_after_load_ms: 500,
        ..ScraperConfig::default()
    })
    .await
    .unwrap();
    let session = reopened.evaluate_page(&format!("http://{address}/readable"), "(() => ({ cookie: document.cookie, storage: localStorage.getItem('rivulet_login') }))()").await.unwrap();
    reopened.close().await.unwrap();
    stop.store(true, Ordering::Relaxed);
    thread.join().unwrap();
    assert_eq!(session["storage"], "active");
    assert!(
        session["cookie"]
            .as_str()
            .unwrap()
            .contains("rivulet_session=active"),
        "session cookies must survive the login-to-headless transition"
    );
}

#[tokio::test]
#[ignore = "requires Chrome and permission to launch a local browser"]
async fn user_login_browser_does_not_expose_automation() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = server.local_addr().unwrap();
    let (observations, receiver) = async_channel::unbounded();
    let server_task = tokio::spawn(async move {
        loop {
            let (mut connection, _) = server.accept().await.unwrap();
            let observations = observations.clone();
            tokio::spawn(async move {
                let mut buffer = [0; 4096];
                let Ok(Ok(count)) = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    connection.read(&mut buffer),
                )
                .await
                else {
                    return;
                };
                let request = String::from_utf8_lossy(&buffer[..count]);
                if request.contains("/probe?automated=false ") {
                    let _ = observations.send(false).await;
                }
                if request.contains("/probe?automated=true ") {
                    let _ = observations.send(true).await;
                }
                let html = if request.contains("/login ") {
                    "<!doctype html><title>Local login browser test</title><p>Local browser test; no account login.</p><script>fetch('/probe?automated=' + navigator.webdriver)</script>"
                } else {
                    "OK"
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    html.len(),
                    html
                );
                let _ = connection.write_all(response.as_bytes()).await;
            });
        }
    });
    let automated_profile = tempfile::tempdir().unwrap();
    let automated = ChromeScraper::new(ScraperConfig {
        headless: false,
        user_data_dir: Some(automated_profile.path().into()),
        user_agent: None,
        wait_after_load_ms: 500,
        ..ScraperConfig::default()
    })
    .await
    .unwrap();
    let webdriver = automated
        .evaluate_page(&format!("http://{address}/login"), "navigator.webdriver")
        .await
        .unwrap();
    assert_eq!(
        webdriver, true,
        "showing the old browser did not remove automation"
    );
    automated.close().await.unwrap();
    struct TestBrowser(std::process::Child);
    impl Drop for TestBrowser {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let profile = tempfile::tempdir().unwrap();
    let _browser = TestBrowser(
        ChromeScraper::open_user_browser(profile.path(), &format!("http://{address}/login"))
            .unwrap(),
    );
    let observed = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        while let Ok(automated) = receiver.recv().await {
            if !automated {
                return;
            }
        }
        panic!("local browser observation channel closed");
    })
    .await;
    server_task.abort();
    observed.expect("normal Chrome did not load the local test page without automation");
    assert!(!profile.path().join("DevToolsActivePort").exists());
}
