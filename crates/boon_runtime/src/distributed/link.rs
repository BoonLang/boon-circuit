use super::{DistributedRuntimeError, runtime_error};
use boon_wire::{
    ClientSessionDataOperation, ClientSessionFrame, ClientSessionFrameError,
    ClientSessionFrameLimits, SessionId, decode_client_session_frame, encode_client_session_frame,
};
use std::collections::BTreeMap;

const RECEIVE_REPLAY_WINDOW: u64 = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReceiveOperation {
    Next,
    Duplicate,
    Gap { expected_next: u64 },
}

#[derive(Clone)]
pub(super) struct ClientSessionLink {
    graph_hash: [u8; 32],
    schema_hash: [u8; 32],
    graph_revision: u64,
    session_id: SessionId,
    generation: u64,
    next_send_operation_sequence: u64,
    peer_acknowledged_through: u64,
    applied_receive_through: u64,
    receive_fingerprints: BTreeMap<u64, [u8; 32]>,
    ack_pending: bool,
    resync_expected_next: Option<u64>,
}

impl ClientSessionLink {
    pub(super) fn new(
        graph_hash: [u8; 32],
        schema_hash: [u8; 32],
        graph_revision: u64,
        session_id: SessionId,
        generation: u64,
    ) -> Self {
        Self {
            graph_hash,
            schema_hash,
            graph_revision,
            session_id,
            generation,
            next_send_operation_sequence: 1,
            peer_acknowledged_through: 0,
            applied_receive_through: 0,
            receive_fingerprints: BTreeMap::new(),
            ack_pending: false,
            resync_expected_next: None,
        }
    }

    pub(super) fn rebind(
        &self,
        generation: u64,
        peer_applied_through: u64,
    ) -> Result<Self, DistributedRuntimeError> {
        if generation <= self.generation {
            return Err(DistributedRuntimeError::StaleTransportGeneration);
        }
        let mut rebound = self.clone();
        rebound.generation = generation;
        rebound.rebase_send_after_handshake(peer_applied_through)?;
        rebound.ack_pending = rebound.applied_receive_through != 0;
        rebound.resync_expected_next = None;
        Ok(rebound)
    }

    pub(super) fn rebase_send_after_handshake(
        &mut self,
        peer_applied_through: u64,
    ) -> Result<(), DistributedRuntimeError> {
        self.next_send_operation_sequence = peer_applied_through
            .checked_add(1)
            .ok_or_else(|| runtime_error("client/session send operation sequence exhausted"))?;
        self.peer_acknowledged_through = peer_applied_through;
        Ok(())
    }

    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub(super) fn allocate_operation_sequence(&mut self) -> Result<u64, DistributedRuntimeError> {
        let sequence = self.next_send_operation_sequence;
        self.next_send_operation_sequence = sequence
            .checked_add(1)
            .ok_or_else(|| runtime_error("client/session send operation sequence exhausted"))?;
        Ok(sequence)
    }

    pub(super) fn last_send_operation_sequence(&self) -> u64 {
        self.next_send_operation_sequence.saturating_sub(1)
    }

    pub(super) fn applied_receive_through(&self) -> u64 {
        self.applied_receive_through
    }

    pub(super) fn encode_data(
        &self,
        operation_sequence: u64,
        edge_id: [u8; 32],
        operation: ClientSessionDataOperation,
        call_instance_id: Option<[u8; 32]>,
        semantic_revision: u64,
        result_revision: Option<u64>,
        payload: boon_data::Value,
    ) -> Result<Vec<u8>, DistributedRuntimeError> {
        encode_client_session_frame(
            &ClientSessionFrame::Data {
                graph_hash: self.graph_hash,
                graph_revision: self.graph_revision,
                schema_hash: self.schema_hash,
                session_id: self.session_id,
                generation: self.generation,
                operation_sequence,
                ack_through: self.applied_receive_through,
                edge_id,
                operation,
                call_instance_id,
                semantic_revision,
                result_revision,
                payload,
            },
            ClientSessionFrameLimits::default(),
        )
        .map_err(|_| DistributedRuntimeError::InvalidTransportFrame)
    }

