use std::collections::{HashMap, hash_map};

use error_stack::Report;

use crate::{
    DecimalType,
    engine_error::EngineError,
    transaction::{Transaction, TransactionId, TransactionKind},
};

pub type ClientId = u16;

/// State of all clients in the system.
#[derive(Default)]
pub struct AllClientsState(HashMap<ClientId, ClientState>);

impl AllClientsState {
    /// Return the client if unlocked, creating if missing.
    /// If locked, returns `EngineError::ClientLocked`
    pub fn get_unlocked_client_mut_or_create(
        &mut self,
        client_id: ClientId,
    ) -> Result<&mut ClientState, Report<EngineError>> {
        let client_entry = self.0.entry(client_id);
        if let hash_map::Entry::Occupied(o) = &client_entry {
            if o.get().locked() {
                return Err(Report::from(EngineError::ClientLocked(client_id)));
            }
        }
        Ok(client_entry.or_insert_with(|| ClientState {
            available: 0.into(),
            held: 0.into(),
            locked: false,
            tx_lookup: HashMap::new(),
        }))
    }

    /// Return the client if it exists and is unlocked.
    /// If locked, returns `EngineError::ClientLocked`
    pub fn get_unlocked_client_mut(
        &mut self,
        client_id: ClientId,
    ) -> Result<Option<&mut ClientState>, Report<EngineError>> {
        if let Some(client) = self.0.get_mut(&client_id) {
            if client.locked() {
                return Err(Report::from(EngineError::ClientLocked(client_id)));
            }
            Ok(Some(client))
        } else {
            Ok(None)
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ClientId, &ClientState)> {
        self.0.iter()
    }
}

/// State of a single client in the system.
pub struct ClientState {
    available: DecimalType,
    held: DecimalType,
    locked: bool,
    tx_lookup: HashMap<TransactionId, Transaction>,
}

impl ClientState {
    pub fn available(&self) -> DecimalType {
        self.available
    }

    pub fn held(&self) -> DecimalType {
        self.held
    }

    pub fn total(&self) -> DecimalType {
        self.held + self.available
    }

    pub fn deposit(&mut self, tx: Transaction) {
        self.available += tx.amount();
        self.tx_lookup.insert(tx.txid(), tx);
    }

    pub fn locked(&self) -> bool {
        self.locked
    }

    pub fn withdraw(&mut self, tx: Transaction) -> Result<(), Report<EngineError>> {
        // Withdrawal should fail atomically if insufficient funds
        if self.available < tx.amount() {
            return Err(Report::from(EngineError::InsufficientFunds));
        }
        self.available -= tx.amount();
        self.tx_lookup.insert(tx.txid(), tx);
        Ok(())
    }

    pub fn dispute_transaction(&mut self, txid: TransactionId) -> Result<(), Report<EngineError>> {
        let tx = self
            .tx_lookup
            .get_mut(&txid)
            .ok_or(EngineError::TxNotFound(txid))?;
        tx.mark_disputed()?;
        match tx.kind() {
            TransactionKind::Deposit { amount } => {
                // Not checking for >0 as disputes can allow user to go negative
                self.available -= amount;
                self.held += amount;
            }
            TransactionKind::Withdrawal { .. } => {
                return Err(Report::from(EngineError::TxCannotBeDisputed(txid)));
            }
        }
        Ok(())
    }

    pub fn resolve_transaction(&mut self, txid: TransactionId) -> Result<(), Report<EngineError>> {
        let tx = self
            .tx_lookup
            .get_mut(&txid)
            .ok_or_else(|| Report::from(EngineError::TxNotFound(txid)))?;
        tx.mark_resolved()?;
        match tx.kind() {
            TransactionKind::Deposit { amount } => {
                // Should be impossible that a held amount is less than the disputed amount:
                if self.held < *amount {
                    return Err(Report::from(EngineError::InternalError).attach(format!(
                        "Held funds {} less than resolving dispute amount {} for txid {}",
                        self.held, amount, txid
                    )));
                }
                self.held -= amount;
                self.available += amount;
            }
            TransactionKind::Withdrawal { amount: _ } => {
                return Err(Report::from(EngineError::TxCannotBeDisputed(txid)));
            }
        }
        Ok(())
    }

    pub fn chargeback_transaction(
        &mut self,
        txid: TransactionId,
    ) -> Result<(), Report<EngineError>> {
        let tx = self
            .tx_lookup
            .get_mut(&txid)
            .ok_or_else(|| Report::from(EngineError::TxNotFound(txid)))?;
        tx.mark_chargedback()?;
        match tx.kind() {
            TransactionKind::Deposit { amount } => {
                // Should be impossible that a held amount is less than the disputed amount:
                if self.held < *amount {
                    return Err(Report::from(EngineError::InternalError).attach(format!(
                        "Held funds {} less than resolving dispute amount {} for txid {}",
                        self.held, amount, txid
                    )));
                }
                self.held -= amount;
            }
            TransactionKind::Withdrawal { amount: _ } => {
                return Err(Report::from(EngineError::TxCannotBeDisputed(txid)));
            }
        }
        self.locked = true;
        Ok(())
    }
}
