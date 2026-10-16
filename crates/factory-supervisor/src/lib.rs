//! The supervisor library: requests, the queue, runtime records and the scheduler.
pub mod queue;
pub mod records;
pub mod request;
pub mod scheduler;

pub use queue::{QueueError, QueueItem};
pub use records::{ChildRecord, Outcome, RecordError, ResultRecord};
pub use request::{Action, Request, RequestError};
pub use scheduler::{decide, Decision, ProjectState, SchedulerInput};
