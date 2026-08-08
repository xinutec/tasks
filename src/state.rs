//! Shared application state + the short-lived OAuth `state` store (in-memory,
//! per process — fine for a single-pod deployment). Copied from memview, which
//! took it from `messages`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand::RngCore;
use sqlx::MySqlPool;

use crate::config::Config;

const OAUTH_TTL: Duration = Duration::from_secs(600); // 10 minutes

pub struct PendingOauth {
    created: Instant,
    pub return_to: Option<String>,
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

    pub fn create_oauth_state(&self, return_to: Option<String>) -> String {
        let mut bytes = [0u8; 24];
        rand::rng().fill_bytes(&mut bytes);
        let state = hex::encode(bytes);
        let mut map = self.oauth.lock().expect("oauth map poisoned");
        map.retain(|_, v| v.created.elapsed() < OAUTH_TTL);
        map.insert(
            state.clone(),
            PendingOauth {
                created: Instant::now(),
                return_to,
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
