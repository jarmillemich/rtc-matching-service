use axum::extract::ws::{Message, WebSocket};
use futures::{stream::SplitSink, SinkExt, StreamExt};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{
    oneshot::{self, Sender},
    Mutex, RwLock,
};

use crate::{JoinSessionRequest, JoinSessionResponse};

#[derive(Default)]
pub struct MatchingService {
    // TODO so much indirection here, is it necessary?
    current_hosts: Arc<RwLock<HashMap<String, Arc<RwLock<Arc<Host>>>>>>,
}

// Hosting stuff
impl MatchingService {
    pub async fn init_host_session(
        &self,
        socket: WebSocket,
        host_name: String,
    ) -> anyhow::Result<()> {
        let mut hosts = self.current_hosts.write().await;

        if hosts.contains_key(&host_name) {
            return Err(anyhow::anyhow!("Host already exists"));
        }

        let hosts_clone = self.current_hosts.clone();
        let host_name_clone = host_name.clone();
        let host = Arc::new(RwLock::new(Arc::new(Host::new(&host_name, socket, || {
            tokio::spawn(async move {
                hosts_clone.write().await.remove(&host_name_clone);
            });
        }))));
        hosts.insert(host_name, host);

        Ok(())
    }

    pub async fn end_host_session(&self, host_name: String) -> anyhow::Result<()> {
        let mut hosts = self.current_hosts.write().await;

        if !hosts.contains_key(&host_name) {
            return Err(anyhow::anyhow!("Host does not exist"));
        }

        hosts.remove(&host_name);

        Ok(())
    }

    pub async fn list_sessions(&self) -> anyhow::Result<Vec<String>> {
        let hosts = self.current_hosts.read().await;

        Ok(hosts.keys().cloned().collect())
    }

    pub async fn attempt_join(
        &self,
        join_session: JoinSessionRequest,
    ) -> anyhow::Result<JoinSessionResponse> {
        let hosts = self.current_hosts.read().await;
        let session = hosts
            .get(&join_session.session_name)
            .ok_or_else(|| anyhow::anyhow!("Host does not exist"))?
            .clone();

        let host = session.read().await.clone();
        host.do_join_for(join_session).await
    }
}

struct Host {
    name: String,
    socket: Mutex<SplitSink<WebSocket, Message>>,
    joiners: Arc<RwLock<HashMap<String, Sender<JoinSessionResponse>>>>,
    recv_task: tokio::task::JoinHandle<()>,
}

impl Host {
    pub fn new<OC: FnOnce() -> () + Send + 'static>(
        name: &String,
        socket: WebSocket,
        on_close: OC,
    ) -> Self {
        let name = name.clone();
        let joiners = Arc::new(RwLock::new(
            HashMap::<String, Sender<JoinSessionResponse>>::new(),
        ));

        let (tx, mut rx) = socket.split();

        let task_joiners = joiners.clone();
        let recv_task = tokio::spawn(async move {
            // Receive join responses from this host and forward them appropriately
            while let Some(Ok(Message::Text(text))) = rx.next().await {
                if let Ok(join_response) = serde_json::from_str::<JoinSessionResponse>(&text) {
                    tracing::trace!("Received join response from host: {:?}", join_response);

                    let mut joiners = task_joiners.write().await;

                    if let Some(joiner) = joiners.remove(&join_response.client_name) {
                        let _ = joiner
                            .send(join_response)
                            .map_err(|_| tracing::error!("Failed to send answer to client"));

                        tracing::trace!("Sent join response to client");
                    } else {
                        tracing::warn!("Invalid joiner?");
                    }
                } else {
                    tracing::warn!("Received unknown message from host: {}", text);
                }
            }

            // Drop all of our pending joiners so the receivers can fail
            // TODO is this implicit?
            let mut joiners = task_joiners.write().await;
            joiners.drain();

            on_close();
        });

        let socket = Mutex::new(tx);
        Self {
            name,
            socket,
            joiners,
            recv_task,
        }
    }

    pub async fn do_join_for(
        &self,
        join_session: JoinSessionRequest,
    ) -> anyhow::Result<JoinSessionResponse> {
        let (send_response, on_response) = oneshot::channel::<JoinSessionResponse>();

        // Register our joiner (and be sure to drop the lock)
        {
            let mut joiners = self.joiners.write().await;

            if joiners.contains_key(&join_session.client_name) {
                return Err(anyhow::anyhow!("Client already joining"));
            }

            joiners.insert(join_session.client_name.clone(), send_response);
        }

        // Send the offer to the host
        let join_message = serde_json::to_string(&join_session)?;
        let mut socket = self.socket.lock().await;
        socket
            .send(Message::Text(join_message))
            .await
            .map_err(|_| anyhow::anyhow!("Failed to send offer to host"))?;

        on_response
            .await
            .map_err(|_| anyhow::anyhow!("Failed to receive response from host"))
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.recv_task.abort();

        let _ = self.socket.blocking_lock().close();

        self.joiners.blocking_write().drain();
    }
}
