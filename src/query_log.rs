use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
    sync::Mutex,
    time::Duration,
};

use chrono::{DateTime, Utc};

const MAX_AGE: Duration = Duration::from_secs(600); // 10 minutes
const MAX_PER_IP: usize = 1000;

#[derive(serde::Serialize, Debug, Clone)]
pub struct QueryLog {
    pub query_time: DateTime<Utc>,
    pub question: String,
    pub answer: String,
}

#[derive(Debug, Default)]
pub struct QueryLogStore {
    logs: Mutex<HashMap<IpAddr, VecDeque<QueryLog>>>,
}

impl QueryLogStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, ip: IpAddr, log: QueryLog) {
        let mut store = self.logs.lock().unwrap();
        let entries = store.entry(ip).or_default();

        // Evict expired entries
        let cutoff = Utc::now() - MAX_AGE;
        while entries.front().is_some_and(|e| e.query_time < cutoff) {
            entries.pop_front();
        }

        // Cap per-IP entries
        if entries.len() >= MAX_PER_IP {
            entries.pop_front();
        }

        entries.push_back(log);
    }

    pub fn get_logs(&self, ip: &IpAddr) -> Vec<QueryLog> {
        let store = self.logs.lock().unwrap();
        let cutoff = Utc::now() - MAX_AGE;

        store
            .get(ip)
            .map(|entries| {
                entries
                    .iter()
                    .filter(|e| e.query_time >= cutoff)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn active_ips(&self) -> usize {
        let store = self.logs.lock().unwrap();
        let cutoff = Utc::now() - MAX_AGE;

        store
            .values()
            .filter(|entries| entries.back().is_some_and(|e| e.query_time >= cutoff))
            .count()
    }
}
