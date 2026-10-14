//! Headless agent runner for the software factory.

pub mod claude;
pub mod events;
pub mod run;
pub mod runner;
pub mod spec;
pub mod stream;
pub mod usage;

pub use claude::{AgentError, ClaudeRunner, ALLOW_API_KEY_VAR};
pub use runner::{AgentRunner, FakeRunner};
pub use events::{AgentEvent, EventKind, EventSink};
pub use run::{AgentRun, StopReason};
pub use spec::{AgentSpec, Caps};
pub use stream::{StreamEvent, StreamState};
pub use usage::Usage;
