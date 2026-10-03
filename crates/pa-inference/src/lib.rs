//! Ausschließlich lokale llama.cpp-Prozesssteuerung und Streaming.

pub mod adapter;
pub mod chat;
pub mod config;
pub mod embeddings;
pub mod error;
pub mod gemma4;
pub mod llama32;
pub mod loopback;
pub mod ministral3;
pub mod multipart;
pub mod process;
pub mod qwen3;
pub mod sse;
pub mod supervisor;

pub use error::InferenceError;
