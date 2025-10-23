use error_stack::{Report, ResultExt};
use futures::StreamExt;
use serde::{Deserialize, Deserializer, Serialize};
use tokio::io::AsyncWrite;

use crate::{
    DECIMAL_ACCURACY, DecimalType,
    app_error::AppError,
    client::{AllClientsState, ClientId},
    engine::{EngineEvent, EngineHandle},
    transaction::TransactionId,
};

const RECORD_TYPE_DEPOSIT: &str = "deposit";
const RECORD_TYPE_WITHDRAWAL: &str = "withdrawal";
const RECORD_TYPE_DISPUTE: &str = "dispute";
const RECORD_TYPE_RESOLVE: &str = "resolve";
const RECORD_TYPE_CHARGEBACK: &str = "chargeback";

#[derive(Deserialize)]
struct CsvInputRecord {
    #[serde(rename = "type", deserialize_with = "deserialize_lowercase")]
    record_type: String,
    #[serde(rename = "client")]
    client_id: ClientId,
    #[serde(rename = "tx")]
    txid: TransactionId,
    amount: Option<DecimalType>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq, Deserialize))]
pub struct CsvOutputRecord {
    #[serde(rename = "client")]
    client_id: ClientId,
    #[serde(serialize_with = "serialize_decimal")]
    available: DecimalType,
    #[serde(serialize_with = "serialize_decimal")]
    held: DecimalType,
    #[serde(serialize_with = "serialize_decimal")]
    total: DecimalType,
    locked: bool,
}

#[cfg(test)]
impl CsvOutputRecord {
    pub fn client_id(&self) -> ClientId {
        self.client_id
    }
}

fn deserialize_lowercase<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(String::deserialize(deserializer)?.to_lowercase())
}

fn serialize_decimal<S>(dec: &DecimalType, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&dec.round_dp(DECIMAL_ACCURACY).to_string())
}

pub async fn process_input(
    engine: &mut EngineHandle,
    input_csv: impl tokio::io::AsyncRead + Unpin + Send,
    verbose: bool,
) -> Result<(), Report<AppError>> {
    let mut reader = csv_async::AsyncReaderBuilder::new()
        .trim(csv_async::Trim::All)
        .create_deserializer(input_csv);

    let normalised_headers = reader
        .headers()
        .await
        .change_context(AppError)?
        .iter()
        .map(|h| h.to_lowercase())
        .collect::<Vec<_>>();
    reader.set_headers(csv_async::StringRecord::from(normalised_headers));

    let mut records = reader.deserialize::<CsvInputRecord>();

    let mut row_index: usize = 0;
    while let Some(row_result) = records.next().await {
        process_csv_row(engine, row_result, verbose)
            .await
            .attach_with(|| format!("Processing CSV row at index {}", row_index))?;
        row_index += 1;
    }

    Ok(())
}

pub async fn output_client_state(
    all_clients_state: &AllClientsState,
    writer: impl AsyncWrite + Unpin,
) -> Result<(), Report<AppError>> {
    let mut wtr = csv_async::AsyncSerializer::from_writer(writer);

    for (client_id, client) in all_clients_state.iter() {
        wtr.serialize(&CsvOutputRecord {
            client_id: *client_id,
            available: client.available(),
            held: client.held(),
            total: client.total(),
            locked: client.locked(),
        })
        .await
        .change_context(AppError)?;
    }

    wtr.flush().await.change_context(AppError)?;

    Ok(())
}

async fn process_csv_row(
    engine: &mut EngineHandle,
    row_result: Result<CsvInputRecord, csv_async::Error>,
    verbose: bool,
) -> Result<(), Report<AppError>> {
    let row_record = row_result.change_context(AppError)?;

    match row_record.record_type.as_str() {
        RECORD_TYPE_DEPOSIT | RECORD_TYPE_WITHDRAWAL => {
            let amount = row_record
                .amount
                .ok_or_else(|| Report::new(AppError).attach("Missing amount column in CSV"))?;

            // Reject/ignore negative amounts:
            if amount < DecimalType::ZERO {
                if verbose {
                    eprintln!(
                        "Warning: skipping record with negative amount, assumed invalid: type={}, client_id={}, txid={}, amount={}",
                        row_record.record_type, row_record.client_id, row_record.txid, amount
                    );
                }
                return Ok(());
            }

            match row_record.record_type.as_str() {
                RECORD_TYPE_DEPOSIT => {
                    // Will block until event is accepted by channel, providing backpressure to the csv reading:
                    engine
                        .send_event(EngineEvent::Deposit {
                            txid: row_record.txid,
                            client_id: row_record.client_id,
                            amount,
                        })
                        .await?;
                }
                RECORD_TYPE_WITHDRAWAL => {
                    // Will block until event is accepted by channel, providing backpressure to the csv reading:
                    engine
                        .send_event(EngineEvent::Withdrawal {
                            txid: row_record.txid,
                            client_id: row_record.client_id,
                            amount,
                        })
                        .await?;
                }
                _ => unreachable!(),
            }
        }
        RECORD_TYPE_DISPUTE => {
            engine
                .send_event(EngineEvent::Dispute {
                    txid: row_record.txid,
                    client_id: row_record.client_id,
                })
                .await?;
        }
        RECORD_TYPE_RESOLVE => {
            engine
                .send_event(EngineEvent::Resolve {
                    txid: row_record.txid,
                    client_id: row_record.client_id,
                })
                .await?;
        }
        RECORD_TYPE_CHARGEBACK => {
            engine
                .send_event(EngineEvent::Chargeback {
                    txid: row_record.txid,
                    client_id: row_record.client_id,
                })
                .await?;
        }
        other_type => {
            if verbose {
                eprintln!("Warning: skipping unknown record type '{other_type}'")
            }
        }
    }

    Ok(())
}
