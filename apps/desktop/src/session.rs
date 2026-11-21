// SPDX-License-Identifier: AGPL-3.0-only
//! Sign-in and upload logic shared by the window and the command line.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use opnrelay_capture::StepDraft;
use opnrelay_client::{Client, ClientError, SignIn, Tokens};

use crate::store;

pub type Shared = Arc<Mutex<Client>>;

fn persist(server: String) -> Box<dyn Fn(&Tokens) + Send> {
    Box::new(move |t: &Tokens| {
        store::save_refresh(&server, &t.refresh_token);
    })
}

pub fn device_name() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
        })
        .filter(|h| !h.is_empty());
    let os = match std::env::consts::OS {
        "macos" => "Mac",
        "windows" => "Windows",
        _ => "Linux",
    };
    match host {
        Some(h) => format!("Desktop recorder on {h} ({os})"),
        None => format!("Desktop recorder ({os})"),
    }
}

/// Resume a previous sign-in, if a refresh token is stored.
pub fn resume(server: &str) -> Option<Shared> {
    let token = store::load_refresh(server)?;
    match Client::from_refresh(server, &token, persist(server.to_string())) {
        Ok(c) => Some(Arc::new(Mutex::new(c))),
        Err(_) => {
            store::clear_refresh(server);
            None
        }
    }
}

/// Begin a browser sign-in: returns the URL to open and a function that
/// waits for completion.
pub fn begin_sign_in(
    server: &str,
) -> Result<
    (
        String,
        impl FnOnce() -> Result<Shared, ClientError> + Send + use<>,
    ),
    ClientError,
> {
    let s = SignIn::start(server, &device_name())?;
    let url = s.url.clone();
    let server = server.to_string();
    Ok((url, move || {
        let tokens = s.finish(&server, Duration::from_secs(300))?;
        store::save_refresh(&server, &tokens.refresh_token);
        Ok(Arc::new(Mutex::new(Client::new(
            &server,
            tokens,
            persist(server.clone()),
        ))))
    }))
}

/// Upload recorded steps as a new draft guide. Returns the editor URL.
pub fn upload(
    client: &Shared,
    workspace: &str,
    title: &str,
    steps: &[StepDraft],
    progress: impl Fn(usize, usize),
) -> Result<String, ClientError> {
    let mut c = client.lock().expect("client lock");
    let guide = c.create_guide(workspace, title)?;
    let mut out = Vec::with_capacity(steps.len());
    for (i, s) in steps.iter().enumerate() {
        progress(i, steps.len());
        let media = c.upload_png(&guide, &s.png)?;
        let media_id = uuid::Uuid::parse_str(&media).ok();
        out.push(s.to_step(media_id));
    }
    c.put_steps(&guide, &out)?;
    progress(steps.len(), steps.len());
    Ok(c.guide_edit_url(&guide))
}
