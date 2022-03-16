//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Provides serializable strucutres for bootstrapping the FinalState

use massa_ledger::FinalLedgerBootstrapState;
use massa_models::{DeserializeCompact, SerializeCompact, Slot};
use serde::{Deserialize, Serialize};

/// Represents a snapshot of the final state,
/// which is enough to fully bootstrap a FinalState
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalStateBootstrap {
    /// slot at the output of which the state is attached
    pub(crate) slot: Slot,
    /// final ledger
    pub(crate) ledger: FinalLedgerBootstrapState,
}

/// Allows serializing the FinalStateBootstrap to a compact binary representation
impl SerializeCompact for FinalStateBootstrap {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, massa_models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // final slot
        res.extend(self.slot.to_bytes_compact()?);

        // final ledger
        res.extend(self.ledger.to_bytes_compact()?);

        Ok(res)
    }
}

/// Allows deserializing a FinalStateBootstrap from its compact binary representation
impl DeserializeCompact for FinalStateBootstrap {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), massa_models::ModelsError> {
        let mut cursor = 0usize;

        // final slot
        let (slot, delta) = Slot::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        // final ledger
        let (ledger, delta) = FinalLedgerBootstrapState::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        Ok((FinalStateBootstrap { slot, ledger }, cursor))
    }
}
