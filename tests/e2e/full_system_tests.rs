use anyhow::Result;
use chrono::Utc;
use acdp_common::{
    ClientConnectionType, ClientId, ClientInfo, IpcMessage, JsonRpcRequest, JsonRpcResponse,
    LogEntry, LogLevel, ProxyId, ProxyInfo, ProxySession, ProxyStats, ProxyStatus, ServerId,
    SessionId, SessionStatus, TransportType,
};
use acdp_transport::BufferedIpcClient;
use acdp_tui::{
    components::ActivityStatus,
    ipc_handler::{AppStateUpdate, IpcHandler},
};
use serde_json::json;
use std::time::Duration as StdDuration;
use tempfile::{tempdir, TempDir};
use tokio::sync::mpsc::Receiver;
use tokio::time::{sleep, timeout, Duration};

#[cfg(test)]
const CONNECT_DELAY_MS: u64 = 150;

async fn start_monitor(
    socket_name: &str,
) -> Result<(
    TempDir,
    IpcHandler,
    Receiver<AppStateUpdate>,
    BufferedIpcClient,
)> {
    let temp_dir = tempdir()?;
    let socket_path = temp_dir.path().join(socket_name);
    let socket = socket_path.to_string_lossy().to_string();

    let (handler, updates) = IpcHandler::new(socket.clone()).await?;
    let client = BufferedIpcClient::new(socket).await;

    // Give the background client task time to connect.
    sleep(Duration::from_millis(CONNECT_DELAY_MS)).await;

    Ok((temp_dir, handler, updates, client))
}

async fn next_update(rx: &mut Receiver<AppStateUpdate>) -> AppStateUpdate {
    timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for IPC update")
        .expect("IPC handler closed channel unexpectedly")
}

async fn shutdown(mut handler: IpcHandler, client: BufferedIpcClient) {
    client.shutdown().await;
    if let Some(handle) = handler.server_handle.take() {
        handle.abort();
    }
}

#[tokio::test]
async fn proxy_lifecycle_updates_flow() -> Result<()> {
    let (_temp_dir, mut handler, mut updates, client) =
        start_monitor("proxy_lifecycle.sock").await?;

    let proxy_id = ProxyId::new();
    let mut stats = ProxyStats {
        proxy_id: proxy_id.clone(),
        total_requests: 0,
        successful_requests: 0,
        failed_requests: 0,
        active_connections: 1,
        uptime: StdDuration::from_secs(1),
        bytes_transferred: 0,
    };

    let proxy_info = ProxyInfo {
        id: proxy_id.clone(),
        name: "proxy-lifecycle".into(),
        listen_address: "127.0.0.1:9000".into(),
        target_command: vec!["python3".into(), "tests/common/test_server.py".into()],
        status: ProxyStatus::Running,
        stats: stats.clone(),
        transport_type: TransportType::Stdio,
    };

    client
        .send(IpcMessage::ProxyStarted(proxy_info.clone()))
        .await?;

    match next_update(&mut updates).await {
        AppStateUpdate::ServerAdded(server) => {
            assert_eq!(server.id, proxy_info.id.0.to_string());
            assert_eq!(server.name, proxy_info.name);
        }
        other => panic!("expected ServerAdded update, got {:?}", other),
    }

    let log_entry = LogEntry::new(
        LogLevel::Request,
        r#"{"method":"tools/list","id":"1"}"#.into(),
        proxy_id.clone(),
    );

    client.send(IpcMessage::LogEntry(log_entry)).await?;

    match next_update(&mut updates).await {
        AppStateUpdate::ActivityAdded(activity) => {
            assert!(activity.action.contains("tools/list"));
            assert_eq!(activity.status, ActivityStatus::Success);
        }
        other => panic!("expected ActivityAdded update, got {:?}", other),
    }

    stats.total_requests = 5;
    stats.successful_requests = 4;
    client.send(IpcMessage::StatsUpdate(stats.clone())).await?;

    match next_update(&mut updates).await {
        AppStateUpdate::ServerStatsUpdate {
            server_id,
            requests_received,
        } => {
            assert_eq!(server_id, proxy_id.0.to_string());
            assert_eq!(requests_received, stats.total_requests);
        }
        other => panic!("expected ServerStatsUpdate, got {:?}", other),
    }

    client
        .send(IpcMessage::ProxyStopped(proxy_id.clone()))
        .await?;

    match next_update(&mut updates).await {
        AppStateUpdate::ServerRemoved(id) => assert_eq!(id, proxy_id.0.to_string()),
        other => panic!("expected ServerRemoved update, got {:?}", other),
    }

    shutdown(handler, client).await;
    Ok(())
}