    pub(super) fn encode_pending_control(
        &self,
    ) -> Result<Option<(Vec<u8>, SentControl)>, DistributedRuntimeError> {
        let frame = if let Some(expected_next) = self.resync_expected_next {
            (
                ClientSessionFrame::Resync {
                    session_id: self.session_id,
                    generation: self.generation,
                    expected_next,
                },
                SentControl::Resync { expected_next },
            )
        } else if self.ack_pending {
            (
                ClientSessionFrame::Ack {
                    session_id: self.session_id,
                    generation: self.generation,
                    ack_through: self.applied_receive_through,
                },
                SentControl::Ack {
                    ack_through: self.applied_receive_through,
                },
            )
        } else {
            return Ok(None);
        };
        encode_client_session_frame(&frame.0, ClientSessionFrameLimits::default())
            .map(|bytes| Some((bytes, frame.1)))
            .map_err(|_| DistributedRuntimeError::InvalidTransportFrame)
    }

    pub(super) fn has_resync_pending(&self) -> bool {
        self.resync_expected_next.is_some()
    }

    pub(super) fn decode_identity(
        &self,
        bytes: &[u8],
    ) -> Result<ClientSessionFrame, DistributedRuntimeError> {
        let frame = decode_client_session_frame(bytes, ClientSessionFrameLimits::default())
            .map_err(|error| match error {
                ClientSessionFrameError::UnsupportedProtocolVersion(_) => {
                    DistributedRuntimeError::ProtocolMismatch
                }
                _ => DistributedRuntimeError::InvalidTransportFrame,
            })?;
        match &frame {
            ClientSessionFrame::Data {
                graph_hash,
                graph_revision,
                schema_hash,
                session_id,
                generation,
                ..
            } => {
                if graph_hash != &self.graph_hash || *graph_revision != self.graph_revision {
                    return Err(DistributedRuntimeError::ProtocolMismatch);
                }
                if schema_hash != &self.schema_hash {
                    return Err(DistributedRuntimeError::SchemaMismatch);
                }
                self.validate_private_identity(*session_id, *generation)?;
            }
            ClientSessionFrame::Ack {
                session_id,
                generation,
                ..
            }
            | ClientSessionFrame::Resync {
                session_id,
                generation,
                ..
            } => self.validate_private_identity(*session_id, *generation)?,
        }
        Ok(frame)
    }

    fn validate_private_identity(
        &self,
        session_id: SessionId,
        generation: u64,
    ) -> Result<(), DistributedRuntimeError> {
        if session_id != self.session_id {
            return Err(DistributedRuntimeError::InvalidTransportFrame);
        }
        if generation != self.generation {
            return Err(DistributedRuntimeError::StaleTransportGeneration);
        }
        Ok(())
    }

    pub(super) fn classify_receive(
        &self,
        operation_sequence: u64,
        fingerprint: [u8; 32],
    ) -> Result<ReceiveOperation, DistributedRuntimeError> {
        let expected_next = self.applied_receive_through.saturating_add(1);
        if operation_sequence == expected_next {
            Ok(ReceiveOperation::Next)
        } else if operation_sequence <= self.applied_receive_through {
            match self.receive_fingerprints.get(&operation_sequence) {
                Some(accepted) if accepted == &fingerprint => Ok(ReceiveOperation::Duplicate),
                Some(_) => Err(DistributedRuntimeError::InvalidTransportFrame),
                None => Err(DistributedRuntimeError::TransportSequenceMismatch),
            }
        } else {
            Ok(ReceiveOperation::Gap { expected_next })
        }
    }

