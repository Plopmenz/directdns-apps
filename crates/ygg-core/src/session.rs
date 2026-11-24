//! Synchronous session handle used by desktop UI and the Android JNI bridge.
//!
//! The Yggdrasil core must live inside a Tokio runtime. `Session` owns that
//! runtime on a dedicated thread and exposes a small blocking API.

use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use yggdrasil::tun::InterfaceSource;

use crate::node::Node;
use crate::{Error, Status};

enum Command {
    Stop { reply: oneshot::Sender<()> },
}

/// A node running on its own Tokio runtime thread.
pub struct Session {
    address: String,
    state: Arc<Mutex<Status>>,
    cmd_tx: mpsc::UnboundedSender<Command>,
    thread: Option<JoinHandle<()>>,
}

impl Session {
    /// Starts a node and waits until the core and TUN adapter are up.
    pub fn start(seed: [u8; 32], interface: InterfaceSource) -> Result<Self, Error> {
        let address = crate::key::address_from_seed(&seed);
        let state = Arc::new(Mutex::new(Status {
            running: true,
            message: "Starting".into(),
            address: crate::key::display_address(&address),
            ..Status::default()
        }));
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = oneshot::channel::<Result<(), String>>();
        let thread_state = state.clone();
        let thread_address = address.clone();

        let thread = std::thread::Builder::new()
            .name("directdns-runtime".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = ready_tx.send(Err(format!("cannot start runtime: {error}")));
                        return;
                    }
                };

                runtime.block_on(async move {
                    match Node::start(seed, interface).await {
                        Ok(mut node) => {
                            *lock(&thread_state) = node.status();
                            let _ = ready_tx.send(Ok(()));
                            run_loop(&mut node, cmd_rx, thread_state).await;
                        }
                        Err(error) => {
                            *lock(&thread_state) = Status {
                                running: false,
                                message: error.0.clone(),
                                address: crate::key::display_address(&thread_address),
                                ..Status::default()
                            };
                            let _ = ready_tx.send(Err(error.0));
                        }
                    }
                });
            })
            .map_err(|error| Error(format!("cannot start runtime thread: {error}")))?;

        match ready_rx.blocking_recv() {
            Ok(Ok(())) => Ok(Self {
                address,
                state,
                cmd_tx,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(Error(error))
            }
            Err(_) => {
                let _ = thread.join();
                Err(Error(
                    "runtime thread exited before reporting readiness".into(),
                ))
            }
        }
    }

    /// The node's Yggdrasil IPv6 address (without the `ip:` display prefix).
    pub fn address(&self) -> &str {
        &self.address
    }

    /// The address formatted for display and clipboard copy.
    pub fn display_address(&self) -> String {
        crate::key::display_address(&self.address)
    }

    /// Latest connection status.
    pub fn status(&self) -> Status {
        lock(&self.state).clone()
    }

    /// Stops the node and joins its runtime thread.
    pub fn stop(&mut self) -> Result<(), Error> {
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        let (reply_tx, reply_rx) = oneshot::channel();
        if self.cmd_tx.send(Command::Stop { reply: reply_tx }).is_err() {
            let _ = thread.join();
            return Ok(());
        }
        reply_rx
            .blocking_recv()
            .map_err(|_| Error("runtime thread dropped stop reply".into()))?;
        thread
            .join()
            .map_err(|_| Error("runtime thread panicked".into()))?;
        *lock(&self.state) = Status {
            address: crate::key::display_address(&self.address),
            ..Status::default()
        };
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

async fn run_loop(
    node: &mut Node,
    mut commands: mpsc::UnboundedReceiver<Command>,
    state: Arc<Mutex<Status>>,
) {
    let mut tick = tokio::time::interval(Duration::from_millis(750));
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(Command::Stop { reply }) => {
                    node.stop();
                    *lock(&state) = node.status();
                    let _ = reply.send(());
                    break;
                }
                None => {
                    node.stop();
                    break;
                }
            },
            _ = tick.tick() => {
                *lock(&state) = node.status();
            }
        }
    }
}

fn lock(state: &Arc<Mutex<Status>>) -> MutexGuard<'_, Status> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
