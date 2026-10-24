//! Headless agent runner for the software factory.

pub mod child;
pub mod claude;
pub mod codex;
pub mod events;
pub mod run;
pub mod runner;
pub mod spec;
pub mod stream;
pub mod usage;

pub use child::AgentStream;
pub use claude::{AgentError, ApiKeySource, ClaudeRunner, ALLOW_API_KEY_VAR};
pub use codex::{codex_args, CodexRunner, CodexStream};
pub use events::{AgentEvent, EventKind, EventSink};
pub use run::{AgentRun, StopReason};
pub use runner::{AgentRunner, FakeRunner};
pub use spec::{AgentSpec, Caps, SessionHook};
pub use stream::{StreamEvent, StreamState};
pub use usage::Usage;
