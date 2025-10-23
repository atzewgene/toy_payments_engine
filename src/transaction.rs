use std::collections::HashSet;

use error_stack::Report;

use crate::{DecimalType, engine_error::EngineError};

pub type TransactionId = u32;

pub struct Transaction {
    txid: TransactionId,
    kind: TransactionKind,
    state: TransactionState,
}

impl Transaction {
    pub fn new(
        seen_txids: &mut HashSet<TransactionId>,
        txid: TransactionId,
        kind: TransactionKind,
    ) -> Result<Self, Report<EngineError>> {
        let is_new = seen_txids.insert(txid);
        if !is_new {
            Err(Report::from(EngineError::TxAlreadySeen(txid)))
        } else {
            Ok(Self {
                txid,
                kind,
                state: TransactionState::Normal,
            })
        }
    }

    pub fn txid(&self) -> TransactionId {
        self.txid
    }

    pub fn kind(&self) -> &TransactionKind {
        &self.kind
    }

    pub fn amount(&self) -> DecimalType {
        match &self.kind {
            TransactionKind::Deposit { amount } => *amount,
            TransactionKind::Withdrawal { amount } => *amount,
        }
    }

    pub fn mark_disputed(&mut self) -> Result<(), Report<EngineError>> {
        self.check_state_is(TransactionState::Normal)?;
        self.state = TransactionState::Disputed;
        Ok(())
    }

    pub fn mark_resolved(&mut self) -> Result<(), Report<EngineError>> {
        self.check_state_is(TransactionState::Disputed)?;
        self.state = TransactionState::Normal;
        Ok(())
    }

    pub fn mark_chargedback(&mut self) -> Result<(), Report<EngineError>> {
        self.check_state_is(TransactionState::Disputed)?;
        self.state = TransactionState::ChargedBack;
        Ok(())
    }

    fn check_state_is(&self, state: TransactionState) -> Result<(), Report<EngineError>> {
        if self.state != state {
            return Err(Report::from(EngineError::TxNotInState {
                txid: self.txid,
                expected: state,
                actual: self.state,
            }));
        }
        Ok(())
    }
}

pub enum TransactionKind {
    Deposit { amount: DecimalType },
    Withdrawal { amount: DecimalType },
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TransactionState {
    Normal,
    Disputed,
    ChargedBack,
}
