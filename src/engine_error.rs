use crate::{
    client::ClientId,
    transaction::{TransactionId, TransactionState},
};

/// Errors that can occur within the engine.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// The only hard error in the engine.
    #[error("InternalError")]
    InternalError,

    #[error("Client with ID '{0}' is already locked and can no longer be interacted with")]
    ClientLocked(ClientId),
    #[error("Client with ID '{0}' not found during an operation that requires an existing client")]
    ClientNotFound(ClientId),
    #[error("Insufficient funds for withdrawal")]
    InsufficientFunds,
    #[error(
        "Transaction with ID '{txid}' not in expected state. Expected: {expected:?}, actual: {actual:?}"
    )]
    TxNotInState {
        txid: TransactionId,
        expected: TransactionState,
        actual: TransactionState,
    },
    #[error("Transaction with ID '{0}' not found")]
    TxNotFound(TransactionId),
    #[error(
        "Transaction with ID '{0}' cannot be disputed, only deposit transaction types can be disputed"
    )]
    TxCannotBeDisputed(TransactionId),
    #[error("Transaction with ID '{0}' has already been seen")]
    TxAlreadySeen(TransactionId),
}