#[tokio::test]
async fn client_and_session_updates_flow() -> Result<()> {
    let (_temp_dir, mut handler, mut updates, client) =
        start_monitor("client_session.sock").await?;

    let client_id = ClientId::new();
    let mut client_info = ClientInfo::default();
    client_info.id = client_id.clone();
    client_info.name = "demo-client".into();
    client_info.connection_type = ClientConnectionType::Stdio;
    client_info.total_requests = 1;
    client_info.last_activity = Utc::now();

    client
        .send(IpcMessage::ClientConnected(client_info.clone()))
        .await?;

    match next_update(&mut updates).await {
        AppStateUpdate::ClientAdded(client) => {
            assert_eq!(client.id, client_info.id.0.to_string());
            assert_eq!(client.name, client_info.name);
        }
        other => panic!("expected ClientAdded update, got {:?}", other),
    }

    client_info.total_requests = 3;
    client_info.last_activity = Utc::now();

    client
        .send(IpcMessage::ClientUpdated(client_info.clone()))
        .await?;

    match next_update(&mut updates).await {
        AppStateUpdate::ClientUpdated {
            client_id: updated_id,
            requests_sent,
            ..
        } => {
            assert_eq!(updated_id, client_info.id.0.to_string());
            assert_eq!(requests_sent, client_info.total_requests);
        }
        other => panic!("expected ClientUpdated update, got {:?}", other),
    }

    let mut session = ProxySession::default();
    session.id = SessionId::new();
    session.client_id = client_id.clone();
    session.server_id = ServerId::new();
    session.status = SessionStatus::Active;

    client
        .send(IpcMessage::SessionStarted(session.clone()))
        .await?;

    match next_update(&mut updates).await {
        AppStateUpdate::SessionAdded(started) => assert_eq!(started.id, session.id),
        other => panic!("expected SessionAdded update, got {:?}", other),
    }

    let request = JsonRpcRequest {
        jsonrpc: "2.0".into(),
        id: json!(1),
        method: "tools/list".into(),
        params: None,
    };

    client
        .send(IpcMessage::ClientRequest {
            client_id: client_id.clone(),
            request: request.clone(),
            session_id: Some(session.id.clone()),
        })
        .await?;

    match next_update(&mut updates).await {
        AppStateUpdate::ActivityAdded(activity) => {
            assert_eq!(activity.status, ActivityStatus::Processing);
            assert!(activity.action.contains("tools/list"));
        }
        other => panic!("expected ClientRequest activity, got {:?}", other),
    }

    let response = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: json!(1),
        result: Some(json!({"tools": []})),
        error: None,
    };

    client
        .send(IpcMessage::ServerResponse {
            server_id: session.server_id.clone(),
            response,
            session_id: Some(session.id.clone()),
        })
        .await?;

    match next_update(&mut updates).await {
        AppStateUpdate::ActivityAdded(activity) => {
            assert_eq!(activity.status, ActivityStatus::Success);
            assert!(activity.action.contains("Response"));
        }
        other => panic!("expected ServerResponse activity, got {:?}", other),
    }

    client
        .send(IpcMessage::SessionEnded(session.id.clone()))
        .await?;

    match next_update(&mut updates).await {
        AppStateUpdate::SessionRemoved(ended) => assert_eq!(ended, session.id),
        other => panic!("expected SessionRemoved update, got {:?}", other),
    }

    client
        .send(IpcMessage::ClientDisconnected(client_id.clone()))
        .await?;

    match next_update(&mut updates).await {
        AppStateUpdate::ClientRemoved(id) => assert_eq!(id, client_info.id.0.to_string()),
        other => panic!("expected ClientRemoved update, got {:?}", other),
    }

    shutdown(handler, client).await;
    Ok(())
}
