//! Live event stream: one long-lived SSE connection per open tab, carrying only the
//! authenticated user's events. Used to close the loop on "Ingest now" - the browser learns when
//! *its* ingestion actually finished, which the `refresh-all` response cannot say (it returns
//! before the scheduler has polled anything).

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use futures_util::stream::Stream;
use serde_json::json;
use tokio::sync::broadcast::error::RecvError;

use crate::auth::extract::CurrentUser;
use crate::events::Events;
use crate::http::AppState;

/// Proxies (nginx et al.) buffer streaming responses by default, which would hold events back
/// until the connection closes. This is the documented opt-out.
const NO_BUFFER: (&str, &str) = ("X-Accel-Buffering", "no");

/// Frequent enough to survive the common 30-60s idle timeouts in front of a self-hosted app.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/events", get(stream))
        .route("/ingest/status", get(status))
}

/// `GET /api/events` - SSE stream of this user's events. Auth is the normal session cookie
/// (`EventSource` sends cookies same-origin). The browser reconnects on its own if it drops.
async fn stream(
    user: CurrentUser,
    State(state): State<AppState>,
) -> (
    [(&'static str, &'static str); 1],
    Sse<impl Stream<Item = Result<SseEvent, Infallible>>>,
) {
    let stream = user_stream(state.events, user.id);
    (
        [NO_BUFFER],
        Sse::new(stream).keep_alive(KeepAlive::new().interval(KEEP_ALIVE)),
    )
}

fn user_stream(events: Events, user_id: i64) -> impl Stream<Item = Result<SseEvent, Infallible>> {
    let rx = events.subscribe();
    futures_util::stream::unfold(rx, move |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(ev) if ev.user_id == user_id => {
                    let data = match serde_json::to_string(&ev.event) {
                        Ok(json) => json,
                        // Unreachable for our own event enum; dropping one event beats killing
                        // the whole stream.
                        Err(_) => continue,
                    };
                    return Some((Ok(SseEvent::default().data(data)), rx));
                }
                // Another user's event, or we fell behind and the channel dropped some. The
                // client's next `ingest_finished` (or its fallback timeout) resyncs it.
                Ok(_) | Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return None,
            }
        }
    })
}

/// `GET /api/ingest/status` - the caller's in-flight run and cooldown, so a reload (or a second
/// tab) restores the button/toast state instead of pretending nothing is running.
async fn status(user: CurrentUser, State(state): State<AppState>) -> impl IntoResponse {
    Json(json!({
        "run": state.events.run_for(user.id),
        "cooldown_secs": state.events.cooldown_left(user.id),
    }))
}
