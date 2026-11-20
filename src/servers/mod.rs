use pocket_relay_client_shared::servers::redirector;
use tokio::join;

pub mod components;
pub mod main;
pub mod packet;
pub mod retriever;

pub fn start_servers() {
    tokio::spawn(async move {
        _ = join!(main::start_server(), redirector::start_redirector_server(),);
    });
}
