use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerConnection {
    pub peer_id: Uuid,
    pub connection_id: String,
    pub state: PeerState,
    pub media_types: Vec<MediaType>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PeerState {
    Connecting,
    Connected,
    Disconnected,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MediaType {
    Audio,
    Video,
    ScreenShare,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IceCandidate {
    pub candidate: String,
    pub sdp_mid: Option<String>,
    pub sdp_mline_index: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDescription {
    pub sdp_type: String,
    pub sdp: String,
}

impl PeerConnection {
    pub fn new(peer_id: Uuid) -> Self {
        Self {
            peer_id,
            connection_id: uuid::Uuid::new_v4().to_string(),
            state: PeerState::Connecting,
            media_types: vec![MediaType::Audio, MediaType::Video],
        }
    }

    pub fn set_state(&mut self, state: PeerState) {
        self.state = state;
    }

    pub fn add_media_type(&mut self, media_type: MediaType) {
        if !self.media_types.contains(&media_type) {
            self.media_types.push(media_type);
        }
    }

    pub fn is_connected(&self) -> bool {
        self.state == PeerState::Connected
    }
}
