//! One-lane request scheduler (ADR-012, INFERENCE-CONTRACT §5).
//!
//! llama-server runs with `--parallel 1`, so requests are serialized by a single worker task.
//! Interactive (chat) jobs always run before queued Background (analyzer, pipelines) jobs; a job that
//! has started is never preempted. The background queue is bounded at 8.

use std::pin::Pin;

use futures_util::stream::{self, Stream, StreamExt};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::client::LlmClient;
use crate::error::LlmError;
use crate::supervisor::Supervisor;
use crate::types::{ChatRequest, StreamEvent};

/// Capacity of the background queue.
pub const BACKGROUND_QUEUE: usize = 8;
/// Capacity of the interactive queue (a UI never gets near this).
pub const INTERACTIVE_QUEUE: usize = 32;

/// Who is asking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    Interactive,
    Background,
}

/// A stream of events for one streaming job.
pub type JobStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, LlmError>> + Send>>;

#[derive(Clone)]
enum Source {
    Fixed(LlmClient),
    Supervisor(Supervisor),
}

impl Source {
    async fn client(&self) -> Option<LlmClient> {
        match self {
            Source::Fixed(c) => Some(c.clone()),
            Source::Supervisor(s) => s.client().await,
        }
    }
}

enum Reply {
    Complete(oneshot::Sender<Result<String, LlmError>>),
    Stream(mpsc::Sender<Result<StreamEvent, LlmError>>),
}

struct Job {
    req: ChatRequest,
    cancel: CancellationToken,
    reply: Reply,
}

/// Cheap-to-clone handle to the scheduler. The worker stops when every handle is dropped.
#[derive(Clone)]
pub struct Scheduler {
    source: Source,
    interactive: mpsc::Sender<Job>,
    background: mpsc::Sender<Job>,
}

impl Scheduler {
    /// Schedule against whatever client `supervisor` currently has (none while not `Ready`).
    pub fn new(supervisor: Supervisor) -> Self {
        Self::build(Source::Supervisor(supervisor))
    }

    /// Schedule against a fixed client (tests, examples).
    pub fn with_client(client: LlmClient) -> Self {
        Self::build(Source::Fixed(client))
    }

    fn build(source: Source) -> Self {
        let (itx, irx) = mpsc::channel(INTERACTIVE_QUEUE);
        let (btx, brx) = mpsc::channel(BACKGROUND_QUEUE);
        tokio::spawn(worker(source.clone(), irx, brx));
        Self {
            source,
            interactive: itx,
            background: btx,
        }
    }

    async fn ensure_ready(&self) -> Result<(), LlmError> {
        self.source
            .client()
            .await
            .map(|_| ())
            .ok_or(LlmError::RuntimeUnavailable)
    }

    async fn submit(&self, prio: Priority, job: Job, wait: bool) -> Result<(), LlmError> {
        self.ensure_ready().await?;
        let tx = match prio {
            Priority::Interactive => &self.interactive,
            Priority::Background => &self.background,
        };
        if wait {
            tx.send(job).await.map_err(|_| LlmError::RuntimeUnavailable)
        } else {
            tx.try_send(job).map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => LlmError::Busy,
                mpsc::error::TrySendError::Closed(_) => LlmError::RuntimeUnavailable,
            })
        }
    }

    /// Run a non-streaming request and return the first choice's text. Returns `Busy` if the queue is full.
    pub async fn complete(
        &self,
        prio: Priority,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> Result<String, LlmError> {
        self.complete_inner(prio, req, cancel, false).await
    }

    /// Like [`Scheduler::complete`], but waits for queue capacity instead of returning `Busy`.
    pub async fn complete_wait(
        &self,
        prio: Priority,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> Result<String, LlmError> {
        self.complete_inner(prio, req, cancel, true).await
    }

    async fn complete_inner(
        &self,
        prio: Priority,
        req: ChatRequest,
        cancel: CancellationToken,
        wait: bool,
    ) -> Result<String, LlmError> {
        let (tx, rx) = oneshot::channel();
        self.submit(
            prio,
            Job {
                req,
                cancel: cancel.clone(),
                reply: Reply::Complete(tx),
            },
            wait,
        )
        .await?;
        tokio::select! {
            r = rx => r.unwrap_or(Err(LlmError::Cancelled)),
            _ = cancel.cancelled() => Err(LlmError::Cancelled),
        }
    }

    /// Run a streaming request. The lane is held until the stream ends, errors, is cancelled, or the
    /// returned stream is dropped. Returns `Busy` if the queue is full.
    pub async fn stream(
        &self,
        prio: Priority,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> Result<JobStream, LlmError> {
        self.stream_inner(prio, req, cancel, false).await
    }

    /// Like [`Scheduler::stream`], but waits for queue capacity instead of returning `Busy`.
    pub async fn stream_wait(
        &self,
        prio: Priority,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> Result<JobStream, LlmError> {
        self.stream_inner(prio, req, cancel, true).await
    }

    async fn stream_inner(
        &self,
        prio: Priority,
        req: ChatRequest,
        cancel: CancellationToken,
        wait: bool,
    ) -> Result<JobStream, LlmError> {
        let (tx, rx) = mpsc::channel(64);
        self.submit(
            prio,
            Job {
                req,
                cancel,
                reply: Reply::Stream(tx),
            },
            wait,
        )
        .await?;
        Ok(Box::pin(stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        })))
    }
}

async fn worker(source: Source, mut interactive: mpsc::Receiver<Job>, mut background: mpsc::Receiver<Job>) {
    loop {
        let job = tokio::select! {
            biased;
            Some(j) = interactive.recv() => j,
            Some(j) = background.recv() => j,
            else => break,
        };
        run(&source, job).await;
    }
}

async fn run(source: &Source, job: Job) {
    let Job { req, cancel, reply } = job;
    if cancel.is_cancelled() {
        fail(reply, LlmError::Cancelled).await;
        return;
    }
    let Some(client) = source.client().await else {
        fail(reply, LlmError::RuntimeUnavailable).await;
        return;
    };
    match reply {
        Reply::Complete(tx) => {
            let result = tokio::select! {
                r = client.chat(&req) => r.map(|resp| resp.text()),
                _ = cancel.cancelled() => Err(LlmError::Cancelled),
            };
            let _ = tx.send(result);
        }
        Reply::Stream(tx) => {
            let opened = tokio::select! {
                r = client.chat_stream(&req) => r,
                _ = cancel.cancelled() => Err(LlmError::Cancelled),
            };
            let mut events = match opened {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    return;
                }
            };
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        let _ = tx.send(Err(LlmError::Cancelled)).await;
                        break;
                    }
                    _ = tx.closed() => break, // consumer dropped the stream: free the lane
                    ev = events.next() => match ev {
                        Some(item) => {
                            let stop = item.is_err() || matches!(item, Ok(StreamEvent::Done { .. }));
                            if tx.send(item).await.is_err() || stop {
                                break;
                            }
                        }
                        None => break,
                    },
                }
            }
        }
    }
}

async fn fail(reply: Reply, e: LlmError) {
    match reply {
        Reply::Complete(tx) => {
            let _ = tx.send(Err(e));
        }
        Reply::Stream(tx) => {
            let _ = tx.send(Err(e)).await;
        }
    }
}
