use std::collections::VecDeque;
use std::thread;
use std::time::{Duration, SystemTime};

pub struct TPMLimiter {
    max_tpm: u32,
    min_interval: Duration,
    token_usage: VecDeque<(SystemTime, u32)>,
    last_request: Option<SystemTime>,
}

impl TPMLimiter {
    pub fn new(max_tpm: u32, min_interval_secs: u64) -> Self {
        Self {
            max_tpm,
            min_interval: Duration::from_secs(min_interval_secs),
            token_usage: VecDeque::new(),
            last_request: None,
        }
    }
    
    pub fn add_token_usage(&mut self, tokens: u32) {
        let now = SystemTime::now();
        self.token_usage.push_back((now, tokens));
        
        let one_minute_ago = now - Duration::from_secs(60);
        while let Some((time, _)) = self.token_usage.front() {
            if *time < one_minute_ago {
                self.token_usage.pop_front();
            } else {
                break;
            }
        }
    }
    
    pub fn wait_if_needed(&mut self) {
        let now = SystemTime::now();
        
        if let Some(last_req) = self.last_request {
            if let Ok(elapsed) = last_req.elapsed() {
                if elapsed < self.min_interval {
                    thread::sleep(self.min_interval - elapsed);
                }
            }
        }
        
        let current_tpm = self.get_current_tpm();
        
        if current_tpm >= self.max_tpm {
            if let Some((oldest_time, _)) = self.token_usage.front() {
                if let Ok(elapsed) = oldest_time.elapsed() {
                    if elapsed < Duration::from_secs(60) {
                        let wait_time = Duration::from_secs(60) - elapsed + Duration::from_millis(100);
                        thread::sleep(wait_time);
                    }
                }
            }
        }
        
        self.last_request = Some(now);
    }
    
    fn get_current_tpm(&self) -> u32 {
        let now = SystemTime::now();
        let one_minute_ago = now - Duration::from_secs(60);
        
        self.token_usage
            .iter()
            .filter(|(time, _)| *time >= one_minute_ago)
            .map(|(_, tokens)| tokens)
            .sum()
    }
}
