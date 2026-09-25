use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use owo_credentials::CredentialStore;
use owo_gateway::{AppState, GatewayConfig};

use crate::cli::GlobalArgs;
use crate::context;

/// How long `owo history` and `owo usage` can look back.
const CALL_RETENTION: Duration = Duration::from_secs(400 * 24 * 3600);

pub fn run(global: &GlobalArgs, listen: Option<String>) -> Result<()> {
    if std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        crate::banner::print();
    }
    let paths = context::paths(global)?;
    let (mut config, warnings) = context::load_config(&paths)?;
    context::print_diagnostics(&warnings);
    if let Some(listen) = listen {
        config.server.listen = listen;
        // Re-check the loopback/auth rule for the overridden address.
        let errors: Vec<_> = config
            .validate()
            .into_iter()
            .filter(|d| d.severity == owo_config::Severity::Error)
            .collect();
        if !errors.is_empty() {
            context::print_diagnostics(&errors);
            bail!("invalid --listen");
        }
    }
    paths.ensure_dirs().with_context(|| format!("cannot create state directories under {}", paths.root.display()))?;

    let (router, registry_warnings) = context::build_router(&config)?;
    context::print_diagnostics(&registry_warnings);
    let (recorder, calls) = owo_usage::channel();
    let router = router.with_sink(recorder);

    let listen: SocketAddr = config.server.listen.parse().context("invalid server.listen")?;
    let auth_token = match &config.server.auth_token {
        Some(reference) => Some(
            CredentialStore::new(config.credentials.backend)
                .resolve(reference)
                .context("cannot resolve server.auth_token")?
                .context("server.auth_token must not be `none`")?,
        ),
        None => None,
    };

    let state = Arc::new(AppState {
        router: Arc::new(router),
        config: GatewayConfig {
            listen,
            auth_token,
            max_body_bytes: config.server.max_body_bytes,
            request_timeout: Duration::from_secs(config.server.request_timeout_secs),
            max_concurrent_requests: config.server.max_concurrent_requests,
            control_api: config.server.control_api,
        },
        started: Instant::now(),
        codex_template: owo_client_codex::stored_template(&paths.state),
        codex_native_aliases: owo_client_codex::stored_native_aliases(&paths.state),
        shutdown: Arc::default(),
    });
    let _pid = PidFile::create(&paths, &crate::cmd_client::client_address(&listen.to_string()))?;

    let available = state.router.registry().available_models().count();
    tracing::info!(config = %paths.config.display(), models = available, "starting OwO AI Gateway {}", env!("CARGO_PKG_VERSION"));
    if available == 0 {
        tracing::warn!("no models are available; add [[models]] entries to config.toml");
    }

    let cursor_enabled = crate::cmd_cursor::connected(&paths);
    let cursor_name = config.client_name(crate::cmd_cursor::CLIENT_ID).to_string();
    context::runtime()?.block_on(async move {
        let usage_path = paths.state.join(owo_usage::FILE_NAME);
        match owo_usage::UsageLog::open(&usage_path).await {
            Ok(log) => {
                match log.prune(CALL_RETENTION).await {
                    Ok(0) => {}
                    Ok(removed) => tracing::info!(removed, "removed call records older than 400 days"),
                    Err(error) => tracing::warn!(%error, "could not prune old call records"),
                }
                tokio::spawn(owo_usage::write_all(log, calls));
            }
            Err(error) => {
                tracing::warn!(%error, path = %usage_path.display(), "call history is unavailable; calls are not recorded");
            }
        }

        let shutdown = tokio_util::sync::CancellationToken::new();
        let cursor = if cursor_enabled {
            match crate::cmd_cursor::start(&paths, state.router.clone(), &cursor_name, shutdown.clone()).await {
                Ok(task) => Some(task),
                Err(error) => {
                    tracing::error!("Cursor integration failed to start: {error:#}");
                    None
                }
            }
        } else {
            None
        };

        let signal = shutdown.clone();
        let remote_stop = state.shutdown.clone();
        let served = owo_gateway::serve(state, async move {
            tokio::select! {
                () = stop_requested() => {}
                () = signal.cancelled() => {}
                () = remote_stop.notified() => {}
            }
            tracing::info!("shutting down");
        })
        .await
        .with_context(|| format!("failed to serve on {listen}"));

        // Stops the Cursor backend, which removes its proxy from Cursor's settings.
        shutdown.cancel();
        if let Some(task) = cursor {
            let _ = task.await;
        }
        served
    })
}

