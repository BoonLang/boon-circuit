use super::{DistributedRuntimeError, runtime_error};
use boon_wire::{
    ClientSessionFrame, ClientSessionFrameError, ClientSessionFrameLimits, SessionId,
    decode_client_session_frame, encode_client_session_frame,
};
use serde::{Deserialize, Serialize};

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
    ack_pending: bool,
    resync_expected_next: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct ClientSessionLinkRecovery {
    session_id: [u8; 32],
    generation: u64,
    next_send_operation_sequence: u64,
    peer_acknowledged_through: u64,
    applied_receive_through: u64,
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
            ack_pending: false,
            resync_expected_next: None,
        }
    }

    pub(super) fn rebind(&self, session_id: SessionId, generation: u64) -> Self {
        let mut rebound = self.clone();
        rebound.session_id = session_id;
        rebound.generation = generation;
        rebound.ack_pending = rebound.applied_receive_through != 0;
        rebound.resync_expected_next = None;
        rebound
    }

    pub(super) fn recovery_image(&self) -> ClientSessionLinkRecovery {
        ClientSessionLinkRecovery {
            session_id: self.session_id.into_bytes(),
            generation: self.generation,
            next_send_operation_sequence: self.next_send_operation_sequence,
            peer_acknowledged_through: self.peer_acknowledged_through,
            applied_receive_through: self.applied_receive_through,
        }
    }

    pub(super) fn from_recovery(
        recovery: ClientSessionLinkRecovery,
        graph_hash: [u8; 32],
        schema_hash: [u8; 32],
        graph_revision: u64,
    ) -> Result<Self, DistributedRuntimeError> {
        if recovery.generation == 0
            || recovery.next_send_operation_sequence == 0
            || recovery.peer_acknowledged_through >= recovery.next_send_operation_sequence
        {
            return Err(DistributedRuntimeError::InvalidTransportFrame);
        }
        Ok(Self {
            graph_hash,
            schema_hash,
            graph_revision,
            session_id: SessionId::from_bytes(recovery.session_id),
            generation: recovery.generation,
            next_send_operation_sequence: recovery.next_send_operation_sequence,
            peer_acknowledged_through: recovery.peer_acknowledged_through,
            applied_receive_through: recovery.applied_receive_through,
            ack_pending: false,
            resync_expected_next: None,
        })
    }

    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) fn graph_hash(&self) -> [u8; 32] {
        self.graph_hash
    }

    pub(super) fn schema_hash(&self) -> [u8; 32] {
        self.schema_hash
    }

    pub(super) fn graph_revision(&self) -> u64 {
        self.graph_revision
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

    pub(super) fn peer_acknowledged_through(&self) -> u64 {
        self.peer_acknowledged_through
    }

    pub(super) fn encode_data(
        &self,
        operation_sequence: u64,
        edge_id: [u8; 32],
        semantic_revision: u64,
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
                semantic_revision,
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

    pub(super) fn classify_receive(&self, operation_sequence: u64) -> ReceiveOperation {
        let expected_next = self.applied_receive_through.saturating_add(1);
        if operation_sequence == expected_next {
            ReceiveOperation::Next
        } else if operation_sequence <= self.applied_receive_through {
            ReceiveOperation::Duplicate
        } else {
            ReceiveOperation::Gap { expected_next }
        }
    }

    pub(super) fn accept_receive(
        &mut self,
        operation_sequence: u64,
    ) -> Result<(), DistributedRuntimeError> {
        match self.classify_receive(operation_sequence) {
            ReceiveOperation::Next => {
                self.applied_receive_through = operation_sequence;
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
