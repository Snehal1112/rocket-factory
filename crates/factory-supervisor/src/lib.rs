//! The supervisor library: requests, the queue, runtime records and the scheduler.
pub mod api;
pub mod daily;
pub mod factory_cli;
pub mod lock;
pub mod procinfo;
pub mod queue;
pub mod records;
pub mod request;
pub mod scheduler;
pub mod spawn;
pub mod supervisor;
pub mod token;
pub mod version_check;

pub use factory_cli::{CliOutput, CliSpawnError, FactoryCli, ProcessFactoryCli};
pub use lock::{InstanceLock, LockError};
pub use queue::{QueueError, QueueItem};
pub use records::{ChildRecord, Outcome, RecordError, ResultRecord};
pub use request::{Action, Request, RequestError};
pub use scheduler::{decide, Decision, ProjectState, SchedulerInput};
pub use spawn::{ChildPoll, ProcessSpawner, SpawnError, Spawner};
pub use supervisor::{pid_alive, RecoveryReport, Supervisor, SupervisorError, TickReport};