/// Resolves when the process is asked to stop, so shutdown cleanup (such as removing the
/// Cursor proxy from Cursor's settings) also runs under service managers and when the
/// console window closes.
async fn stop_requested() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let (Ok(mut term), Ok(mut hup)) = (signal(SignalKind::terminate()), signal(SignalKind::hangup())) else {
            let _ = tokio::signal::ctrl_c().await;
            return;
        };
        // A background gateway outlives the terminal that started it.
        let detached = std::env::var_os(DETACHED_ENV).is_some();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
            _ = hup.recv(), if !detached => {}
        }
    }
    #[cfg(windows)]
    {
        use tokio::signal::windows::{ctrl_break, ctrl_close, ctrl_logoff, ctrl_shutdown};
        let (Ok(mut brk), Ok(mut close), Ok(mut logoff), Ok(mut shutdown)) = (ctrl_break(), ctrl_close(), ctrl_logoff(), ctrl_shutdown()) else {
            let _ = tokio::signal::ctrl_c().await;
            return;
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = brk.recv() => {}
            _ = close.recv() => {}
            _ = logoff.recv() => {}
            _ = shutdown.recv() => {}
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// Set for the gateway process `owo start -d` spawns.
const DETACHED_ENV: &str = "OWO_DETACHED";

fn pid_path(paths: &owo_config::OwoPaths) -> std::path::PathBuf {
    paths.state.join("owo.pid")
}

/// `state/owo.pid` while a gateway runs: its pid and the address clients dial, so
/// `owo stop` finds a gateway started with `--listen`, and can fall back to the pid when
/// the control API is off.
struct PidFile(std::path::PathBuf);

impl PidFile {
    fn create(paths: &owo_config::OwoPaths, address: &str) -> Result<Self> {
        let path = pid_path(paths);
        std::fs::write(&path, format!("{}\n{address}\n", std::process::id())).with_context(|| format!("cannot write {}", path.display()))?;
        Ok(Self(path))
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        if read_pid_file(&self.0).is_some_and(|(pid, _)| pid == std::process::id()) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

/// Where the gateway answers: the address a running `owo start` recorded, else `server.listen`.
pub fn gateway_address(paths: &owo_config::OwoPaths, config: &owo_config::Config) -> String {
    read_pid_file(&pid_path(paths))
        .and_then(|(_, a)| a)
        .filter(|a| crate::cmd_client::gateway_running(a))
        .unwrap_or_else(|| crate::cmd_client::client_address(&config.server.listen))
}

fn read_pid_file(path: &std::path::Path) -> Option<(u32, Option<String>)> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    let pid = lines.next()?.trim().parse().ok()?;
    Some((pid, lines.next().map(|a| a.trim().to_string()).filter(|a| !a.is_empty())))
}

/// `owo start -d`: runs `owo start` as a background process and waits until it answers.
pub fn start_detached(global: &GlobalArgs, listen: Option<String>) -> Result<()> {
    let paths = context::paths(global)?;
    let (config, warnings) = context::load_config(&paths)?;
    context::print_diagnostics(&warnings);
    let address = crate::cmd_client::client_address(listen.as_deref().unwrap_or(&config.server.listen));
    if crate::cmd_client::gateway_running(&address) {
        println!("OwO AI Gateway is already running at http://{address}");
        return Ok(());
    }
    paths.ensure_dirs()?;
    let log_path = paths.logs.join("owo.log");
    let log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path).with_context(|| format!("cannot open {}", log_path.display()))?;

    let mut cmd = std::process::Command::new(std::env::current_exe().context("cannot locate the owo executable")?);
    if global.portable {
        cmd.arg("--portable");
    }
    if let Some(c) = &global.config {
        cmd.arg("--config").arg(c);
    }
    cmd.arg("start");
    if let Some(l) = &listen {
        cmd.arg("--listen").arg(l);
    }
    cmd.env(DETACHED_ENV, "1")
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
        // Windows children inherit every inheritable handle, including the pipe this process
        // writes to; a background gateway holding it would keep `owo start -d | …` or
        // `$(owo start -d)` waiting for output that never ends.
        stop_std_handle_inheritance();
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().context("cannot start the gateway in the background")?;

    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait()? {
            bail!("the gateway exited during startup ({status}); see {}", log_path.display());
        }
        if crate::cmd_client::gateway_running(&address) {
            println!("OwO AI Gateway is running in the background at http://{address} (pid {})", child.id());
            println!("logs:  {}", log_path.display());
            println!("stop:  owo stop");
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    bail!("the gateway did not start answering within 15 s; see {}", log_path.display())
}

#[cfg(windows)]
fn stop_std_handle_inheritance() {
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: querying this process's own standard handles and clearing a flag on them.
        unsafe {
            let h = GetStdHandle(which);
            if !h.is_null() && h != INVALID_HANDLE_VALUE {
                SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

/// `owo stop`: asks the gateway to shut down (it cleans up as on Ctrl+C) and waits.
pub fn stop(global: &GlobalArgs) -> Result<()> {
    let paths = context::paths(global)?;
    let (config, _) = context::load_config(&paths)?;
    let recorded = read_pid_file(&pid_path(&paths));
    let address = recorded
        .as_ref()
        .and_then(|(_, a)| a.clone())
        .unwrap_or_else(|| crate::cmd_client::client_address(&config.server.listen));
    if !crate::cmd_client::gateway_running(&address) {
        let _ = std::fs::remove_file(pid_path(&paths));
        println!("OwO AI Gateway is not running.");
        return Ok(());
    }
    let token = crate::cmd_claude::gateway_token_opt(&config)?;
    let url = format!("http://{address}/control/v1/shutdown");
    let asked = context::runtime()?.block_on(async {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(5)).build()?;
        let mut req = client.post(&url);
        if let Some(t) = &token {
            req = req.bearer_auth(t);
        }
        anyhow::Ok(req.send().await?.status().is_success())
    });
    if !matches!(asked, Ok(true)) {
        // Control API disabled or unreachable: stop the recorded process instead.
        let Some((pid, _)) = recorded else {
            bail!("the gateway at {address} did not accept the stop request and left no pid file; stop it where it runs (Ctrl+C)");
        };
        terminate(pid)?;
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if !crate::cmd_client::gateway_running(&address) {
            println!("OwO AI Gateway stopped.");
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    bail!("the gateway at {address} is still running after 15 s")
}

/// Asks process `pid` to terminate: SIGTERM on Unix (a clean shutdown), `taskkill` on Windows.
fn terminate(pid: u32) -> Result<()> {
    let pid = pid.to_string();
    let status = if cfg!(windows) {
        std::process::Command::new("taskkill").args(["/PID", &pid, "/F"]).status()
    } else {
        std::process::Command::new("kill").args(["-TERM", &pid]).status()
    }
    .context("cannot signal the gateway process")?;
    if !status.success() {
        bail!("could not stop process {pid}");
    }
    Ok(())
}
