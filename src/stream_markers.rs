use {
    livesplit_core::{Timer, TimerPhase},
    tokio::runtime::{current_thread, Runtime},
    twitch_stream_markers::{futures::Future, Client as MarkerClient},
};

pub struct Client {
    client: Option<MarkerClient>,
    is_running: Option<String>,
    runtime: Runtime,
}

impl Client {
    pub fn new(token: Option<&str>) -> Self {
        if let Some(token) = token {
            if let Ok(mut rt) = current_thread::Runtime::new() {
                return Self {
                    client: rt.block_on(MarkerClient::new(token)).ok(),
                    is_running: None,
                    runtime: Runtime::new().unwrap(),
                };
            }
        }
        Self {
            client: None,
            is_running: None,
            runtime: Runtime::new().unwrap(),
        }
    }

    pub fn tick(&mut self, timer: &Timer) {
        if let Some(client) = &self.client {
            let is_running = timer.current_phase() != TimerPhase::NotRunning;
            if !is_running {
                if let Some(description) = self.is_running.take() {
                    self.runtime.spawn(
                        client
                            .create_marker(Some(&format!("End of {}", description)))
                            .map(drop)
                            .map_err(drop),
                    );
                }
            } else if self.is_running.is_none() {
                let description = format!(
                    "attempt {} in {}",
                    timer.run().attempt_count(),
                    timer.run().extended_name(false)
                );
                self.runtime.spawn(
                    client
                        .create_marker(Some(&format!("Start of {}", description)))
                        .map(drop)
                        .map_err(drop),
                );
                self.is_running = Some(description);
            }
        }
    }
}
