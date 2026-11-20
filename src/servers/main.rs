use crate::{
    constants::MAIN_PORT,
    messaging::info_message,
    servers::{
        components,
        packet::{PacketCodec, PacketDebug},
    },
};
use directories::UserDirs;
use futures::{SinkExt, StreamExt};
use log::{debug, error};
use native_windows_gui::error_message;
use std::{
    collections::HashMap,
    net::Ipv4Addr,
    sync::{atomic::AtomicU32, Arc},
};
use tdf::{TdfDeserialize, TdfDeserializer, TdfMap};
use tokio::{
    net::{TcpListener, TcpStream},
    select,
};
use tokio_util::codec::Framed;

use super::{packet::Packet, retriever::OfficialInstance};

pub static SESSION_ID: AtomicU32 = AtomicU32::new(1);

/// Starts the main server proxy. This creates a connection to the Pocket Relay
/// which is upgraded and then used as the main connection fro the game.
pub async fn start_server() {
    // Initializing the underlying TCP listener
    let listener = match TcpListener::bind((Ipv4Addr::UNSPECIFIED, MAIN_PORT)).await {
        Ok(value) => value,
        Err(err) => {
            error_message("Failed to start main", &err.to_string());
            error!("Failed to start main: {}", err);
            return;
        }
    };

    let instance = match OfficialInstance::obtain().await {
        Ok(value) => value,
        Err(err) => {
            error_message("Failed to create official instance", &err.to_string());
            error!("Failed to create official instance: {}", err);
            return;
        }
    };

    let ret = Arc::new(instance);

    // Accept incoming connections
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(value) => value,
            Err(err) => {
                error!("Failed to accept main connection: {}", err);
                break;
            }
        };

        debug!("Main connection ->");

        // Spawn off a new handler for the connection
        _ = tokio::spawn(handle_blaze(stream, ret.clone())).await;
    }
}

async fn handle_blaze(client: TcpStream, ret: Arc<OfficialInstance>) {
    let server = match ret.stream().await {
        Ok(value) => value,
        Err(err) => {
            error!("Failed to obtain session with official server: {}", err);
            return;
        }
    };

    let id = SESSION_ID.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    debug!("Starting session {}", id);

    let mut client_framed = Framed::new(client, PacketCodec);
    let mut server_framed = Framed::new(server, PacketCodec);

    loop {
        select! {
            packet = client_framed.next() => {
                if let Some(Ok(packet)) = packet {
                    _= server_framed.send(packet).await;
                }
            }
            packet = server_framed.next() => {
                if let Some(Ok(packet)) = packet {
                    on_packet_recv(&packet);
                    _ = client_framed.send(packet).await;
                }
            }
        }
    }
}

/// Structure for the response to loading all the settings
#[derive(TdfDeserialize)]
pub struct SettingsResponse {
    /// The settings map
    #[tdf(tag = "SMAP")]
    pub settings: TdfMap<String, String>,
}

fn on_packet_recv(packet: &Packet) {
    if packet.frame.component == components::util::COMPONENT
        && packet.frame.command == components::util::USER_SETTINGS_LOAD_ALL
    {
        let mut tdf = TdfDeserializer::new(&packet.contents);
        let parsed = match SettingsResponse::deserialize(&mut tdf) {
            Ok(value) => value,
            Err(err) => {
                error!("failed to decode settings response");
                error_message("failed to export", "failed to decode data");
                return;
            }
        };

        let settings: HashMap<String, String> = parsed.settings.into_iter().collect();
        let serialized = match serde_json::to_string(&settings) {
            Ok(value) => value,
            Err(err) => {
                error!("failed to serialize settings response");
                error_message("failed to export", "failed to serialize data");
                return;
            }
        };

        let user_dirs = UserDirs::new().expect("failed to get user dir");
        let export_file = user_dirs
            .document_dir()
            .expect("Failed to get document dir")
            .join("player-data-export.dat");

        tokio::spawn(async move {
            match tokio::fs::write(export_file, serialized.as_bytes()).await {
                Ok(_) => {
                    info_message(
                        "export complete",
                        "player data exported you can now close the application",
                    );
                }
                Err(error) => {
                    error!("failed to save settings: {error}");
                    error_message("failed to export", "failed to save data");
                }
            }
        });
    }
}
