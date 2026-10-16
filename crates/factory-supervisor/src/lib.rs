//! The supervisor library: requests, the queue, runtime records and the scheduler.
pub mod lock;
pub mod queue;
pub mod records;
pub mod request;
pub mod scheduler;
pub mod spawn;

pub use queue::{QueueError, QueueItem};
pub use records::{ChildRecord, Outcome, RecordError, ResultRecord};
pub use request::{Action, Request, RequestError};
pub use scheduler::{decide, Decision, ProjectState, SchedulerInput};
pub use lock::{InstanceLock, LockError};
pub use spawn::{ChildPoll, ProcessSpawner, SpawnError, Spawner};
