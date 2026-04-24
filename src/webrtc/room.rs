use super::peer::{PeerConnection, PeerState};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Room {
    pub room_id: Uuid,
    pub name: String,
    pub creator_id: Uuid,
    pub peers: HashMap<Uuid, PeerConnection>,
    pub max_peers: usize,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomInfo {
    pub room_id: Uuid,
    pub name: String,
    pub peer_count: usize,
    pub max_peers: usize,
    pub is_active: bool,
}

impl Room {
    pub fn new(name: String, creator_id: Uuid, max_peers: usize) -> Self {
        Self {
            room_id: Uuid::new_v4(),
            name,
            creator_id,
            peers: HashMap::new(),
            max_peers,
            created_at: chrono::Utc::now(),
        }
    }

    pub fn add_peer(&mut self, peer: PeerConnection) -> anyhow::Result<()> {
        if self.peers.len() >= self.max_peers {
            return Err(anyhow::anyhow!("Room is full"));
        }
        self.peers.insert(peer.peer_id, peer);
        Ok(())
    }

    pub fn remove_peer(&mut self, peer_id: Uuid) -> Option<PeerConnection> {
        self.peers.remove(&peer_id)
    }

    pub fn get_peer(&self, peer_id: Uuid) -> Option<&PeerConnection> {
        self.peers.get(&peer_id)
    }

    pub fn get_peer_mut(&mut self, peer_id: Uuid) -> Option<&mut PeerConnection> {
        self.peers.get_mut(&peer_id)
    }

    pub fn get_connected_peers(&self) -> Vec<&PeerConnection> {
        self.peers
            .values()
            .filter(|p| p.is_connected())
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    pub fn to_info(&self) -> RoomInfo {
        RoomInfo {
            room_id: self.room_id,
            name: self.name.clone(),
            peer_count: self.peers.len(),
            max_peers: self.max_peers,
            is_active: !self.is_empty(),
        }
    }
}

pub struct RoomManager {
    rooms: HashMap<Uuid, Room>,
}

impl RoomManager {
    pub fn new() -> Self {
        Self {
            rooms: HashMap::new(),
        }
    }

    pub fn create_room(&mut self, name: String, creator_id: Uuid, max_peers: usize) -> Room {
        let room = Room::new(name, creator_id, max_peers);
        self.rooms.insert(room.room_id, room.clone());
        room
    }

    pub fn get_room(&self, room_id: Uuid) -> Option<&Room> {
        self.rooms.get(&room_id)
    }

    pub fn get_room_mut(&mut self, room_id: Uuid) -> Option<&mut Room> {
        self.rooms.get_mut(&room_id)
    }

    pub fn delete_room(&mut self, room_id: Uuid) -> Option<Room> {
        self.rooms.remove(&room_id)
    }

    pub fn list_rooms(&self) -> Vec<RoomInfo> {
        self.rooms.values().map(|r| r.to_info()).collect()
    }

    pub fn cleanup_empty_rooms(&mut self) {
        self.rooms.retain(|_, room| !room.is_empty());
    }
}
