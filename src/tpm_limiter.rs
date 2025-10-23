use std::{
    collections::VecDeque,
    time::{Duration, SystemTime},
};

pub struct TPMLimiter {
    max_tpm: u32,
    min_interval: Duration,
    token_usage: VecDeque<(SystemTime, u32)>,
    last_request: Option<SystemTime>,
    total_tokens_used: u32,
}

impl TPMLimiter {
    pub fn new(max_tpm: u32, min_interval_secs: u64) -> Self {
        Self {
            max_tpm,
            min_interval: Duration::from_secs(min_interval_secs),
            token_usage: VecDeque::new(),
            last_request: None,
            total_tokens_used: 0,
        }
    }

    pub fn add_token_usage(&mut self, tokens: u32) {
        let now = SystemTime::now();
        self.token_usage.push_back((now, tokens));
        self.total_tokens_used += tokens;
        self.last_request = Some(now);

        // Clean up old entries (older than 1 minute)
        let one_minute_ago = now - Duration::from_secs(60);
        while let Some(front) = self.token_usage.front() {
            if front.0 < one_minute_ago {
                self.token_usage.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn wait_if_needed(&mut self) {
        if let Some(last_request) = self.last_request {
            if let Ok(elapsed) = last_request.elapsed() {
                if elapsed < self.min_interval {
                    let sleep_time = self.min_interval - elapsed;
                    std::thread::sleep(sleep_time);
                }
            }
        }

        let current_tpm = self.get_current_tpm();
        if current_tpm >= self.max_tpm {
            let wait_time = Duration::from_secs(60);
            std::thread::sleep(wait_time);
        }
    }

    pub fn get_current_tpm(&self) -> u32 {
        let now = SystemTime::now();
        let one_minute_ago = now - Duration::from_secs(60);

        self.token_usage
            .iter()
            .filter(|(time, _)| *time >= one_minute_ago)
            .map(|(_, tokens)| tokens)
            .sum()
    }

    pub fn get_total_tokens(&self) -> u32 {
        self.total_tokens_used
    }
}
