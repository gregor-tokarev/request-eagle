use crate::workspace::Layout;
use gpui_kit::{App, Entity, Task, Window};
use request_eagle_automation::{
    Call, Command, Connection, MAX_MESSAGE_BYTES, PROTOCOL_VERSION, failure, prepare_directory,
    socket_directory, success,
};
use serde_json::{Value, json};
use settings_ui::CliAccess;
use smol::{
    channel,
    future::FutureExt,
    net::unix::{UnixListener, UnixStream},
};
use std::{io, os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};

type Pending = (Call, String, channel::Sender<Value>);

// The owning task holds the listener and removes only its own socket on drop.
struct Endpoint {
    listener: UnixListener,
    path: PathBuf,
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(crate) fn start(
    layout: &Entity<Layout>,
    window: &mut Window,
    cx: &mut App,
) -> Result<Task<()>, String> {
    let token = cx
        .global::<CliAccess>()
        .token
        .clone()
        .ok_or("CLI access is disabled")?;
    let result = (|| -> io::Result<Endpoint> {
        let directory = socket_directory()?;
        prepare_directory(&directory)?;
        let path = directory.join(format!("{}.sock", std::process::id()));
        // A stale PID socket must not prevent a restarted instance. Never unlink
        // a live listener or a different filesystem object.
        if path.exists() {
            request_eagle_automation::validate_socket(&path)?;
            if std::os::unix::net::UnixStream::connect(&path).is_ok() {
                return Err(io::Error::other("Automation socket is already in use"));
            }
            std::fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Endpoint { listener, path })
    })();
    let endpoint = result.map_err(|error| format!("Could not start CLI connection: {error}"))?;
    let (send, receive) = channel::bounded::<Pending>(16);
    let server = cx.background_executor().spawn(async move {
        // A fixed worker pool bounds connections, buffers and queued commands.
        let (connections, incoming) = channel::bounded(16);
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let incoming = incoming.clone();
                let send = send.clone();
                let token = token.clone();
                smol::spawn(async move {
                    while let Ok(stream) = incoming.recv().await {
                        let _ = serve(stream, &send, &token)
                            .or(async {
                                smol::Timer::after(Duration::from_secs(30)).await;
                                Err(io::Error::new(
                                    io::ErrorKind::TimedOut,
                                    "CLI connection timed out",
                                ))
                            })
                            .await;
                    }
                })
            })
            .collect();
        while let Ok((stream, _)) = endpoint.listener.accept().await {
            // Saturation closes the new connection instead of growing work.
            let _ = connections.try_send(stream);
        }
        drop(workers);
    });
    let window = window.window_handle();
    let weak = layout.downgrade();
    let task = cx.spawn(async move |cx| {
        let _server = server;
        while let Ok((call, token, reply)) = receive.recv().await {
            if reply.is_closed() {
                continue;
            }
            let authorized = cx.update(|cx| cx.global::<CliAccess>().authorizes(&token));
            let response = if !authorized {
                failure(
                    "unauthorized",
                    "Enable CLI access in General settings and copy the session command",
                )
            } else if call.version != PROTOCOL_VERSION {
                failure(
                    "protocol_mismatch",
                    "Install the CLI matching this application",
                )
            } else if matches!(call.command, Command::SettingsProxy { .. }) {
                match cx.update(|cx| super::settings::proxy(call.command, cx)) {
                    Ok(task) => match task.await {
                        Ok(()) => {
                            let _ = window.update(cx, |_, window, cx| {
                                weak.update(cx, |layout, cx| {
                                    layout.settings.update(cx, |settings, cx| {
                                        settings.refresh_preferences(
                                            settings_ui::SettingsPage::Proxy,
                                            window,
                                            cx,
                                        )
                                    });
                                })
                            });
                            success(json!({"saved": true}))
                        }
                        Err(error) => failure("operation_failed", error),
                    },
                    Err(error) => failure("invalid_input", error),
                }
            } else {
                match window.update(cx, |_, window, cx| {
                    weak.update(cx, |layout, cx| {
                        layout.automation_command(call.command, window, cx)
                    })
                }) {
                    Ok(Ok(Ok(value))) => success(value),
                    Ok(Ok(Err(error))) => failure("operation_failed", error),
                    _ => failure("app_closed", "The application window closed"),
                }
            };
            let _ = reply.try_send(response);
        }
    });
    Ok(task)
}

async fn serve(stream: UnixStream, send: &channel::Sender<Pending>, token: &str) -> io::Result<()> {
    let mut connection = Connection::server(stream, token).await?;
    let input = connection.receive().await?;
    let response = match serde_json::from_slice::<Call>(&input) {
        Err(error) => failure("invalid_input", error),
        Ok(call) => {
            let (reply, result) = channel::bounded(1);
            send.send((call, token.to_owned(), reply))
                .await
                .map_err(io::Error::other)?;
            result.recv().await.map_err(io::Error::other)?
        }
    };
    let mut output = serde_json::to_vec(&response)?;
    if output.len() as u64 >= MAX_MESSAGE_BYTES {
        output = serde_json::to_vec(&failure(
            "result_too_large",
            "Use a narrower query or smaller body chunk",
        ))?;
    }
    connection.send(&output).await
}
