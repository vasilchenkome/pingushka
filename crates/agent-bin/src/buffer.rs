use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use pingushka_protocol::ResultItem;
use uuid::Uuid;

pub struct ResultBuffer {
    path: Option<PathBuf>,
    capacity: usize,
    pending: VecDeque<ResultItem>,
    dropped: u32,
}

impl ResultBuffer {
    pub fn new(path: Option<PathBuf>, capacity: usize) -> Self {
        let capacity = capacity.max(1);
        let dropped = path.as_deref().map(read_dropped).unwrap_or(0);
        let mut pending = path
            .as_deref()
            .map(|path| read_pending(path, capacity))
            .unwrap_or_default();
        if pending.len() > capacity {
            let excess = pending.len() - capacity;
            pending.drain(..excess);
            tracing::warn!(excess, "trimmed pending results to buffer capacity");
        }
        Self {
            path,
            capacity,
            pending,
            dropped,
        }
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    pub fn push(&mut self, item: ResultItem) {
        if self.pending.len() >= self.capacity {
            self.pending.pop_front();
            self.dropped = self.dropped.saturating_add(1);
            self.persist_dropped();
        }
        self.pending.push_back(item.clone());
        self.append(&item);
    }

    pub fn batch(&self, limit: usize) -> Vec<ResultItem> {
        self.pending.iter().take(limit).cloned().collect()
    }

    pub fn ack(&mut self, up_to: Uuid) -> usize {
        let Some(position) = self.pending.iter().position(|item| item.result_id == up_to) else {
            return 0;
        };
        let acknowledged = position + 1;
        self.pending.drain(..acknowledged);
        self.dropped = 0;
        self.persist_dropped();
        self.rewrite();
        acknowledged
    }

    fn append(&self, item: &ResultItem) {
        let Some(path) = &self.path else {
            return;
        };
        let Ok(line) = serde_json::to_string(item) else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        match OpenOptions::new().create(true).append(true).open(path) {
            Ok(mut file) => {
                if let Err(error) = writeln!(file, "{line}") {
                    tracing::warn!(%error, "failed to append buffered result");
                }
            }
            Err(error) => tracing::warn!(%error, path = %path.display(), "cannot open buffer file"),
        }
    }

    fn rewrite(&self) {
        let Some(path) = &self.path else {
            return;
        };
        let mut payload = String::new();
        for item in &self.pending {
            if let Ok(line) = serde_json::to_string(item) {
                payload.push_str(&line);
                payload.push('\n');
            }
        }
        let temporary = path.with_extension("jsonl.tmp");
        if let Err(error) = fs::write(&temporary, payload) {
            tracing::warn!(%error, "cannot rewrite result buffer");
            return;
        }
        #[cfg(windows)]
        {
            if path.exists() {
                let _ = fs::remove_file(path);
            }
        }
        if let Err(error) = fs::rename(&temporary, path) {
            tracing::warn!(%error, "cannot replace result buffer");
        }
    }

    fn persist_dropped(&self) {
        let Some(path) = &self.path else {
            return;
        };
        let sidecar = dropped_path(path);
        if self.dropped == 0 {
            let _ = fs::remove_file(sidecar);
            return;
        }
        let _ = fs::write(sidecar, self.dropped.to_string());
    }
}

fn read_pending(path: &Path, capacity: usize) -> VecDeque<ResultItem> {
    let Ok(raw) = fs::read_to_string(path) else {
        return VecDeque::new();
    };
    let mut items: VecDeque<ResultItem> = raw
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| match serde_json::from_str(line) {
            Ok(item) => Some(item),
            Err(error) => {
                tracing::warn!(%error, "skipping malformed buffered result");
                None
            }
        })
        .collect();
    while items.len() > capacity {
        items.pop_front();
    }
    items
}

fn read_dropped(path: &Path) -> u32 {
    fs::read_to_string(dropped_path(path))
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0)
}

fn dropped_path(path: &Path) -> PathBuf {
    path.with_extension("dropped")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pingushka_protocol::ProbeMode;

    fn item(seconds: i64) -> ResultItem {
        ResultItem {
            result_id: Uuid::new_v4(),
            target_id: Uuid::new_v4(),
            checked_at_unix: seconds,
            ok: true,
            latency_ms: 10,
            status_code: Some(200),
            mode: ProbeMode::Http,
            error_kind: None,
            error_message: None,
        }
    }

    #[test]
    fn overflow_drops_oldest_and_counts() {
        let mut buffer = ResultBuffer::new(None, 2);
        let first = item(1);
        let second = item(2);
        let third = item(3);
        buffer.push(first);
        buffer.push(second);
        buffer.push(third);
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.dropped(), 1);
        let batch = buffer.batch(10);
        assert_eq!(batch[0].checked_at_unix, 2);
        assert_eq!(batch[1].checked_at_unix, 3);
    }

    #[test]
    fn ack_removes_results_up_to_id() {
        let mut buffer = ResultBuffer::new(None, 10);
        let first = item(1);
        let second = item(2);
        let third = item(3);
        buffer.push(first.clone());
        buffer.push(second.clone());
        buffer.push(third.clone());
        assert_eq!(buffer.ack(second.result_id), 2);
        let batch = buffer.batch(10);
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].result_id, third.result_id);
        assert_eq!(buffer.ack(third.result_id), 1);
        assert!(buffer.is_empty());
    }

    #[test]
    fn buffer_survives_restart_and_ack_compacts_file() {
        let directory = std::env::temp_dir().join(format!("pingushka-buffer-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("temp dir");
        let path = directory.join("buffer.jsonl");
        let first = item(1);
        let second = item(2);
        {
            let mut buffer = ResultBuffer::new(Some(path.clone()), 10);
            buffer.push(first.clone());
            buffer.push(second.clone());
            buffer.ack(first.result_id);
        }
        let mut restored = ResultBuffer::new(Some(path.clone()), 10);
        assert_eq!(restored.len(), 1);
        assert_eq!(restored.batch(10)[0].result_id, second.result_id);
        restored.ack(second.result_id);
        drop(restored);
        let empty = ResultBuffer::new(Some(path), 10);
        assert!(empty.is_empty());
        let _ = fs::remove_dir_all(directory);
    }
}
