//! Seeds the demo board, serves it on a free port and screenshots the page with a headless browser.
//! Exit 0 on success, 1 on a failure, 3 when no headless browser is installed.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const BROWSERS: [&str; 4] = ["google-chrome", "chromium", "chromium-browser", "firefox"];

// The plan 07 seeder takes only the root. {root} is replaced before running.
const SEED_PROGRAM: &str = "bash";
const SEED_ARGS: [&str; 2] = ["dashboard/tests/seed-demo.sh", "{root}"];
const SERVER_BIN: &str = "dashboard/target/debug/factory-server";

pub fn pick_browser(installed: impl Fn(&str) -> bool) -> Option<&'static str> {
    BROWSERS.iter().copied().find(|n| installed(n))
}

pub fn shot_args(
    browser: &str,
    file: &str,
    w: u32,
    h: u32,
    url: &str,
    extra: &[&str],
) -> Vec<String> {
    if browser == "firefox" {
        return vec![
            "--headless".into(),
            "--screenshot".into(),
            file.into(),
            "--window-size".into(),
            format!("{w},{h}"),
            url.into(),
        ];
    }
    let mut args: Vec<String> = [
        "--headless=new",
        "--no-sandbox",
        "--disable-gpu",
        "--hide-scrollbars",
    ]
    .map(String::from)
    .to_vec();
    args.extend(extra.iter().map(|s| s.to_string()));
    args.push("--virtual-time-budget=4000".into());
    args.push(format!("--window-size={w},{h}"));
    args.push(format!("--screenshot={file}"));
    args.push(url.into());
    args
}

pub fn blocked_message() -> String {
    "BLOCKED: no headless browser found (tried google-chrome, chromium, chromium-browser, firefox).\n\
     Manual steps: seed the demo board, start the dashboard, open it in a real window and check dark theme,\n\
     light theme and a 400 px wide window: six stage columns, summary strip, readable review chips,\n\
     needs-human card with its copy chips, no sideways page scroll."
        .to_string()
}

pub fn free_port() -> std::io::Result<u16> {
    Ok(TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port())
}

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
        .unwrap_or(false)
}

fn healthy(port: u16) -> bool {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return false;
    };
    let request = format!("GET /healthz HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\n\r\n");
    if s.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut buf = String::new();
    s.read_to_string(&mut buf).is_ok() && buf.starts_with("HTTP/1.") && buf.contains(" 200")
}

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run() -> Result<i32, Box<dyn std::error::Error>> {
    let Some(browser) = pick_browser(on_path) else {
        eprintln!("{}", blocked_message());
        return Ok(3);
    };
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| ".superpowers/factory/screenshots".into()),
    );
    std::fs::create_dir_all(&out)?;
    let tmp = tempfile::tempdir()?;
    let root = tmp.path().join("root");
    let port = free_port()?;

    let args: Vec<String> = SEED_ARGS
        .iter()
        .map(|a| a.replace("{root}", &root.to_string_lossy()))
        .collect();
    if !Command::new(SEED_PROGRAM)
        .args(&args)
        .stdout(Stdio::null())
        .status()?
        .success()
    {
        eprintln!("FAIL: the demo seeder failed");
        return Ok(1);
    }
    let built = Command::new("cargo")
        .args([
            "build",
            "-j4",
            "-q",
            "--manifest-path",
            "dashboard/Cargo.toml",
        ])
        .status()?;
    if !built.success() {
        eprintln!("FAIL: the server did not build");
        return Ok(1);
    }
    let log = std::fs::File::create(tmp.path().join("server.log"))?;
    let _server = Server(
        Command::new(SERVER_BIN)
            .args([
                "--root",
                &root.to_string_lossy(),
                "--port",
                &port.to_string(),
            ])
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .spawn()?,
    );
    let start = Instant::now();
    while !healthy(port) {
        if start.elapsed() > Duration::from_secs(10) {
            let text = std::fs::read_to_string(tmp.path().join("server.log")).unwrap_or_default();
            eprintln!("FAIL: the server did not start. Its log:\n{text}");
            return Ok(1);
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let url = format!("http://127.0.0.1:{port}/");
    let shots: [(&str, u32, u32, &[&str]); 3] = [
        ("desktop-dark.png", 1440, 1500, &[]),
        ("phone-dark.png", 400, 2600, &[]),
        (
            "desktop-light.png",
            1440,
            1500,
            &["--blink-settings=preferredColorScheme=1"],
        ),
    ];
    let mut failed = false;
    for (name, w, h, extra) in shots {
        let file = out.join(name);
        let args = shot_args(browser, &file.to_string_lossy(), w, h, &url, extra);
        let _ = Command::new(browser)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if std::fs::metadata(&file)
            .map(|m| m.len() > 0)
            .unwrap_or(false)
        {
            println!("wrote {}", file.display());
        } else {
            eprintln!("FAIL: no screenshot {}", file.display());
            failed = true;
        }
    }
    let (dark, light) = (
        std::fs::read(out.join("desktop-dark.png")),
        std::fs::read(out.join("desktop-light.png")),
    );
    if let (Ok(d), Ok(l)) = (dark, light) {
        if d == l {
            println!("NOTE: the light screenshot equals the dark one, so the light theme was not exercised.");
        }
    }
    Ok(if failed { 1 } else { 0 })
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("FAIL: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_browser_takes_the_first_installed_in_order() {
        assert_eq!(
            pick_browser(|n| n == "firefox" || n == "chromium"),
            Some("chromium")
        );
        assert_eq!(pick_browser(|_| false), None);
    }

    #[test]
    fn chrome_args_set_size_and_screenshot_path() {
        let args = shot_args(
            "google-chrome",
            "/o/a.png",
            400,
            2600,
            "http://127.0.0.1:9/",
            &[],
        );
        assert!(args.contains(&"--headless=new".to_string()));
        assert!(args.contains(&"--window-size=400,2600".to_string()));
        assert!(args.contains(&"--screenshot=/o/a.png".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("http://127.0.0.1:9/"));
    }

    #[test]
    fn firefox_args_use_its_own_flags() {
        let args = shot_args(
            "firefox",
            "/o/a.png",
            1440,
            1500,
            "http://127.0.0.1:9/",
            &[],
        );
        assert_eq!(args[0], "--headless");
        assert!(args.contains(&"/o/a.png".to_string()));
        assert!(args.contains(&"1440,1500".to_string()));
    }

    #[test]
    fn blocked_message_names_the_browsers_and_the_manual_steps() {
        let text = blocked_message();
        assert!(text.starts_with("BLOCKED"));
        assert!(text.contains("google-chrome"));
        assert!(text.contains("400"));
    }

    #[test]
    fn a_free_port_can_be_bound_again_right_after() -> Result<(), Box<dyn std::error::Error>> {
        let port = free_port()?;
        let again = std::net::TcpListener::bind(("127.0.0.1", port));
        assert!(again.is_ok());
        Ok(())
    }
}
