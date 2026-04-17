use crate::embedding_store::EmbeddingChunk;
use std::collections::{HashMap, VecDeque};

pub type ReusableEmbeddingKey = (String, String, String, String, String, String);

pub fn reusable_embedding_key(
    file_path: &str,
    symbol_name: &str,
    kind: &str,
    signature: &str,
    name_path: &str,
    text: &str,
) -> ReusableEmbeddingKey {
    (
        file_path.to_owned(),
        symbol_name.to_owned(),
        kind.to_owned(),
        signature.to_owned(),
        name_path.to_owned(),
        text.to_owned(),
    )
}

pub fn reusable_embedding_key_for_chunk(chunk: &EmbeddingChunk) -> ReusableEmbeddingKey {
    reusable_embedding_key(
        &chunk.file_path,
        &chunk.symbol_name,
        &chunk.kind,
        &chunk.signature,
        &chunk.name_path,
        &chunk.text,
    )
}

pub fn reusable_embedding_key_for_symbol(
    sym: &crate::db::SymbolWithFile,
    text: &str,
) -> ReusableEmbeddingKey {
    reusable_embedding_key(
        &sym.file_path,
        &sym.name,
        &sym.kind,
        &sym.signature,
        &sym.name_path,
        text,
    )
}

pub struct TextEmbeddingCache {
    pub capacity: usize,
    pub order: VecDeque<String>,
    pub entries: HashMap<String, Vec<f32>>,
}

impl TextEmbeddingCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            order: VecDeque::new(),
            entries: HashMap::new(),
        }
    }

    pub fn get(&mut self, key: &str) -> Option<Vec<f32>> {
        let value = self.entries.get(key)?.clone();
        self.touch(key);
        Some(value)
    }

    pub fn insert(&mut self, key: String, value: Vec<f32>) {
        if self.capacity == 0 {
            return;
        }

        self.entries.insert(key.clone(), value);
        self.touch(&key);

        while self.entries.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            } else {
                break;
            }
        }
    }

    fn touch(&mut self, key: &str) {
        if let Some(position) = self.order.iter().position(|existing| existing == key) {
            self.order.remove(position);
        }
        self.order.push_back(key.to_owned());
    }
}
