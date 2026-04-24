use super::peer::{IceCandidate, PeerConnection, SessionDescription};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SignalingMessage {
    Offer {
        from: Uuid,
        to: Uuid,
        sdp: SessionDescription,
    },
    Answer {
        from: Uuid,
        to: Uuid,
        sdp: SessionDescription,
    },
    IceCandidate {
        from: Uuid,
        to: Uuid,
        candidate: IceCandidate,
    },
    PeerJoined {
        peer_id: Uuid,
        room_id: Uuid,
    },
    PeerLeft {
        peer_id: Uuid,
        room_id: Uuid,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalingRequest {
    pub message_type: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalingResponse {
    pub success: bool,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

pub struct SignalingServer {
    message_queue: Vec<SignalingMessage>,
}

impl SignalingServer {
    pub fn new() -> Self {
        Self {
            message_queue: Vec::new(),
        }
    }

    pub fn queue_message(&mut self, message: SignalingMessage) {
        self.message_queue.push(message);
        tracing::debug!("Queued signaling message: {:?}", message);
    }

    pub fn get_pending_messages(&mut self) -> Vec<SignalingMessage> {
        let messages = self.message_queue.clone();
        self.message_queue.clear();
        messages
    }

    pub fn process_offer(
        &mut self,
        from: Uuid,
        to: Uuid,
        sdp: SessionDescription,
    ) -> SignalingResponse {
        self.queue_message(SignalingMessage::Offer { from, to, sdp });
        SignalingResponse {
            success: true,
            message: "Offer queued for delivery".to_string(),
            data: None,
        }
    }

    pub fn process_answer(
        &mut self,
        from: Uuid,
        to: Uuid,
        sdp: SessionDescription,
    ) -> SignalingResponse {
        self.queue_message(SignalingMessage::Answer { from, to, sdp });
        SignalingResponse {
            success: true,
            message: "Answer queued for delivery".to_string(),
            data: None,
        }
    }

    pub fn process_ice_candidate(
        &mut self,
        from: Uuid,
        to: Uuid,
        candidate: IceCandidate,
    ) -> SignalingResponse {
        self.queue_message(SignalingMessage::IceCandidate {
            from,
            to,
            candidate,
        });
        SignalingResponse {
            success: true,
            message: "ICE candidate queued".to_string(),
            data: None,
        }
    }

    pub fn notify_peer_joined(&mut self, peer_id: Uuid, room_id: Uuid) {
        self.queue_message(SignalingMessage::PeerJoined { peer_id, room_id });
    }

    pub fn notify_peer_left(&mut self, peer_id: Uuid, room_id: Uuid) {
        self.queue_message(SignalingMessage::PeerLeft { peer_id, room_id });
    }
}