    pub(super) fn accept_receive(
        &mut self,
        operation_sequence: u64,
        fingerprint: [u8; 32],
    ) -> Result<(), DistributedRuntimeError> {
        match self.classify_receive(operation_sequence, fingerprint)? {
            ReceiveOperation::Next => {
                self.applied_receive_through = operation_sequence;
                self.receive_fingerprints
                    .insert(operation_sequence, fingerprint);
                let retain_from =
                    operation_sequence.saturating_sub(RECEIVE_REPLAY_WINDOW.saturating_sub(1));
                self.receive_fingerprints
                    .retain(|sequence, _| *sequence >= retain_from);
                self.ack_pending = true;
                self.resync_expected_next = None;
                Ok(())
            }
            ReceiveOperation::Duplicate => {
                self.ack_pending = true;
                Ok(())
            }
            ReceiveOperation::Gap { expected_next } => {
                self.resync_expected_next = Some(expected_next);
                Err(DistributedRuntimeError::TransportSequenceMismatch)
            }
        }
    }

    pub(super) fn request_resync(&mut self, expected_next: u64) {
        self.resync_expected_next = Some(expected_next.max(1));
    }

    pub(super) fn accept_peer_ack(
        &mut self,
        ack_through: u64,
    ) -> Result<u64, DistributedRuntimeError> {
        if ack_through > self.last_send_operation_sequence() {
            return Err(DistributedRuntimeError::TransportSequenceMismatch);
        }
        if ack_through > self.peer_acknowledged_through {
            self.peer_acknowledged_through = ack_through;
        }
        Ok(self.peer_acknowledged_through)
    }

    pub(super) fn acknowledge_sent_control(&mut self, control: SentControl) {
        match control {
            SentControl::Ack { ack_through } if self.applied_receive_through == ack_through => {
                self.ack_pending = false;
            }
            SentControl::Resync { expected_next }
                if self.resync_expected_next == Some(expected_next) =>
            {
                self.resync_expected_next = None;
            }
            _ => {}
        }
    }

    pub(super) fn acknowledge_piggybacked_receive(&mut self, ack_through: u64) {
        if self.applied_receive_through == ack_through {
            self.ack_pending = false;
        }
    }

    pub(super) fn pending_control_count(&self) -> usize {
        usize::from(self.resync_expected_next.is_some() || self.ack_pending)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SentControl {
    Ack { ack_through: u64 },
    Resync { expected_next: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link() -> ClientSessionLink {
        ClientSessionLink::new([1; 32], [2; 32], 1, SessionId::from_bytes([3; 32]), 1)
    }

    #[test]
    fn duplicate_receive_requires_the_same_semantic_fingerprint() {
        let mut link = link();
        link.accept_receive(1, [7; 32]).unwrap();
        assert_eq!(
            link.classify_receive(1, [7; 32]).unwrap(),
            ReceiveOperation::Duplicate
        );
        assert!(matches!(
            link.classify_receive(1, [8; 32]),
            Err(DistributedRuntimeError::InvalidTransportFrame)
        ));
    }

    #[test]
    fn stale_duplicates_outside_the_bounded_replay_window_fail() {
        let mut link = link();
        for sequence in 1..=RECEIVE_REPLAY_WINDOW + 1 {
            link.accept_receive(sequence, [sequence as u8; 32]).unwrap();
        }
        assert!(matches!(
            link.classify_receive(1, [1; 32]),
            Err(DistributedRuntimeError::TransportSequenceMismatch)
        ));
    }

    #[test]
    fn rebind_requires_a_new_generation_and_uses_the_peer_receive_watermark() {
        let mut link = link();
        link.accept_receive(1, [9; 32]).unwrap();
        assert!(matches!(
            link.rebind(1, 0),
            Err(DistributedRuntimeError::StaleTransportGeneration)
        ));
        let mut rebound = link.rebind(2, 7).unwrap();
        assert_eq!(
            rebound.classify_receive(1, [9; 32]).unwrap(),
            ReceiveOperation::Duplicate
        );
        assert_eq!(rebound.allocate_operation_sequence().unwrap(), 8);
    }
}
