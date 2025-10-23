use std::collections::HashSet;

use error_stack::{Report, ResultExt};

use crate::{
    DecimalType,
    app_error::AppError,
    client::{AllClientsState, ClientId},
    engine_error::EngineError,
    transaction::{Transaction, TransactionId, TransactionKind},
};

const CHANNEL_BUFFER_SIZE: usize = 10_000;

pub enum EngineEvent {
    Deposit {
        txid: TransactionId,
        client_id: ClientId,
        amount: DecimalType,
    },
    Withdrawal {
        txid: TransactionId,
        client_id: ClientId,
        amount: DecimalType,
    },
    Dispute {
        txid: TransactionId,
        client_id: ClientId,
    },
    Resolve {
        txid: TransactionId,
        client_id: ClientId,
    },
    Chargeback {
        txid: TransactionId,
        client_id: ClientId,
    },
    Exit,
}

pub struct EngineState {
    all_clients_state: AllClientsState,
    // To avoid re-processing txids
    seen_txids: HashSet<TransactionId>,
}

impl EngineState {
    pub fn all_clients_state(&self) -> &AllClientsState {
        &self.all_clients_state
    }
}

enum EngineResponse {
    EngineState(EngineState),
}

pub struct EngineHandle {
    engine_event_tx: tokio::sync::mpsc::Sender<EngineEvent>,
    response_rx: tokio::sync::mpsc::Receiver<EngineResponse>,
}

impl EngineHandle {
    /// Resolves once the event has been successfully pushed to the channel.
    pub async fn send_event(&self, event: EngineEvent) -> Result<(), Report<AppError>> {
        self.engine_event_tx
            .send(event)
            .await
            .attach("engine shutdown unexpectedly")
            .change_context(AppError)?;
        Ok(())
    }

    /// Sends the shutdown event and waits for the final engine state to be returned.
    pub async fn shutdown(mut self) -> Result<EngineState, Report<AppError>> {
        self.engine_event_tx
            .send(EngineEvent::Exit)
            .await
            .attach("engine shutdown unexpectedly")
            .change_context(AppError)?;
        match self
            .response_rx
            .recv()
            .await
            .ok_or_else(|| Report::new(AppError).attach("engine shutdown unexpectedly"))?
        {
            EngineResponse::EngineState(engine_state) => Ok(engine_state),
        }
    }
}

enum EventOutput {
    Continue,
    Exit,
}

/// Spawn the engine future that will stay alive until the `Engine` is dropped or an `EngineEvent::Exit`
pub fn spawn_engine(verbose: bool) -> EngineHandle {
    let (engine_event_tx, mut engine_event_rx) = tokio::sync::mpsc::channel(CHANNEL_BUFFER_SIZE);
    let (response_tx, response_rx) = tokio::sync::mpsc::channel(CHANNEL_BUFFER_SIZE);
    tokio::spawn({
        async move {
            let mut engine_state = EngineState {
                all_clients_state: AllClientsState::default(),
                seen_txids: HashSet::new(),
            };
            while let Some(event) = engine_event_rx.recv().await {
                match handle_engine_event(&mut engine_state, event).await {
                    Ok(EventOutput::Exit) => {
                        response_tx
                            .send(EngineResponse::EngineState(engine_state))
                            .await
                            .unwrap();
                        return;
                    }
                    Ok(EventOutput::Continue) => {}
                    Err(report) => match report.current_context() {
                        EngineError::InternalError => {
                            eprintln!("{report:?}");
                            std::process::exit(1);
                        }
                        soft_error => {
                            if verbose {
                                eprintln!("Engine rejected request: {:?}", soft_error);
                            }
                        }
                    },
                }
            }
        }
    });
    EngineHandle {
        engine_event_tx,
        response_rx,
    }
}

async fn handle_engine_event(
    engine: &mut EngineState,
    event: EngineEvent,
) -> Result<EventOutput, Report<EngineError>> {
    match event {
        EngineEvent::Deposit {
            txid,
            client_id,
            amount,
        } => {
            let tx = Transaction::new(
                &mut engine.seen_txids,
                txid,
                TransactionKind::Deposit { amount },
            )?;
            let client = engine
                .all_clients_state
                .get_unlocked_client_mut_or_create(client_id)?;
            client.deposit(tx);
        }
        EngineEvent::Withdrawal {
            txid,
            client_id,
            amount,
        } => {
            let tx = Transaction::new(
                &mut engine.seen_txids,
                txid,
                TransactionKind::Withdrawal { amount },
            )?;
            let client = engine
                .all_clients_state
                .get_unlocked_client_mut_or_create(client_id)?;
            client.withdraw(tx)?;
        }
        EngineEvent::Dispute { txid, client_id } => {
            engine
                .all_clients_state
                .get_unlocked_client_mut(client_id)?
                .ok_or(EngineError::ClientNotFound(client_id))?
                .dispute_transaction(txid)?;
        }
        EngineEvent::Resolve { txid, client_id } => {
            engine
                .all_clients_state
                .get_unlocked_client_mut(client_id)?
                .ok_or(EngineError::ClientNotFound(client_id))?
                .resolve_transaction(txid)?;
        }
        EngineEvent::Chargeback { txid, client_id } => {
            engine
                .all_clients_state
                .get_unlocked_client_mut(client_id)?
                .ok_or(EngineError::ClientNotFound(client_id))?
                .chargeback_transaction(txid)?;
        }
        EngineEvent::Exit => return Ok(EventOutput::Exit),
    };
    Ok(EventOutput::Continue)
}
