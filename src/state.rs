//! Shared application state, and the short-lived OAuth `state` store —
//! in-memory, per process, which is fine for a single-pod deployment.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand::Rng;
use sqlx::MySqlPool;

use crate::config::Config;

/// How long a sign-in stays valid: one number for the entry and its cookie, so the cookie can't outlive it.
pub const OAUTH_TTL_SECS: i64 = 600; // 10 minutes

const OAUTH_TTL: Duration = Duration::from_secs(OAUTH_TTL_SECS as u64);

pub struct PendingOauth {
    created: Instant,
    pub return_to: Option<String>,
    /// The state this sign-in was issued under, compared with the callback's.
    pub state: String,
    /// Already sent back once for a missing `state`; a second miss is refused.
    pub retried: bool,
}

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub db: MySqlPool,
    pub http: reqwest::Client,
    oauth: Arc<Mutex<HashMap<String, PendingOauth>>>,
}

impl AppState {
    pub fn new(cfg: Config, db: MySqlPool, http: reqwest::Client) -> Self {
        Self {
            cfg: Arc::new(cfg),
            db,
            http,
            oauth: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn create_oauth_state(&self, return_to: Option<String>, retried: bool) -> String {
        let mut bytes = [0u8; 24];
        rand::rng().fill_bytes(&mut bytes);
        let state = hex::encode(bytes);
        let mut map = self.oauth.lock().expect("oauth map poisoned");
        map.retain(|_, v| v.created.elapsed() < OAUTH_TTL);
        map.insert(
            state.clone(),
            PendingOauth {
                created: Instant::now(),
                state: state.clone(),
                return_to,
                retried,
            },
        );
        state
    }

    pub fn consume_oauth_state(&self, state: &str) -> Option<PendingOauth> {
        let mut map = self.oauth.lock().expect("oauth map poisoned");
        let entry = map.remove(state)?;
        if entry.created.elapsed() > OAUTH_TTL {
            return None;
        }
        Some(entry)
    }
}
